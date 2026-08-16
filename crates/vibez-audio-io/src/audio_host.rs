//! Audio host and device enumeration via cpal.
//!
//! This module wraps [`cpal::Host`] to provide a simple interface for
//! discovering input/output devices and their supported configurations.

use cpal::traits::{DeviceTrait, HostTrait};
use serde::{Deserialize, Serialize};

/// Stable application-facing audio backend choice.
///
/// `System` maps to the native CPAL default on every platform. `Asio` remains
/// serializable on non-Windows systems so a settings file copied between
/// machines can be read and safely repaired by the application.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AudioBackend {
    #[default]
    System,
    Asio,
}

impl AudioBackend {
    /// Backends compiled into this Vibez build for the current platform.
    pub const fn available() -> &'static [Self] {
        #[cfg(target_os = "windows")]
        {
            &[Self::System, Self::Asio]
        }
        #[cfg(not(target_os = "windows"))]
        {
            &[Self::System]
        }
    }

    pub fn is_available(self) -> bool {
        Self::available().contains(&self)
    }

    /// Construct the CPAL host represented by this stable backend choice.
    pub fn create_host(self) -> Result<cpal::Host, AudioHostError> {
        match self {
            Self::System => Ok(cpal::default_host()),
            Self::Asio => {
                #[cfg(target_os = "windows")]
                {
                    cpal::host_from_id(cpal::HostId::Asio)
                        .map_err(|_| AudioHostError::BackendUnavailable(self))
                }
                #[cfg(not(target_os = "windows"))]
                {
                    Err(AudioHostError::BackendUnavailable(self))
                }
            }
        }
    }
}

impl std::fmt::Display for AudioBackend {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::System => formatter.write_str(system_backend_name()),
            Self::Asio => formatter.write_str("ASIO"),
        }
    }
}

const fn system_backend_name() -> &'static str {
    #[cfg(target_os = "windows")]
    {
        "WASAPI"
    }
    #[cfg(target_os = "macos")]
    {
        "CoreAudio"
    }
    #[cfg(target_os = "linux")]
    {
        "ALSA"
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        "System Audio"
    }
}

/// Information about an available audio input or output device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceInfo {
    /// Human-readable device name.
    pub name: String,
    /// The default stream configuration (if available).
    pub default_config: Option<StreamConfigInfo>,
    /// All supported stream configuration ranges.
    pub supported_configs: Vec<SupportedConfigRange>,
}

/// A snapshot of a stream config (sample rate, channels, buffer size).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamConfigInfo {
    pub sample_rate: u32,
    pub channels: u16,
    pub sample_format: String,
}

/// A supported stream configuration range.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupportedConfigRange {
    pub channels: u16,
    pub min_sample_rate: u32,
    pub max_sample_rate: u32,
    pub sample_format: String,
    /// Fixed buffer-size bounds when the backend exposes them.
    pub buffer_size_range: Option<(u32, u32)>,
}

/// One refreshable snapshot of the platform audio devices.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AudioDeviceCatalog {
    pub default_input_name: Option<String>,
    pub default_output_name: Option<String>,
    pub input_devices: Vec<DeviceInfo>,
    pub output_devices: Vec<DeviceInfo>,
}

/// Errors that can occur during host / device enumeration.
#[derive(Debug)]
pub enum AudioHostError {
    /// The selected backend is not compiled in or cannot initialise.
    BackendUnavailable(AudioBackend),
    /// Failed to enumerate devices.
    DevicesError(cpal::DevicesError),
    /// No default device found for the requested direction.
    NoDefaultDevice(&'static str),
    /// Could not query the device name.
    DeviceNameError(cpal::DeviceNameError),
    /// Could not query supported output configs.
    SupportedConfigsError(cpal::SupportedStreamConfigsError),
    /// Could not query the default output config.
    DefaultConfigError(cpal::DefaultStreamConfigError),
}

impl std::fmt::Display for AudioHostError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BackendUnavailable(backend) => {
                write!(f, "{backend} audio backend is unavailable")
            }
            Self::DevicesError(e) => write!(f, "failed to enumerate audio devices: {e}"),
            Self::NoDefaultDevice(direction) => {
                write!(f, "no default audio {direction} device found")
            }
            Self::DeviceNameError(e) => write!(f, "failed to get device name: {e}"),
            Self::SupportedConfigsError(e) => {
                write!(f, "failed to query supported configs: {e}")
            }
            Self::DefaultConfigError(e) => write!(f, "failed to query default config: {e}"),
        }
    }
}

impl std::error::Error for AudioHostError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::BackendUnavailable(_) => None,
            Self::DevicesError(e) => Some(e),
            Self::NoDefaultDevice(_) => None,
            Self::DeviceNameError(e) => Some(e),
            Self::SupportedConfigsError(e) => Some(e),
            Self::DefaultConfigError(e) => Some(e),
        }
    }
}

impl From<cpal::DevicesError> for AudioHostError {
    fn from(e: cpal::DevicesError) -> Self {
        Self::DevicesError(e)
    }
}

impl From<cpal::DeviceNameError> for AudioHostError {
    fn from(e: cpal::DeviceNameError) -> Self {
        Self::DeviceNameError(e)
    }
}

impl From<cpal::SupportedStreamConfigsError> for AudioHostError {
    fn from(e: cpal::SupportedStreamConfigsError) -> Self {
        Self::SupportedConfigsError(e)
    }
}

impl From<cpal::DefaultStreamConfigError> for AudioHostError {
    fn from(e: cpal::DefaultStreamConfigError) -> Self {
        Self::DefaultConfigError(e)
    }
}

/// Wrapper around [`cpal::Host`] that provides ergonomic device enumeration.
pub struct AudioHost {
    backend: AudioBackend,
    host: cpal::Host,
}

impl AudioHost {
    /// Create an `AudioHost` for one explicit application backend.
    pub fn new(backend: AudioBackend) -> Result<Self, AudioHostError> {
        Ok(Self {
            backend,
            host: backend.create_host()?,
        })
    }

    pub fn backend(&self) -> AudioBackend {
        self.backend
    }

    /// Return a reference to the inner [`cpal::Host`].
    pub fn inner(&self) -> &cpal::Host {
        &self.host
    }

    /// Get the default output device.
    ///
    /// Returns `Err(AudioHostError::NoDefaultDevice("output"))` if unavailable.
    pub fn default_output_device(&self) -> Result<cpal::Device, AudioHostError> {
        self.host
            .default_output_device()
            .ok_or(AudioHostError::NoDefaultDevice("output"))
    }

    /// Get the default output device's stream configuration.
    pub fn default_output_config(&self) -> Result<cpal::SupportedStreamConfig, AudioHostError> {
        let device = self.default_output_device()?;
        let config = device.default_output_config()?;
        Ok(config)
    }

    /// List all available output devices with their info.
    pub fn output_devices(&self) -> Result<Vec<DeviceInfo>, AudioHostError> {
        let devices = self.host.output_devices()?;
        let mut result = Vec::new();
        for device in devices {
            let info = output_device_info(&device)?;
            result.push(info);
        }
        Ok(result)
    }

    /// List all available input devices with their capabilities.
    pub fn input_devices(&self) -> Result<Vec<DeviceInfo>, AudioHostError> {
        let devices = self.host.input_devices()?;
        let mut result = Vec::new();
        for device in devices {
            let info = input_device_info(&device)?;
            result.push(info);
        }
        Ok(result)
    }

    /// Capture input/output names and capabilities in one refreshable value.
    pub fn catalog(&self) -> Result<AudioDeviceCatalog, AudioHostError> {
        let default_input_name = self
            .host
            .default_input_device()
            .and_then(|device| device.name().ok());
        let default_output_name = self
            .host
            .default_output_device()
            .and_then(|device| device.name().ok());
        Ok(AudioDeviceCatalog {
            default_input_name,
            default_output_name,
            input_devices: self.input_devices()?,
            output_devices: self.output_devices()?,
        })
    }

    /// Get info about the default output device.
    pub fn default_output_device_info(&self) -> Result<DeviceInfo, AudioHostError> {
        let device = self.default_output_device()?;
        output_device_info(&device)
    }
}

impl Default for AudioHost {
    fn default() -> Self {
        Self {
            backend: AudioBackend::System,
            host: cpal::default_host(),
        }
    }
}

/// Extract [`DeviceInfo`] from a cpal device.
fn buffer_size_range(size: &cpal::SupportedBufferSize) -> Option<(u32, u32)> {
    match size {
        cpal::SupportedBufferSize::Range { min, max } => Some((*min, *max)),
        cpal::SupportedBufferSize::Unknown => None,
    }
}

#[derive(Clone, Copy)]
enum StreamDirection {
    Input,
    Output,
}

fn stream_config_info(config: cpal::SupportedStreamConfig) -> StreamConfigInfo {
    StreamConfigInfo {
        sample_rate: config.sample_rate().0,
        channels: config.channels(),
        sample_format: format!("{:?}", config.sample_format()),
    }
}

fn supported_config_info(range: cpal::SupportedStreamConfigRange) -> SupportedConfigRange {
    SupportedConfigRange {
        channels: range.channels(),
        min_sample_rate: range.min_sample_rate().0,
        max_sample_rate: range.max_sample_rate().0,
        sample_format: format!("{:?}", range.sample_format()),
        buffer_size_range: buffer_size_range(range.buffer_size()),
    }
}

fn device_info(
    device: &cpal::Device,
    direction: StreamDirection,
) -> Result<DeviceInfo, AudioHostError> {
    let name = device.name()?;
    let default_config = match direction {
        StreamDirection::Input => device.default_input_config(),
        StreamDirection::Output => device.default_output_config(),
    }
    .ok()
    .map(stream_config_info);
    let supported_configs = match direction {
        StreamDirection::Input => device
            .supported_input_configs()?
            .map(supported_config_info)
            .collect(),
        StreamDirection::Output => device
            .supported_output_configs()?
            .map(supported_config_info)
            .collect(),
    };
    Ok(DeviceInfo {
        name,
        default_config,
        supported_configs,
    })
}

fn output_device_info(device: &cpal::Device) -> Result<DeviceInfo, AudioHostError> {
    device_info(device, StreamDirection::Output)
}

fn input_device_info(device: &cpal::Device) -> Result<DeviceInfo, AudioHostError> {
    device_info(device, StreamDirection::Input)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Verify that `AudioHost` can be constructed.  Device availability is
    /// hardware-dependent, so we only assert that construction succeeds.
    #[test]
    fn audio_host_construction() {
        let host = AudioHost::new(AudioBackend::System).unwrap();
        assert_eq!(host.backend(), AudioBackend::System);
    }

    #[test]
    fn backend_names_match_the_native_platform() {
        assert!(AudioBackend::available().contains(&AudioBackend::System));
        #[cfg(target_os = "windows")]
        assert_eq!(AudioBackend::System.to_string(), "WASAPI");
        #[cfg(target_os = "macos")]
        assert_eq!(AudioBackend::System.to_string(), "CoreAudio");
        #[cfg(target_os = "linux")]
        assert_eq!(AudioBackend::System.to_string(), "ALSA");
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_build_includes_the_asio_host() {
        let host = AudioHost::new(AudioBackend::Asio).unwrap();
        assert_eq!(host.backend(), AudioBackend::Asio);
    }

    #[test]
    fn backend_identity_roundtrips_for_cross_platform_settings() {
        let encoded = serde_json::to_string(&AudioBackend::Asio).unwrap();
        assert_eq!(encoded, "\"asio\"");
        assert_eq!(
            serde_json::from_str::<AudioBackend>(&encoded).unwrap(),
            AudioBackend::Asio
        );
    }

    /// Verify that `DeviceInfo`, `StreamConfigInfo`, and `SupportedConfigRange`
    /// can be constructed and formatted.
    #[test]
    fn info_types_are_debug() {
        let info = DeviceInfo {
            name: "Test Device".into(),
            default_config: Some(StreamConfigInfo {
                sample_rate: 44100,
                channels: 2,
                sample_format: "F32".into(),
            }),
            supported_configs: vec![SupportedConfigRange {
                channels: 2,
                min_sample_rate: 44100,
                max_sample_rate: 192000,
                sample_format: "F32".into(),
                buffer_size_range: Some((64, 2048)),
            }],
        };

        let debug = format!("{info:?}");
        assert!(debug.contains("Test Device"));
        assert!(debug.contains("44100"));
    }

    /// Verify that the error type implements Display and Error.
    #[test]
    fn error_display() {
        let err = AudioHostError::NoDefaultDevice("output");
        let msg = format!("{err}");
        assert!(msg.contains("no default"));
    }
}
