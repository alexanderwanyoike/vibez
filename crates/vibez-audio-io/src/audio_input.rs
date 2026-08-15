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
    DefaultConfig(cpal::DefaultStreamConfigError),
    SupportedConfigs(cpal::SupportedStreamConfigsError),
    Build(cpal::BuildStreamError),
    Play(cpal::PlayStreamError),
    UnsupportedConfiguration(String),
}

impl fmt::Display for AudioInputError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoInputDevice => formatter.write_str("no default Audio Input is available"),
            Self::InputDeviceNotFound(name) => {
                write!(formatter, "Audio Input is unavailable: {name}")
            }
            Self::Devices(error) => write!(formatter, "Audio Input enumeration failed: {error}"),
            Self::DefaultConfig(error) => write!(
                formatter,
                "Audio Input default configuration failed: {error}"
            ),
            Self::SupportedConfigs(error) => {
                write!(formatter, "Audio Input capabilities failed: {error}")
            }
            Self::Build(error) => {
                write!(formatter, "Audio Input stream could not be built: {error}")
            }
            Self::Play(error) => write!(formatter, "Audio Input stream could not start: {error}"),
            Self::UnsupportedConfiguration(description) => formatter.write_str(description),
        }
    }
}

impl std::error::Error for AudioInputError {}

impl From<cpal::DevicesError> for AudioInputError {
    fn from(error: cpal::DevicesError) -> Self {
        Self::Devices(error)
    }
}
impl From<cpal::DefaultStreamConfigError> for AudioInputError {
    fn from(error: cpal::DefaultStreamConfigError) -> Self {
        Self::DefaultConfig(error)
    }
}
impl From<cpal::SupportedStreamConfigsError> for AudioInputError {
    fn from(error: cpal::SupportedStreamConfigsError) -> Self {
        Self::SupportedConfigs(error)
    }
}
impl From<cpal::BuildStreamError> for AudioInputError {
    fn from(error: cpal::BuildStreamError) -> Self {
        Self::Build(error)
    }
}
impl From<cpal::PlayStreamError> for AudioInputError {
    fn from(error: cpal::PlayStreamError) -> Self {
        Self::Play(error)
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
    recording: AtomicBool,
    recording_stopped: AtomicBool,
    record_start_position: AtomicU64,
    overflowed: AtomicBool,
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
            recording: AtomicBool::new(false),
            recording_stopped: AtomicBool::new(true),
            record_start_position: AtomicU64::new(u64::MAX),
            overflowed: AtomicBool::new(false),
            peak_l: AtomicU32::new(0),
            peak_r: AtomicU32::new(0),
        }
    }

    pub fn set_route(&self, route: AudioInputRoute) {
        let encoded = match route {
            AudioInputRoute::Mono { channel } => u32::from(channel),
            AudioInputRoute::Stereo { left } => STEREO_ROUTE_BIT | u32::from(left),
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

    pub fn begin_recording(&self) {
        self.recorded.drain();
        self.overflowed.store(false, Ordering::Release);
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
            f32::from_bits(self.peak_l.load(Ordering::Relaxed)),
            f32::from_bits(self.peak_r.load(Ordering::Relaxed)),
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
        let recording = target.is_some() && self.recording.load(Ordering::Acquire);
        for frame in destination.chunks_mut(channels.max(1)) {
            let input = self.input.pop().unwrap_or([0.0; 2]);
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

    fn push_interleaved<T>(&self, data: &[T], channels: usize)
    where
        T: Copy,
        f32: FromSample<T>,
    {
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
            };
            peak_l = peak_l.max(routed[0].abs());
            peak_r = peak_r.max(routed[1].abs());
            if !self.input.push(routed) {
                self.overflowed.store(true, Ordering::Release);
            }
        }
        self.peak_l.store(peak_l.to_bits(), Ordering::Relaxed);
        self.peak_r.store(peak_r.to_bits(), Ordering::Relaxed);
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
        let selected = select_input_config(&device, sample_rate, buffer_size)?;
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

fn select_input_config(
    device: &cpal::Device,
    sample_rate: u32,
    buffer_size: Option<u32>,
) -> Result<cpal::SupportedStreamConfig, AudioInputError> {
    let default_channels = device
        .default_input_config()
        .ok()
        .map(|config| config.channels());
    let mut candidates: Vec<_> = device
        .supported_input_configs()?
        .filter(|range| {
            (range.min_sample_rate().0..=range.max_sample_rate().0).contains(&sample_rate)
        })
        .filter(|range| match (buffer_size, range.buffer_size()) {
            (Some(size), cpal::SupportedBufferSize::Range { min, max }) => {
                (*min..=*max).contains(&size)
            }
            _ => true,
        })
        .collect();
    candidates.sort_by_key(|range| {
        (
            range.sample_format() != SampleFormat::F32,
            default_channels.is_some_and(|channels| range.channels() != channels),
            std::cmp::Reverse(range.channels()),
        )
    });
    candidates
        .into_iter()
        .next()
        .and_then(|range| range.try_with_sample_rate(SampleRate(sample_rate)))
        .ok_or_else(|| {
            AudioInputError::UnsupportedConfiguration(format!(
                "Audio Input does not support {sample_rate} Hz with {} buffer",
                buffer_size
                    .map(|size| format!("a {size}-frame"))
                    .unwrap_or_else(|| "the default".into())
            ))
        })
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
    Ok(match format {
        SampleFormat::F32 => build!(f32),
        SampleFormat::F64 => build!(f64),
        SampleFormat::I8 => build!(i8),
        SampleFormat::I16 => build!(i16),
        SampleFormat::I24 => build!(cpal::I24),
        SampleFormat::I32 => build!(i32),
        SampleFormat::I64 => build!(i64),
        SampleFormat::U8 => build!(u8),
        SampleFormat::U16 => build!(u16),
        SampleFormat::U32 => build!(u32),
        SampleFormat::U64 => build!(u64),
        unsupported => {
            return Err(AudioInputError::UnsupportedConfiguration(format!(
                "Audio Input sample format {unsupported} is unsupported"
            )))
        }
    })
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

        bridge.end_recording();
        assert!(!bridge.recording_stopped());
        bridge.clock_output(&mut silent_monitor, 2);
        assert!(bridge.recording_stopped());
    }

    #[test]
    fn bounded_overflow_is_reported() {
        let bridge = AudioInputBridge::new(1);
        bridge.push_interleaved(&[0.1f32, 0.2, 0.3, 0.4], 2);
        assert!(bridge.overflowed());
    }
}
