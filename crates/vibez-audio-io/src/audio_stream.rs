//! Audio output stream that bridges cpal and the Vibez audio engine.
//!
//! [`AudioOutputStream`] creates a cpal output stream and calls
//! [`AudioEngine::process()`](vibez_engine::engine::AudioEngine::process)
//! inside the real-time audio callback.

use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{
    BuildStreamError, DefaultStreamConfigError, DevicesError, PauseStreamError, PlayStreamError,
    SampleRate, StreamConfig,
};

use vibez_core::constants::{DEFAULT_CHANNELS, DEFAULT_SAMPLE_RATE};
use vibez_engine::engine::AudioEngine;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CallbackAction {
    Process,
    SilenceAndYield,
}

#[derive(Clone, Default)]
struct StreamHealth(Arc<AtomicBool>);

impl StreamHealth {
    fn mark_failed(&self) {
        self.0.store(true, Ordering::Release);
    }

    fn callback_action(&self) -> CallbackAction {
        if self.0.load(Ordering::Acquire) {
            CallbackAction::SilenceAndYield
        } else {
            CallbackAction::Process
        }
    }
}

/// Errors from [`AudioOutputStream`].
#[derive(Debug)]
pub enum AudioStreamError {
    /// No default output device found.
    NoOutputDevice,
    /// Could not enumerate devices.
    DevicesError(DevicesError),
    /// Could not query default stream config.
    DefaultConfigError(DefaultStreamConfigError),
    /// Could not build the cpal stream.
    BuildStreamError(BuildStreamError),
    /// Could not start the stream.
    PlayError(PlayStreamError),
    /// Could not pause the stream.
    PauseError(PauseStreamError),
}

impl fmt::Display for AudioStreamError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoOutputDevice => write!(f, "no default audio output device available"),
            Self::DevicesError(e) => write!(f, "device enumeration error: {e}"),
            Self::DefaultConfigError(e) => write!(f, "default stream config error: {e}"),
            Self::BuildStreamError(e) => write!(f, "failed to build audio stream: {e}"),
            Self::PlayError(e) => write!(f, "failed to play audio stream: {e}"),
            Self::PauseError(e) => write!(f, "failed to pause audio stream: {e}"),
        }
    }
}

impl std::error::Error for AudioStreamError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::NoOutputDevice => None,
            Self::DevicesError(e) => Some(e),
            Self::DefaultConfigError(e) => Some(e),
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

/// An audio output stream backed by cpal.
///
/// The stream runs the [`AudioEngine`] in the real-time callback.
///
/// # Usage
///
/// ```ignore
/// let (engine, cmd_tx, event_rx) = AudioEngine::new();
/// let stream = AudioOutputStream::open(engine, None)?;
/// stream.play()?;
/// // ... use cmd_tx / event_rx on the UI thread ...
/// stream.pause()?;
/// ```
///
/// # Thread safety
///
/// The `AudioEngine` is held in an `Arc<Mutex<Option<AudioEngine>>>` shared
/// between the UI thread and the cpal callback closure.  The audio callback
/// uses `try_lock` so it never blocks — if the UI thread briefly holds the
/// lock (during [`reconfigure`](AudioOutputStream::reconfigure)), the
/// callback outputs silence for that single buffer (a few ms, inaudible).
///
/// Outside of reconfigure the lock is uncontended, so `try_lock` always
/// succeeds with no overhead beyond the atomic check.
pub struct AudioOutputStream {
    stream: cpal::Stream,
    params: StreamParams,
    /// Shared engine slot.  The audio callback `try_lock`s this each
    /// invocation and calls `engine.process()` if the lock is obtained.
    engine_slot: Arc<Mutex<Option<AudioEngine>>>,
}

impl AudioOutputStream {
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
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or(AudioStreamError::NoOutputDevice)?;

        Self::open_on_device(engine, &device, buffer_size)
    }

    /// Open a new output stream on a specific device.
    pub fn open_on_device(
        engine: AudioEngine,
        device: &cpal::Device,
        buffer_size: Option<u32>,
    ) -> Result<Self, AudioStreamError> {
        let engine_slot = Arc::new(Mutex::new(Some(engine)));
        Self::build_stream(engine_slot, device, buffer_size)
    }

    /// Reconfigure the stream with a new buffer size, preserving the engine
    /// and all its state (tracks, clips, effects, plugins, transport, etc.).
    ///
    /// The old cpal stream is dropped and a new one is created.  During the
    /// brief moment the engine is being moved between streams, the old
    /// callback outputs silence (one buffer, inaudible).
    pub fn reconfigure(&mut self, buffer_size: Option<u32>) -> Result<(), AudioStreamError> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or(AudioStreamError::NoOutputDevice)?;

        // Pause the old stream so the callback stops firing.
        let _ = self.stream.pause();

        // Take the engine out of the shared slot.  The old callback will
        // output silence if it fires between pause and drop.
        let engine_slot = Arc::clone(&self.engine_slot);

        // Build a new stream that reuses the same engine slot.
        let new = Self::build_stream(engine_slot, &device, buffer_size)?;

        self.stream = new.stream;
        self.params = new.params;
        // engine_slot is already the same Arc
        Ok(())
    }

    /// Internal: build a cpal stream around an existing engine slot.
    fn build_stream(
        engine_slot: Arc<Mutex<Option<AudioEngine>>>,
        device: &cpal::Device,
        buffer_size: Option<u32>,
    ) -> Result<Self, AudioStreamError> {
        let supported_config = device.default_output_config()?;

        // Prefer our default sample rate if the device supports it, otherwise
        // fall back to whatever the device reports as its default.
        let sample_rate = {
            let dev_rate = supported_config.sample_rate().0;
            if dev_rate == DEFAULT_SAMPLE_RATE {
                DEFAULT_SAMPLE_RATE
            } else {
                dev_rate
            }
        };

        let channels = supported_config.channels() as usize;
        // Clamp to at least DEFAULT_CHANNELS so the engine always gets stereo.
        let channels = channels.max(DEFAULT_CHANNELS);

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

        let ch = channels;

        // Promote the audio callback thread to realtime on first invocation.
        // The handle must be kept alive for the lifetime of the stream on
        // platforms where dropping it demotes the thread (macOS/Windows).
        let buffer_frames = buffer_size.unwrap_or(512);
        let rt_sample_rate = sample_rate;
        #[allow(clippy::type_complexity)]
        let mut rt_state: Option<Result<audio_thread_priority::RtPriorityHandle, ()>> = None;

        let slot = Arc::clone(&engine_slot);
        let health = StreamHealth::default();
        let callback_health = health.clone();

        let stream = device.build_output_stream(
            &config,
            move |data: &mut [f32], _info: &cpal::OutputCallbackInfo| {
                if callback_health.callback_action() == CallbackAction::SilenceAndYield {
                    if let Some(Ok(handle)) = rt_state.take() {
                        if let Err(error) =
                            audio_thread_priority::demote_current_thread_from_real_time(handle)
                        {
                            eprintln!("vibez: failed to demote disconnected audio thread: {error}");
                        }
                    }
                    data.fill(0.0);
                    // ALSA can hot-loop callbacks after a USB device disappears.
                    // This path is already unrecoverable for the current stream;
                    // yielding prevents CPU spin and Linux RLIMIT_RTTIME SIGXCPU.
                    std::thread::sleep(Duration::from_millis(10));
                    return;
                }
                if rt_state.is_none() {
                    rt_state = Some(
                        match audio_thread_priority::promote_current_thread_to_real_time(
                            buffer_frames,
                            rt_sample_rate,
                        ) {
                            Ok(handle) => {
                                eprintln!("vibez: audio thread promoted to realtime");
                                Ok(handle)
                            }
                            Err(e) => {
                                eprintln!("vibez: failed to promote audio thread: {e}");
                                Err(())
                            }
                        },
                    );
                }

                // try_lock: never blocks the audio thread.  If the UI thread
                // holds the lock during reconfigure, output silence.
                if let Ok(mut guard) = slot.try_lock() {
                    if let Some(engine) = guard.as_mut() {
                        engine.process(data, ch);
                        return;
                    }
                }
                // Fallback: silence
                data.fill(0.0);
            },
            move |err| {
                health.mark_failed();
                eprintln!("vibez: audio stream error: {err}");
            },
            None,
        )?;

        Ok(Self {
            stream,
            params,
            engine_slot,
        })
    }

    /// Start (or resume) audio playback.
    pub fn play(&self) -> Result<(), AudioStreamError> {
        self.stream.play()?;
        Ok(())
    }

    /// Pause audio playback.
    ///
    /// Not all backends support pausing at the hardware level; this may
    /// silently do nothing on some platforms.
    pub fn pause(&self) -> Result<(), AudioStreamError> {
        self.stream.pause()?;
        Ok(())
    }

    /// Return the negotiated stream parameters.
    pub fn params(&self) -> StreamParams {
        self.params
    }

    /// Return the sample rate negotiated with the device.
    pub fn sample_rate(&self) -> u32 {
        self.params.sample_rate
    }

    /// Return the channel count negotiated with the device.
    pub fn channels(&self) -> usize {
        self.params.channels
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            sample_rate: DEFAULT_SAMPLE_RATE,
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

    /// Verify `BufferSize::Default` is used for `None`.
    #[test]
    fn buffer_size_none_uses_default() {
        let buf: cpal::BufferSize = match None::<u32> {
            Some(size) => cpal::BufferSize::Fixed(size),
            None => cpal::BufferSize::Default,
        };
        assert!(matches!(buf, cpal::BufferSize::Default));
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
