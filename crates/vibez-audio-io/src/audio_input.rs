//! Hardware Audio Input capture and the lock-free bridge into Vibez's output clock.

use std::cell::UnsafeCell;
use std::fmt;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::Arc;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, Sample, SampleFormat, SampleRate, StreamConfig};
use vibez_core::id::TrackId;
use vibez_core::track::AudioInputRoute;

use crate::stream_config::{select_stream_config, StreamDirection, StreamOpenError};

const DEFAULT_BRIDGE_FRAMES: usize = 262_144;
const STEREO_ROUTE_BIT: u32 = 1 << 31;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AudioInputEvent {
    Error(String),
}

#[derive(Debug)]
pub enum AudioInputError {
    NoInputDevice,
    InputDeviceNotFound(String),
    Devices(cpal::DevicesError),
    StreamOpen(StreamOpenError),
}

impl fmt::Display for AudioInputError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoInputDevice => formatter.write_str("no default Audio Input is available"),
            Self::InputDeviceNotFound(name) => {
                write!(formatter, "Audio Input is unavailable: {name}")
            }
            Self::Devices(error) => write!(formatter, "Audio Input enumeration failed: {error}"),
            Self::StreamOpen(error) => write!(formatter, "Audio Input {error}"),
        }
    }
}

impl std::error::Error for AudioInputError {}

impl From<cpal::DevicesError> for AudioInputError {
    fn from(error: cpal::DevicesError) -> Self {
        Self::Devices(error)
    }
}
impl From<cpal::BuildStreamError> for AudioInputError {
    fn from(error: cpal::BuildStreamError) -> Self {
        Self::StreamOpen(error.into())
    }
}
impl From<cpal::PlayStreamError> for AudioInputError {
    fn from(error: cpal::PlayStreamError) -> Self {
        Self::StreamOpen(error.into())
    }
}
impl From<StreamOpenError> for AudioInputError {
    fn from(error: StreamOpenError) -> Self {
        Self::StreamOpen(error)
    }
}

/// Fixed-size SPSC ring. Its producer and consumer never allocate or lock.
struct StereoRing {
    slots: Box<[UnsafeCell<[f32; 2]>]>,
    head: AtomicUsize,
    tail: AtomicUsize,
}

// Each ring has exactly one producer and one consumer. Slot publication is
// ordered by head/tail Release/Acquire operations.
unsafe impl Sync for StereoRing {}

impl StereoRing {
    fn new(usable_capacity: usize) -> Self {
        let slots = (0..usable_capacity.saturating_add(1).max(2))
            .map(|_| UnsafeCell::new([0.0; 2]))
            .collect::<Vec<_>>()
            .into_boxed_slice();
        Self {
            slots,
            head: AtomicUsize::new(0),
            tail: AtomicUsize::new(0),
        }
    }

    fn push(&self, frame: [f32; 2]) -> bool {
        let head = self.head.load(Ordering::Relaxed);
        let next = (head + 1) % self.slots.len();
        if next == self.tail.load(Ordering::Acquire) {
            return false;
        }
        // SAFETY: only the producer writes the unpublished head slot.
        unsafe {
            *self.slots[head].get() = frame;
        }
        self.head.store(next, Ordering::Release);
        true
    }

    fn pop(&self) -> Option<[f32; 2]> {
        let tail = self.tail.load(Ordering::Relaxed);
        if tail == self.head.load(Ordering::Acquire) {
            return None;
        }
        // SAFETY: Acquire observed producer publication; only this consumer reads this slot.
        let frame = unsafe { *self.slots[tail].get() };
        self.tail
            .store((tail + 1) % self.slots.len(), Ordering::Release);
        Some(frame)
    }

    fn drain(&self) {
        while self.pop().is_some() {}
    }
}

/// Shared input state linking the hardware input callback, output callback,
/// and UI thread without locks.
pub struct AudioInputBridge {
    input: StereoRing,
    recorded: StereoRing,
    route: AtomicU32,
    target_track: AtomicU64,
    monitoring: AtomicBool,
    resample_source_track: AtomicU64,
    recording: AtomicBool,
    recording_stopped: AtomicBool,
    record_start_position: AtomicU64,
    overflowed: AtomicBool,
    underrun_frames: AtomicU64,
    peak_l: AtomicU32,
    peak_r: AtomicU32,
}

impl Default for AudioInputBridge {
    fn default() -> Self {
        Self::new(DEFAULT_BRIDGE_FRAMES)
    }
}

impl AudioInputBridge {
    pub fn new(capacity_frames: usize) -> Self {
        Self {
            input: StereoRing::new(capacity_frames),
            recorded: StereoRing::new(capacity_frames),
            route: AtomicU32::new(0),
            target_track: AtomicU64::new(0),
            monitoring: AtomicBool::new(false),
            resample_source_track: AtomicU64::new(0),
            recording: AtomicBool::new(false),
            recording_stopped: AtomicBool::new(true),
            record_start_position: AtomicU64::new(u64::MAX),
            overflowed: AtomicBool::new(false),
            underrun_frames: AtomicU64::new(0),
            peak_l: AtomicU32::new(0),
            peak_r: AtomicU32::new(0),
        }
    }

    pub fn set_route(&self, route: AudioInputRoute) {
        let encoded = match route {
            AudioInputRoute::Mono { channel } => u32::from(channel),
            AudioInputRoute::Stereo { left } => STEREO_ROUTE_BIT | u32::from(left),
            AudioInputRoute::Resample { .. } => return,
        };
        self.route.store(encoded, Ordering::Release);
    }

    pub fn route(&self) -> AudioInputRoute {
        let encoded = self.route.load(Ordering::Acquire);
        if encoded & STEREO_ROUTE_BIT != 0 {
            AudioInputRoute::Stereo {
                left: (encoded & !STEREO_ROUTE_BIT) as u16,
            }
        } else {
            AudioInputRoute::Mono {
                channel: encoded as u16,
            }
        }
    }

    pub fn set_target(&self, target: Option<TrackId>, monitoring: bool) {
        self.target_track
            .store(target.map_or(0, TrackId::raw), Ordering::Release);
        self.monitoring.store(monitoring, Ordering::Release);
        if target.is_none() {
            self.peak_l.store(0, Ordering::Relaxed);
            self.peak_r.store(0, Ordering::Relaxed);
        }
    }

    pub fn target_track_raw(&self) -> Option<u64> {
        match self.target_track.load(Ordering::Acquire) {
            0 => None,
            raw => Some(raw),
        }
    }

    pub fn set_resample_source(&self, source: Option<TrackId>) {
        self.resample_source_track
            .store(source.map_or(0, TrackId::raw), Ordering::Release);
        self.peak_l.store(0, Ordering::Relaxed);
        self.peak_r.store(0, Ordering::Relaxed);
    }

    pub fn resample_source_track_raw(&self) -> Option<u64> {
        match self.resample_source_track.load(Ordering::Acquire) {
            0 => None,
            raw => Some(raw),
        }
    }

    pub fn begin_recording(&self) {
        self.recorded.drain();
        self.overflowed.store(false, Ordering::Release);
        self.underrun_frames.store(0, Ordering::Release);
        self.record_start_position
            .store(u64::MAX, Ordering::Release);
        self.recording_stopped.store(false, Ordering::Release);
        self.recording.store(true, Ordering::Release);
    }

    pub fn end_recording(&self) {
        self.recording.store(false, Ordering::Release);
    }
    pub fn is_recording(&self) -> bool {
        self.recording.load(Ordering::Acquire)
    }
    pub fn recording_stopped(&self) -> bool {
        self.recording_stopped.load(Ordering::Acquire)
    }
    pub fn overflowed(&self) -> bool {
        self.overflowed.load(Ordering::Acquire)
    }
    pub fn underrun_frames(&self) -> u64 {
        self.underrun_frames.load(Ordering::Acquire)
    }

    pub fn latch_record_start_position(&self, position_samples: u64) {
        if self.is_recording() {
            let _ = self.record_start_position.compare_exchange(
                u64::MAX,
                position_samples,
                Ordering::AcqRel,
                Ordering::Acquire,
            );
        }
    }

    pub fn record_start_position(&self) -> Option<u64> {
        match self.record_start_position.load(Ordering::Acquire) {
            u64::MAX => None,
            position => Some(position),
        }
    }

    pub fn report_overflow(&self) {
        self.overflowed.store(true, Ordering::Release);
    }

    pub fn meter(&self) -> (f32, f32) {
        (
            f32::from_bits(self.peak_l.swap(0, Ordering::AcqRel)),
            f32::from_bits(self.peak_r.swap(0, Ordering::AcqRel)),
        )
    }

    pub fn drain_recorded(&self, destination: &mut Vec<[f32; 2]>) {
        while let Some(frame) = self.recorded.pop() {
            destination.push(frame);
        }
    }

    pub fn clock_output(&self, destination: &mut [f32], channels: usize) -> Option<u64> {
        let target = self.target_track_raw();
        let monitoring = target.is_some() && self.monitoring.load(Ordering::Acquire);
        let recording = target.is_some()
            && self.resample_source_track_raw().is_none()
            && self.recording.load(Ordering::Acquire);
        for frame in destination.chunks_mut(channels.max(1)) {
            let input = match self.input.pop() {
                Some(input) => input,
                None => {
                    if recording {
                        self.underrun_frames.fetch_add(1, Ordering::Relaxed);
                    }
                    [0.0; 2]
                }
            };
            if recording && !self.recorded.push(input) {
                self.overflowed.store(true, Ordering::Release);
            }
            for sample in frame.iter_mut() {
                *sample = 0.0;
            }
            if monitoring {
                if let Some(left) = frame.get_mut(0) {
                    *left = input[0];
                }
                if let Some(right) = frame.get_mut(1) {
                    *right = input[1];
                }
            }
        }
        if !self.recording.load(Ordering::Acquire) {
            // This store happens after every possible recorded-ring push in
            // this callback. The UI may drain only after observing it.
            self.recording_stopped.store(true, Ordering::Release);
        }
        monitoring.then_some(target?)
    }

    /// Capture one output-clock block tapped from a Project Track. The engine
    /// supplies post-device/post-fader samples; this bridge owns only bounded
    /// transfer, metering, and the Stop acknowledgement shared with hardware
    /// input recording.
    pub fn capture_track_output(&self, source: &[f32], channels: usize) {
        if self.resample_source_track_raw().is_none() {
            return;
        }
        let recording = self.recording.load(Ordering::Acquire);
        let mut peak_l = 0.0f32;
        let mut peak_r = 0.0f32;
        for frame in source.chunks(channels.max(1)) {
            let routed = [
                frame.first().copied().unwrap_or(0.0),
                frame
                    .get(1)
                    .copied()
                    .unwrap_or_else(|| frame.first().copied().unwrap_or(0.0)),
            ];
            peak_l = peak_l.max(routed[0].abs());
            peak_r = peak_r.max(routed[1].abs());
            if recording && !self.recorded.push(routed) {
                self.overflowed.store(true, Ordering::Release);
            }
        }
        self.peak_l.fetch_max(peak_l.to_bits(), Ordering::Relaxed);
        self.peak_r.fetch_max(peak_r.to_bits(), Ordering::Relaxed);
        if !recording {
            self.recording_stopped.store(true, Ordering::Release);
        }
    }

    fn push_interleaved<T>(&self, data: &[T], channels: usize)
    where
        T: Copy,
        f32: FromSample<T>,
    {
        if self.resample_source_track_raw().is_some() {
            return;
        }
        let route = self.route();
        let mut peak_l = 0.0f32;
        let mut peak_r = 0.0f32;
        for frame in data.chunks(channels.max(1)) {
            let routed = match route {
                AudioInputRoute::Mono { channel } => {
                    let sample = frame
                        .get(channel as usize)
                        .copied()
                        .map(f32::from_sample)
                        .unwrap_or(0.0);
                    [sample, sample]
                }
                AudioInputRoute::Stereo { left } => {
                    let left = left as usize;
                    [
                        frame
                            .get(left)
                            .copied()
                            .map(f32::from_sample)
                            .unwrap_or(0.0),
                        frame
                            .get(left + 1)
                            .copied()
                            .map(f32::from_sample)
                            .unwrap_or(0.0),
                    ]
                }
                AudioInputRoute::Resample { .. } => [0.0; 2],
            };
            peak_l = peak_l.max(routed[0].abs());
            peak_r = peak_r.max(routed[1].abs());
            if !self.input.push(routed) {
                self.overflowed.store(true, Ordering::Release);
            }
        }
        self.peak_l.fetch_max(peak_l.to_bits(), Ordering::Relaxed);
        self.peak_r.fetch_max(peak_r.to_bits(), Ordering::Relaxed);
    }
}

pub struct AudioInputStream {
    stream: cpal::Stream,
    events: Receiver<AudioInputEvent>,
    pub device_name: String,
    pub channels: usize,
    pub sample_rate: u32,
}

impl AudioInputStream {
    pub fn open(
        device_name: Option<&str>,
        sample_rate: u32,
        buffer_size: Option<u32>,
        bridge: Arc<AudioInputBridge>,
    ) -> Result<Self, AudioInputError> {
        let host = cpal::default_host();
        let device = match device_name {
            Some(name) => host
                .input_devices()?
                .find(|device| device.name().is_ok_and(|candidate| candidate == name))
                .ok_or_else(|| AudioInputError::InputDeviceNotFound(name.to_string()))?,
            None => host
                .default_input_device()
                .ok_or(AudioInputError::NoInputDevice)?,
        };
        let selected = select_stream_config(
            &device,
            StreamDirection::Input,
            Some(sample_rate),
            buffer_size,
            None,
        )?;
        let channels = selected.channels() as usize;
        let config = StreamConfig {
            channels: selected.channels(),
            sample_rate: SampleRate(sample_rate),
            buffer_size: buffer_size.map_or(cpal::BufferSize::Default, cpal::BufferSize::Fixed),
        };
        let (event_tx, events) = mpsc::sync_channel(8);
        let stream = build_input_stream(
            &device,
            &config,
            selected.sample_format(),
            channels,
            bridge,
            event_tx,
        )?;
        stream.play()?;
        Ok(Self {
            stream,
            events,
            device_name: device
                .name()
                .unwrap_or_else(|_| device_name.unwrap_or("System Default").to_string()),
            channels,
            sample_rate,
        })
    }

    pub fn try_next_event(&self) -> Option<AudioInputEvent> {
        self.events.try_recv().ok()
    }
    pub fn is_open(&self) -> bool {
        let _ = &self.stream;
        true
    }
}

fn build_input_stream(
    device: &cpal::Device,
    config: &StreamConfig,
    format: SampleFormat,
    channels: usize,
    bridge: Arc<AudioInputBridge>,
    events: SyncSender<AudioInputEvent>,
) -> Result<cpal::Stream, AudioInputError> {
    macro_rules! build {
        ($sample:ty) => {{
            let callback_bridge = Arc::clone(&bridge);
            let error_events = events.clone();
            device.build_input_stream(
                config,
                move |data: &[$sample], _| callback_bridge.push_interleaved(data, channels),
                move |error| {
                    let _ = error_events.try_send(AudioInputEvent::Error(error.to_string()));
                },
                None,
            )?
        }};
    }
    Ok(dispatch_sample_format!(format, build, |unsupported| {
        AudioInputError::from(StreamOpenError::Unsupported(format!(
            "Audio Input sample format {unsupported} is unsupported"
        )))
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mono_and_stereo_routes_are_applied_before_the_output_clock() {
        let bridge = AudioInputBridge::new(8);
        bridge.set_target(Some(TrackId::new()), true);
        bridge.set_route(AudioInputRoute::Mono { channel: 1 });
        bridge.push_interleaved(&[0.1f32, 0.6, 0.2, -0.4], 2);
        let mut output = [0.0; 4];
        assert!(bridge.clock_output(&mut output, 2).is_some());
        assert_eq!(output, [0.6, 0.6, -0.4, -0.4]);

        bridge.set_route(AudioInputRoute::Stereo { left: 0 });
        bridge.push_interleaved(&[0.3f32, -0.7], 2);
        let mut output = [0.0; 2];
        bridge.clock_output(&mut output, 2);
        assert_eq!(output, [0.3, -0.7]);
    }

    #[test]
    fn recording_is_clocked_by_output_and_drained_off_thread() {
        let bridge = AudioInputBridge::new(8);
        bridge.set_target(Some(TrackId::new()), false);
        bridge.set_route(AudioInputRoute::Stereo { left: 0 });
        bridge.begin_recording();
        assert!(!bridge.recording_stopped());
        bridge.latch_record_start_position(4_800);
        bridge.latch_record_start_position(5_312);
        bridge.push_interleaved(&[0.25f32, -0.5], 2);
        let mut silent_monitor = [1.0; 2];
        assert!(bridge.clock_output(&mut silent_monitor, 2).is_none());
        assert_eq!(silent_monitor, [0.0, 0.0]);
        let mut take = Vec::new();
        bridge.drain_recorded(&mut take);
        assert_eq!(take, vec![[0.25, -0.5]]);
        assert_eq!(bridge.record_start_position(), Some(4_800));
        assert_eq!(bridge.underrun_frames(), 0);

        bridge.end_recording();
        assert!(!bridge.recording_stopped());
        bridge.clock_output(&mut silent_monitor, 2);
        assert!(bridge.recording_stopped());
    }

    #[test]
    fn recording_counts_silence_substituted_for_input_clock_underruns() {
        let bridge = AudioInputBridge::new(8);
        bridge.set_target(Some(TrackId::new()), false);
        bridge.begin_recording();
        let mut output = [1.0; 6];
        bridge.clock_output(&mut output, 2);
        assert_eq!(bridge.underrun_frames(), 3);
        assert_eq!(output, [0.0; 6]);
    }

    #[test]
    fn resample_capture_uses_track_output_without_hardware_underruns() {
        let bridge = AudioInputBridge::new(8);
        let source = TrackId::new();
        bridge.set_resample_source(Some(source));
        bridge.begin_recording();
        bridge.capture_track_output(&[0.25, -0.5, 0.75, -1.0], 2);
        let mut take = Vec::new();
        bridge.drain_recorded(&mut take);
        assert_eq!(take, vec![[0.25, -0.5], [0.75, -1.0]]);
        assert_eq!(bridge.underrun_frames(), 0);
        assert_eq!(bridge.meter(), (0.75, 1.0));

        bridge.end_recording();
        bridge.capture_track_output(&[0.0, 0.0], 2);
        assert!(bridge.recording_stopped());
    }

    #[test]
    fn meter_holds_the_maximum_until_the_ui_reads_it() {
        let bridge = AudioInputBridge::new(8);
        bridge.set_route(AudioInputRoute::Stereo { left: 0 });
        bridge.push_interleaved(&[0.8f32, -0.6], 2);
        bridge.push_interleaved(&[0.1f32, 0.2], 2);
        assert_eq!(bridge.meter(), (0.8, 0.6));
        assert_eq!(bridge.meter(), (0.0, 0.0));
    }

    #[test]
    fn bounded_overflow_is_reported() {
        let bridge = AudioInputBridge::new(1);
        bridge.push_interleaved(&[0.1f32, 0.2, 0.3, 0.4], 2);
        assert!(bridge.overflowed());
    }
}
