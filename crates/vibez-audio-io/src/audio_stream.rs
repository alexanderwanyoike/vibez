//! Audio output stream that bridges cpal and the Vibez audio engine.
//!
//! [`AudioOutputStream`] creates a cpal output stream and calls
//! [`AudioEngine::process()`](vibez_engine::engine::AudioEngine::process)
//! inside the real-time audio callback.

use std::fmt;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{
    BuildStreamError, DefaultStreamConfigError, DevicesError, PauseStreamError, PlayStreamError,
    SampleRate, StreamConfig,
};

use vibez_core::constants::{DEFAULT_CHANNELS, DEFAULT_SAMPLE_RATE};
use vibez_engine::engine::AudioEngine;

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
/// let stream = AudioOutputStream::open(engine)?;
/// stream.play()?;
/// // ... use cmd_tx / event_rx on the UI thread ...
/// stream.pause()?;
/// ```
///
/// # Thread safety
///
/// The `AudioEngine` is moved *into* the cpal callback closure. Because
/// `AudioEngine` is `Send` and the closure is `FnMut + Send + 'static`,
/// this satisfies cpal's requirements.  The engine is **not** shared across
/// threads -- it lives exclusively on the audio thread once the stream is
/// built.
///
/// # Future improvement
///
/// Use `audio_thread_priority` to promote the cpal callback thread to
/// real-time priority for glitch-free playback.
pub struct AudioOutputStream {
    stream: cpal::Stream,
    params: StreamParams,
}

impl AudioOutputStream {
    /// Open a new output stream on the default device.
    ///
    /// The `engine` is moved into the audio callback; the caller should
    /// have already extracted the command producer and event consumer from
    /// [`AudioEngine::new()`] before calling this.
    pub fn open(engine: AudioEngine) -> Result<Self, AudioStreamError> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or(AudioStreamError::NoOutputDevice)?;

        Self::open_on_device(engine, &device)
    }

    /// Open a new output stream on a specific device.
    pub fn open_on_device(
        engine: AudioEngine,
        device: &cpal::Device,
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

        let config = StreamConfig {
            channels: channels as u16,
            sample_rate: SampleRate(sample_rate),
            buffer_size: cpal::BufferSize::Default,
        };

        let params = StreamParams {
            sample_rate,
            channels,
        };

        // Move the engine into the callback.  This is lock-free: the engine
        // communicates with the UI thread via rtrb ring buffers, not mutexes.
        let mut engine = engine;
        let ch = channels;

        let stream = device.build_output_stream(
            &config,
            move |data: &mut [f32], _info: &cpal::OutputCallbackInfo| {
                engine.process(data, ch);
            },
            move |err| {
                eprintln!("vibez: audio stream error: {err}");
            },
            None,
        )?;

        Ok(Self { stream, params })
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
}
