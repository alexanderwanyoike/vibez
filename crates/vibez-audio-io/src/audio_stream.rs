//! Audio output stream that bridges cpal and the Vibez audio engine.
//!
//! [`AudioOutputStream`] creates a cpal output stream and calls
//! [`AudioEngine::process()`](vibez_engine::engine::AudioEngine::process)
//! inside the real-time audio callback.

use std::fmt;
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{DevicesError, PauseStreamError, SampleRate, StreamConfig};

use crate::audio_host::{AudioBackend, AudioHostError};
use crate::audio_input::AudioInputBridge;
use crate::stream_config::{select_stream_config, StreamDirection, StreamOpenError};
use vibez_core::constants::DEFAULT_CHANNELS;
use vibez_engine::engine::AudioEngine;

mod callback;
#[cfg(test)]
use crate::stream_config::buffer_size_supported;
use callback::{
    build_output_stream_for_format, scratch_buffer_frames, OutputCallback, StreamCpuLoad,
};
#[cfg(test)]
use callback::{callback_load_basis_points, CallbackAction, StreamHealth};

/// A presentation-facing transition in the output stream lifecycle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AudioStreamEvent {
    /// The stream started successfully.
    Running,
    /// The stream failed. The description is suitable for producer-facing UI.
    Error(String),
    /// The existing stream is being replaced.
    Rebuilding,
    /// A replacement stream was built successfully.
    Recovered,
    /// A requested replacement failed while the previous stream stayed live.
    ConfigurationRejected(String),
}

#[derive(Clone)]
struct StreamEventReporter(SyncSender<AudioStreamEvent>);

impl StreamEventReporter {
    fn channel() -> (Self, Receiver<AudioStreamEvent>) {
        let (tx, rx) = mpsc::sync_channel(16);
        (Self(tx), rx)
    }

    fn report(&self, event: AudioStreamEvent) {
        let _ = self.0.try_send(event);
    }
}

/// Errors from [`AudioOutputStream`].
#[derive(Debug)]
pub enum AudioStreamError {
    /// The selected audio backend could not be created.
    AudioHost(AudioHostError),
    /// No default output device found.
    NoOutputDevice,
    /// A persisted named output is not currently visible.
    OutputDeviceNotFound(String),
    /// Could not enumerate devices.
    DevicesError(DevicesError),
    /// Shared stream configuration/build/start failure.
    StreamOpen(StreamOpenError),
    /// Could not pause the stream.
    PauseError(PauseStreamError),
    /// There is no connected stream to start or pause.
    NoActiveStream,
    /// An exclusive driver switch failed and the last working stream could
    /// not be reopened either.
    RollbackFailed {
        requested: Box<AudioStreamError>,
        rollback: Box<AudioStreamError>,
    },
}

impl fmt::Display for AudioStreamError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AudioHost(error) => error.fmt(f),
            Self::NoOutputDevice => write!(f, "no default audio output device available"),
            Self::OutputDeviceNotFound(name) => {
                write!(f, "audio output device is unavailable: {name}")
            }
            Self::DevicesError(e) => write!(f, "device enumeration error: {e}"),
            Self::StreamOpen(error) => error.fmt(f),
            Self::PauseError(e) => write!(f, "failed to pause audio stream: {e}"),
            Self::NoActiveStream => write!(f, "no active audio output stream"),
            Self::RollbackFailed {
                requested,
                rollback,
            } => write!(
                f,
                "requested output failed: {requested}; restoring the previous output also failed: {rollback}"
            ),
        }
    }
}

impl std::error::Error for AudioStreamError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::AudioHost(error) => Some(error),
            Self::NoOutputDevice | Self::OutputDeviceNotFound(_) | Self::NoActiveStream => None,
            Self::DevicesError(e) => Some(e),
            Self::StreamOpen(error) => Some(error),
            Self::PauseError(e) => Some(e),
            Self::RollbackFailed { requested, .. } => Some(requested),
        }
    }
}

impl From<DevicesError> for AudioStreamError {
    fn from(e: DevicesError) -> Self {
        Self::DevicesError(e)
    }
}

impl From<AudioHostError> for AudioStreamError {
    fn from(error: AudioHostError) -> Self {
        Self::AudioHost(error)
    }
}

impl From<StreamOpenError> for AudioStreamError {
    fn from(error: StreamOpenError) -> Self {
        Self::StreamOpen(error)
    }
}

impl From<cpal::BuildStreamError> for AudioStreamError {
    fn from(error: cpal::BuildStreamError) -> Self {
        StreamOpenError::from(error).into()
    }
}

impl From<cpal::PlayStreamError> for AudioStreamError {
    fn from(error: cpal::PlayStreamError) -> Self {
        StreamOpenError::from(error).into()
    }
}

impl From<PauseStreamError> for AudioStreamError {
    fn from(e: PauseStreamError) -> Self {
        Self::PauseError(e)
    }
}

/// The actual sample rate and channel count negotiated with the device.
#[derive(Debug, Clone, Copy)]
pub struct StreamParams {
    pub sample_rate: u32,
    pub channels: usize,
}

/// The complete hardware request used to build an output stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputStreamConfig {
    /// Audio API used to enumerate and open the requested device.
    pub backend: AudioBackend,
    /// `None` follows the platform's System Default output.
    pub device_name: Option<String>,
    /// `None` uses the selected device's default sample rate.
    pub sample_rate: Option<u32>,
    /// `None` lets the backend choose its buffer size.
    pub buffer_size: Option<u32>,
}

impl Default for OutputStreamConfig {
    fn default() -> Self {
        Self {
            backend: AudioBackend::System,
            device_name: None,
            sample_rate: None,
            buffer_size: Some(512),
        }
    }
}

/// An audio output stream backed by cpal.
///
/// The stream runs the [`AudioEngine`] in the real-time callback.
///
/// # Usage
///
/// ```ignore
/// let (engine, cmd_tx, event_rx) = AudioEngine::new();
/// let stream = AudioOutputStream::open(engine, None)?;
/// // ... use cmd_tx / event_rx on the UI thread ...
/// stream.pause()?;
/// ```
///
/// # Thread safety
///
/// The `AudioEngine` is held in an `Arc<Mutex<Option<AudioEngine>>>` shared
/// between replacement cpal callbacks. The callback uses `try_lock` so it
/// never blocks if a retiring and replacement stream briefly overlap.
pub struct AudioOutputStream {
    stream: Option<cpal::Stream>,
    params: Option<StreamParams>,
    active_device: Option<cpal::Device>,
    active_device_name: Option<String>,
    active_backend: Option<AudioBackend>,
    active_config: Option<OutputStreamConfig>,
    /// Shared engine slot.  The audio callback `try_lock`s this each
    /// invocation and calls `engine.process_block()` if the lock is obtained.
    engine_slot: Arc<Mutex<Option<AudioEngine>>>,
    event_reporter: StreamEventReporter,
    event_rx: Receiver<AudioStreamEvent>,
    cpu_load: StreamCpuLoad,
    input_bridge: Arc<AudioInputBridge>,
}

impl AudioOutputStream {
    /// Retain the engine even when no device can be opened yet.
    pub fn idle(engine: AudioEngine) -> Self {
        let (event_reporter, event_rx) = StreamEventReporter::channel();
        Self {
            stream: None,
            params: None,
            active_device: None,
            active_device_name: None,
            active_backend: None,
            active_config: None,
            engine_slot: Arc::new(Mutex::new(Some(engine))),
            event_reporter,
            event_rx,
            cpu_load: StreamCpuLoad::default(),
            input_bridge: Arc::new(AudioInputBridge::default()),
        }
    }

    /// Open a new output stream on the default device.
    ///
    /// The `engine` is moved into a shared slot accessible from the audio
    /// callback; the caller should have already extracted the command
    /// producer and event consumer from [`AudioEngine::new()`] before
    /// calling this.
    ///
    /// If `buffer_size` is `Some(n)`, a fixed buffer size of `n` frames is
    /// requested from the device.  If `None`, the device's default is used.
    pub fn open(engine: AudioEngine, buffer_size: Option<u32>) -> Result<Self, AudioStreamError> {
        let mut output = Self::idle(engine);
        output.reconfigure(OutputStreamConfig {
            buffer_size,
            ..OutputStreamConfig::default()
        })?;
        Ok(output)
    }

    /// Open a new output stream on a specific device.
    pub fn open_on_device(
        engine: AudioEngine,
        device: &cpal::Device,
        buffer_size: Option<u32>,
    ) -> Result<Self, AudioStreamError> {
        let mut output = Self::idle(engine);
        let device_name = device.name().ok();
        let (stream, params) = Self::build_stream(
            Arc::clone(&output.engine_slot),
            device,
            None,
            buffer_size,
            output.event_reporter.clone(),
            output.cpu_load.clone(),
            Arc::clone(&output.input_bridge),
        )?;
        stream.play()?;
        output.stream = Some(stream);
        output.params = Some(params);
        output.active_device = Some(device.clone());
        output.active_device_name = device_name;
        output.active_backend = Some(AudioBackend::System);
        output.active_config = Some(OutputStreamConfig {
            backend: AudioBackend::System,
            device_name: output.active_device_name.clone(),
            sample_rate: output.sample_rate(),
            buffer_size,
        });
        output.event_reporter.report(AudioStreamEvent::Running);
        Ok(output)
    }

    /// Apply a complete output configuration while preserving the engine and
    /// all its state (tracks, clips, effects, plugins, transport, etc.).
    ///
    /// The replacement is built before the working stream is paused. If build
    /// or start fails, the previous stream remains (or is resumed) instead of
    /// leaving the session silent.
    pub fn reconfigure(&mut self, request: OutputStreamConfig) -> Result<(), AudioStreamError> {
        self.reconfigure_candidates(&[request]).map(|_| ())
    }

    /// Try a preferred configuration followed by compatible fallbacks as one
    /// transaction. If an exclusive ASIO driver must be released and every
    /// candidate fails, Vibez reopens the exact configuration which was live
    /// before the attempt.
    ///
    /// The returned index identifies the candidate that became active.
    pub fn reconfigure_candidates(
        &mut self,
        requests: &[OutputStreamConfig],
    ) -> Result<usize, AudioStreamError> {
        assert!(
            !requests.is_empty(),
            "at least one output request is required"
        );
        self.event_reporter.report(AudioStreamEvent::Rebuilding);
        let mut previous_stream_running = self.stream.is_some();
        let previous_config = self.active_config.clone();
        let requested_backend = requests[0].backend;
        // Most native hosts allow a replacement stream to be prepared while
        // the old one remains live. ASIO drivers commonly own one exclusive
        // buffer set, so attempting to load the same driver twice fails. Drop
        // that driver before rebuilding while the engine remains safely held
        // in `engine_slot`.
        let exclusive_stream_released =
            requires_exclusive_reopen(self.active_backend, requested_backend);
        if exclusive_stream_released {
            self.clear_active_stream();
            previous_stream_running = false;
        }
        let mut last_error = None;
        for (index, request) in requests.iter().enumerate() {
            match self.try_reconfigure(request, &mut previous_stream_running) {
                Ok(()) => {
                    self.event_reporter.report(AudioStreamEvent::Recovered);
                    return Ok(index);
                }
                Err(error) => last_error = Some(error),
            }
        }

        let mut error = last_error.expect("a non-empty request list always produces a result");
        if exclusive_stream_released {
            let mut rollback_stream_running = false;
            match previous_config.as_ref() {
                Some(config) => match self.try_reconfigure(config, &mut rollback_stream_running) {
                    Ok(()) => previous_stream_running = true,
                    Err(rollback) => {
                        error = AudioStreamError::RollbackFailed {
                            requested: Box::new(error),
                            rollback: Box::new(rollback),
                        };
                    }
                },
                None => previous_stream_running = false,
            }
        }

        let event = if previous_stream_running {
            AudioStreamEvent::ConfigurationRejected(error.to_string())
        } else {
            AudioStreamEvent::Error(error.to_string())
        };
        self.event_reporter.report(event);
        Err(error)
    }

    fn try_reconfigure(
        &mut self,
        request: &OutputStreamConfig,
        previous_stream_running: &mut bool,
    ) -> Result<(), AudioStreamError> {
        let result: Result<(), AudioStreamError> = (|| {
            let host = request.backend.create_host()?;
            let device = resolve_output_device(&host, request.device_name.as_deref())?;
            let device_name = device.name().unwrap_or_else(|_| {
                request
                    .device_name
                    .clone()
                    .unwrap_or_else(|| "System Default".into())
            });

            // Build while the current stream remains live. CPAL streams start
            // paused, so there is still only one callback driving the engine.
            let (stream, params) = Self::build_stream(
                Arc::clone(&self.engine_slot),
                &device,
                request.sample_rate,
                request.buffer_size,
                self.event_reporter.clone(),
                self.cpu_load.clone(),
                Arc::clone(&self.input_bridge),
            )?;

            if let Some(current) = self.stream.as_ref() {
                current.pause()?;
                *previous_stream_running = false;
            }
            if let Err(error) = stream.play() {
                if let Some(current) = self.stream.as_ref() {
                    *previous_stream_running = current.play().is_ok();
                }
                return Err(error.into());
            }

            self.stream = Some(stream);
            self.params = Some(params);
            self.active_device = Some(device);
            self.active_device_name = Some(device_name);
            self.active_backend = Some(request.backend);
            self.active_config = Some(OutputStreamConfig {
                backend: request.backend,
                device_name: self.active_device_name.clone(),
                sample_rate: self.sample_rate(),
                buffer_size: request.buffer_size,
            });
            Ok(())
        })();
        result
    }

    fn clear_active_stream(&mut self) {
        self.stream = None;
        self.params = None;
        self.active_device = None;
        self.active_device_name = None;
        self.active_backend = None;
        self.active_config = None;
    }

    /// Internal: build a cpal stream around an existing engine slot.
    fn build_stream(
        engine_slot: Arc<Mutex<Option<AudioEngine>>>,
        device: &cpal::Device,
        requested_sample_rate: Option<u32>,
        buffer_size: Option<u32>,
        event_reporter: StreamEventReporter,
        cpu_load: StreamCpuLoad,
        input_bridge: Arc<AudioInputBridge>,
    ) -> Result<(cpal::Stream, StreamParams), AudioStreamError> {
        let supported_config = select_stream_config(
            device,
            StreamDirection::Output,
            requested_sample_rate,
            buffer_size,
            Some(DEFAULT_CHANNELS as u16),
        )?;
        let sample_rate = supported_config.sample_rate().0;
        let channels = supported_config.channels() as usize;

        let buf_size = match buffer_size {
            Some(size) => cpal::BufferSize::Fixed(size),
            None => cpal::BufferSize::Default,
        };

        let config = StreamConfig {
            channels: channels as u16,
            sample_rate: SampleRate(sample_rate),
            buffer_size: buf_size,
        };

        let params = StreamParams {
            sample_rate,
            channels,
        };

        let callback = OutputCallback::new(
            engine_slot,
            cpu_load,
            channels,
            buffer_size.unwrap_or(512),
            scratch_buffer_frames(buffer_size, supported_config.buffer_size()),
            sample_rate,
            input_bridge,
        );
        let stream = build_output_stream_for_format(
            device,
            &config,
            supported_config.sample_format(),
            callback,
            event_reporter,
        )?;

        Ok((stream, params))
    }

    /// Start (or resume) audio playback.
    pub fn play(&self) -> Result<(), AudioStreamError> {
        let stream = self
            .stream
            .as_ref()
            .ok_or(AudioStreamError::NoActiveStream)?;
        match stream.play().map_err(AudioStreamError::from) {
            Ok(()) => {
                self.event_reporter.report(AudioStreamEvent::Running);
                Ok(())
            }
            Err(error) => {
                self.event_reporter
                    .report(AudioStreamEvent::Error(error.to_string()));
                Err(error)
            }
        }
    }

    /// Pause audio playback.
    ///
    /// Not all backends support pausing at the hardware level; this may
    /// silently do nothing on some platforms.
    pub fn pause(&self) -> Result<(), AudioStreamError> {
        self.stream
            .as_ref()
            .ok_or(AudioStreamError::NoActiveStream)?
            .pause()?;
        Ok(())
    }

    /// Return the negotiated stream parameters.
    pub fn params(&self) -> Option<StreamParams> {
        self.params
    }

    /// Return the sample rate negotiated with the device.
    pub fn sample_rate(&self) -> Option<u32> {
        self.params.map(|params| params.sample_rate)
    }

    /// Return the channel count negotiated with the device.
    pub fn channels(&self) -> Option<usize> {
        self.params.map(|params| params.channels)
    }

    /// The concrete device currently driving the callback.
    pub fn active_device_name(&self) -> Option<&str> {
        self.active_device_name.as_deref()
    }

    /// Clone the concrete device which owns the active output stream.
    ///
    /// ASIO requires input and output buffers to be prepared through clones of
    /// the same CPAL device object. Re-enumerating a matching driver name can
    /// create an independent driver state which cannot join the live stream.
    pub fn active_device(&self) -> Option<cpal::Device> {
        self.active_device.clone()
    }

    /// Backend currently driving the output callback.
    pub fn active_backend(&self) -> Option<AudioBackend> {
        self.active_backend
    }

    pub fn is_running(&self) -> bool {
        self.stream.is_some()
    }

    /// Return the next lifecycle event without blocking the UI thread.
    pub fn try_next_event(&self) -> Option<AudioStreamEvent> {
        self.event_rx.try_recv().ok()
    }

    /// Smoothed audio callback load as a percentage of the current buffer
    /// deadline. Values over 100% mean the callback missed its deadline.
    pub fn cpu_load_percent(&self) -> f32 {
        self.cpu_load.percent()
    }

    /// Lock-free bridge shared with an on-demand hardware input stream.
    pub fn input_bridge(&self) -> Arc<AudioInputBridge> {
        Arc::clone(&self.input_bridge)
    }
}

fn requires_exclusive_reopen(
    active_backend: Option<AudioBackend>,
    requested_backend: AudioBackend,
) -> bool {
    active_backend == Some(AudioBackend::Asio) && requested_backend == AudioBackend::Asio
}

fn resolve_output_device(
    host: &cpal::Host,
    requested_name: Option<&str>,
) -> Result<cpal::Device, AudioStreamError> {
    let Some(requested_name) = requested_name else {
        return host
            .default_output_device()
            .ok_or(AudioStreamError::NoOutputDevice);
    };
    host.output_devices()?
        .find(|device| device.name().is_ok_and(|name| name == requested_name))
        .ok_or_else(|| AudioStreamError::OutputDeviceNotFound(requested_name.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// Verify that error variants display correctly.
    #[test]
    fn error_display() {
        let err = AudioStreamError::NoOutputDevice;
        let msg = format!("{err}");
        assert!(msg.contains("no default"));
    }

    /// Verify `StreamParams` can be constructed and copied.
    #[test]
    fn stream_params_copy() {
        let p = StreamParams {
            sample_rate: 44100,
            channels: 2,
        };
        let p2 = p;
        assert_eq!(p2.sample_rate, 44100);
        assert_eq!(p2.channels, 2);
    }

    /// Verify `StreamParams` default values are sensible.
    #[test]
    fn stream_params_default_values() {
        let p = StreamParams {
            sample_rate: vibez_core::constants::DEFAULT_SAMPLE_RATE,
            channels: DEFAULT_CHANNELS,
        };
        assert_eq!(p.sample_rate, 44100);
        assert_eq!(p.channels, 2);
    }

    /// Verify `BufferSize::Fixed` is constructed for `Some(1024)`.
    #[test]
    fn buffer_size_fixed_config() {
        let buf = match Some(1024u32) {
            Some(size) => cpal::BufferSize::Fixed(size),
            None => cpal::BufferSize::Default,
        };
        assert!(matches!(buf, cpal::BufferSize::Fixed(1024)));
    }

    #[test]
    fn asio_releases_its_exclusive_driver_before_reconfiguration() {
        assert!(requires_exclusive_reopen(
            Some(AudioBackend::Asio),
            AudioBackend::Asio
        ));
        assert!(!requires_exclusive_reopen(
            Some(AudioBackend::System),
            AudioBackend::Asio
        ));
        assert!(!requires_exclusive_reopen(
            Some(AudioBackend::Asio),
            AudioBackend::System
        ));
    }

    #[test]
    fn conversion_scratch_is_preallocated_without_trusting_unbounded_hints() {
        let bounded = cpal::SupportedBufferSize::Range {
            min: 128,
            max: 4096,
        };
        assert_eq!(scratch_buffer_frames(Some(512), &bounded), 4096);

        let unbounded = cpal::SupportedBufferSize::Range {
            min: 0,
            max: u32::MAX,
        };
        assert_eq!(scratch_buffer_frames(Some(512), &unbounded), 16_384);
        assert_eq!(
            scratch_buffer_frames(None, &cpal::SupportedBufferSize::Unknown),
            16_384
        );
    }

    #[test]
    fn stream_error_latches_callback_into_silence_and_yield_mode() {
        let health = StreamHealth::default();
        assert_eq!(health.callback_action(), CallbackAction::Process);

        health.mark_failed();

        assert_eq!(health.callback_action(), CallbackAction::SilenceAndYield);
        assert_eq!(health.callback_action(), CallbackAction::SilenceAndYield);
    }

    #[test]
    fn callback_load_compares_processing_time_with_the_device_deadline() {
        assert_eq!(
            callback_load_basis_points(Duration::from_millis(5), 480, 48_000),
            5_000
        );
        assert_eq!(
            callback_load_basis_points(Duration::from_millis(12), 480, 48_000),
            12_000
        );
        assert_eq!(
            callback_load_basis_points(Duration::from_secs(1), 0, 48_000),
            0
        );
    }

    #[test]
    fn callback_load_is_smoothed_without_locking_the_audio_thread() {
        let load = StreamCpuLoad::default();
        load.record(Duration::from_millis(5), 480, 48_000);
        assert_eq!(load.percent(), 50.0);
        load.record(Duration::from_millis(10), 480, 48_000);
        assert_eq!(load.percent(), 60.0);
    }

    #[test]
    fn stream_lifecycle_reports_error_rebuild_and_recovery_in_order() {
        let (reporter, events) = StreamEventReporter::channel();

        reporter.report(AudioStreamEvent::Running);
        reporter.report(AudioStreamEvent::Error(
            "device disconnected mid-session".into(),
        ));
        reporter.report(AudioStreamEvent::Rebuilding);
        reporter.report(AudioStreamEvent::Recovered);
        reporter.report(AudioStreamEvent::ConfigurationRejected(
            "unsupported rate".into(),
        ));

        assert_eq!(events.try_recv(), Ok(AudioStreamEvent::Running));
        assert_eq!(
            events.try_recv(),
            Ok(AudioStreamEvent::Error(
                "device disconnected mid-session".into()
            ))
        );
        assert_eq!(events.try_recv(), Ok(AudioStreamEvent::Rebuilding));
        assert_eq!(events.try_recv(), Ok(AudioStreamEvent::Recovered));
        assert_eq!(
            events.try_recv(),
            Ok(AudioStreamEvent::ConfigurationRejected(
                "unsupported rate".into()
            ))
        );
        assert!(events.try_recv().is_err());
    }

    /// Verify `BufferSize::Default` is used for `None`.
    #[test]
    fn buffer_size_none_uses_default() {
        let buf: cpal::BufferSize = match None::<u32> {
            Some(size) => cpal::BufferSize::Fixed(size),
            None => cpal::BufferSize::Default,
        };
        assert!(matches!(buf, cpal::BufferSize::Default));
    }

    #[test]
    fn fixed_buffer_must_fit_the_device_range() {
        let range = cpal::SupportedBufferSize::Range { min: 128, max: 512 };
        assert!(!buffer_size_supported(Some(64), &range));
        assert!(buffer_size_supported(Some(128), &range));
        assert!(buffer_size_supported(Some(512), &range));
        assert!(!buffer_size_supported(Some(1024), &range));
        assert!(buffer_size_supported(None, &range));
    }

    /// Calling `promote_current_thread_to_real_time` doesn't panic
    /// (may fail without permissions, that's OK).
    #[test]
    fn promote_does_not_panic() {
        let result = audio_thread_priority::promote_current_thread_to_real_time(512, 44100);
        // We don't assert Ok — CI may lack permissions. Just ensure no panic.
        drop(result);
    }
}
