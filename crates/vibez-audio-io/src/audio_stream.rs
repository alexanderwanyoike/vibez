//! Audio output stream that bridges cpal and the Vibez audio engine.
//!
//! [`AudioOutputStream`] creates a cpal output stream and calls
//! [`AudioEngine::process()`](vibez_engine::engine::AudioEngine::process)
//! inside the real-time audio callback.

use std::fmt;
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{
    BuildStreamError, DefaultStreamConfigError, DevicesError, PauseStreamError, PlayStreamError,
    SampleFormat, SampleRate, StreamConfig, SupportedStreamConfigsError,
};

use vibez_core::constants::DEFAULT_CHANNELS;
use vibez_engine::engine::AudioEngine;

mod callback;
use callback::{build_output_stream_for_format, OutputCallback, StreamCpuLoad};
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
    /// No default output device found.
    NoOutputDevice,
    /// A persisted named output is not currently visible.
    OutputDeviceNotFound(String),
    /// Could not enumerate devices.
    DevicesError(DevicesError),
    /// Could not query default stream config.
    DefaultConfigError(DefaultStreamConfigError),
    /// Could not query supported stream configurations.
    SupportedConfigsError(SupportedStreamConfigsError),
    /// Could not build the cpal stream.
    BuildStreamError(BuildStreamError),
    /// Could not start the stream.
    PlayError(PlayStreamError),
    /// Could not pause the stream.
    PauseError(PauseStreamError),
    /// The requested sample-rate/buffer combination is unavailable.
    UnsupportedConfiguration(String),
    /// There is no connected stream to start or pause.
    NoActiveStream,
}

impl fmt::Display for AudioStreamError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoOutputDevice => write!(f, "no default audio output device available"),
            Self::OutputDeviceNotFound(name) => {
                write!(f, "audio output device is unavailable: {name}")
            }
            Self::DevicesError(e) => write!(f, "device enumeration error: {e}"),
            Self::DefaultConfigError(e) => write!(f, "default stream config error: {e}"),
            Self::SupportedConfigsError(e) => {
                write!(f, "supported stream config error: {e}")
            }
            Self::BuildStreamError(e) => write!(f, "failed to build audio stream: {e}"),
            Self::PlayError(e) => write!(f, "failed to play audio stream: {e}"),
            Self::PauseError(e) => write!(f, "failed to pause audio stream: {e}"),
            Self::UnsupportedConfiguration(description) => write!(f, "{description}"),
            Self::NoActiveStream => write!(f, "no active audio output stream"),
        }
    }
}

impl std::error::Error for AudioStreamError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::NoOutputDevice
            | Self::OutputDeviceNotFound(_)
            | Self::UnsupportedConfiguration(_)
            | Self::NoActiveStream => None,
            Self::DevicesError(e) => Some(e),
            Self::DefaultConfigError(e) => Some(e),
            Self::SupportedConfigsError(e) => Some(e),
            Self::BuildStreamError(e) => Some(e),
            Self::PlayError(e) => Some(e),
            Self::PauseError(e) => Some(e),
        }
    }
}

impl From<DevicesError> for AudioStreamError {
    fn from(e: DevicesError) -> Self {
        Self::DevicesError(e)
    }
}

impl From<DefaultStreamConfigError> for AudioStreamError {
    fn from(e: DefaultStreamConfigError) -> Self {
        Self::DefaultConfigError(e)
    }
}

impl From<SupportedStreamConfigsError> for AudioStreamError {
    fn from(e: SupportedStreamConfigsError) -> Self {
        Self::SupportedConfigsError(e)
    }
}

impl From<BuildStreamError> for AudioStreamError {
    fn from(e: BuildStreamError) -> Self {
        Self::BuildStreamError(e)
    }
}

impl From<PlayStreamError> for AudioStreamError {
    fn from(e: PlayStreamError) -> Self {
        Self::PlayError(e)
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
    active_device_name: Option<String>,
    /// Shared engine slot.  The audio callback `try_lock`s this each
    /// invocation and calls `engine.process()` if the lock is obtained.
    engine_slot: Arc<Mutex<Option<AudioEngine>>>,
    event_reporter: StreamEventReporter,
    event_rx: Receiver<AudioStreamEvent>,
    cpu_load: StreamCpuLoad,
}

impl AudioOutputStream {
    /// Retain the engine even when no device can be opened yet.
    pub fn idle(engine: AudioEngine) -> Self {
        let (event_reporter, event_rx) = StreamEventReporter::channel();
        Self {
            stream: None,
            params: None,
            active_device_name: None,
            engine_slot: Arc::new(Mutex::new(Some(engine))),
            event_reporter,
            event_rx,
            cpu_load: StreamCpuLoad::default(),
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
        )?;
        stream.play()?;
        output.stream = Some(stream);
        output.params = Some(params);
        output.active_device_name = device_name;
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
        self.event_reporter.report(AudioStreamEvent::Rebuilding);
        let mut previous_stream_running = self.stream.is_some();
        let result: Result<(), AudioStreamError> = (|| {
            let host = cpal::default_host();
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
            )?;

            if let Some(current) = self.stream.as_ref() {
                current.pause()?;
                previous_stream_running = false;
            }
            if let Err(error) = stream.play() {
                if let Some(current) = self.stream.as_ref() {
                    previous_stream_running = current.play().is_ok();
                }
                return Err(error.into());
            }

            self.stream = Some(stream);
            self.params = Some(params);
            self.active_device_name = Some(device_name);
            Ok(())
        })();

        match result {
            Ok(()) => {
                self.event_reporter.report(AudioStreamEvent::Recovered);
                Ok(())
            }
            Err(error) => {
                let event = if previous_stream_running {
                    AudioStreamEvent::ConfigurationRejected(error.to_string())
                } else {
                    AudioStreamEvent::Error(error.to_string())
                };
                self.event_reporter.report(event);
                Err(error)
            }
        }
    }

    /// Internal: build a cpal stream around an existing engine slot.
    fn build_stream(
        engine_slot: Arc<Mutex<Option<AudioEngine>>>,
        device: &cpal::Device,
        requested_sample_rate: Option<u32>,
        buffer_size: Option<u32>,
        event_reporter: StreamEventReporter,
        cpu_load: StreamCpuLoad,
    ) -> Result<(cpal::Stream, StreamParams), AudioStreamError> {
        let supported_config = select_output_config(device, requested_sample_rate, buffer_size)?;
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
            sample_rate,
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

fn buffer_size_supported(size: Option<u32>, range: &cpal::SupportedBufferSize) -> bool {
    let Some(size) = size else {
        return true;
    };
    match range {
        cpal::SupportedBufferSize::Range { min, max } => (*min..=*max).contains(&size),
        cpal::SupportedBufferSize::Unknown => true,
    }
}

fn select_output_config(
    device: &cpal::Device,
    requested_sample_rate: Option<u32>,
    buffer_size: Option<u32>,
) -> Result<cpal::SupportedStreamConfig, AudioStreamError> {
    let default = device.default_output_config()?;
    let sample_rate = requested_sample_rate.unwrap_or(default.sample_rate().0);
    let mut candidates: Vec<_> = device
        .supported_output_configs()?
        .filter(|range| {
            (range.min_sample_rate().0..=range.max_sample_rate().0).contains(&sample_rate)
        })
        .filter(|range| buffer_size_supported(buffer_size, range.buffer_size()))
        .collect();
    candidates.sort_by_key(|range| {
        let channels = range.channels();
        (
            range.sample_format() != SampleFormat::F32,
            channels != DEFAULT_CHANNELS as u16,
            channels,
        )
    });
    candidates
        .into_iter()
        .next()
        .and_then(|range| range.try_with_sample_rate(SampleRate(sample_rate)))
        .ok_or_else(|| {
            AudioStreamError::UnsupportedConfiguration(format!(
                "audio output does not support {sample_rate} Hz with {} buffer",
                buffer_size
                    .map(|size| format!("a {size}-frame"))
                    .unwrap_or_else(|| "the default".into())
            ))
        })
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
