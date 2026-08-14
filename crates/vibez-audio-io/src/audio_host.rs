//! Audio host and device enumeration via cpal.
//!
//! This module wraps [`cpal::Host`] to provide a simple interface for
//! discovering input/output devices and their supported configurations.

use cpal::traits::{DeviceTrait, HostTrait};

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
    host: cpal::Host,
}

impl AudioHost {
    /// Create a new `AudioHost` using the platform default host.
    pub fn new() -> Self {
        Self {
            host: cpal::default_host(),
        }
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
        Self::new()
    }
}

/// Extract [`DeviceInfo`] from a cpal device.
fn buffer_size_range(size: &cpal::SupportedBufferSize) -> Option<(u32, u32)> {
    match size {
        cpal::SupportedBufferSize::Range { min, max } => Some((*min, *max)),
        cpal::SupportedBufferSize::Unknown => None,
    }
}

fn output_device_info(device: &cpal::Device) -> Result<DeviceInfo, AudioHostError> {
    let name = device.name()?;

    let default_config = device
        .default_output_config()
        .ok()
        .map(|cfg| StreamConfigInfo {
            sample_rate: cfg.sample_rate().0,
            channels: cfg.channels(),
            sample_format: format!("{:?}", cfg.sample_format()),
        });

    let supported_configs: Vec<SupportedConfigRange> = device
        .supported_output_configs()?
        .map(|range| SupportedConfigRange {
            channels: range.channels(),
            min_sample_rate: range.min_sample_rate().0,
            max_sample_rate: range.max_sample_rate().0,
            sample_format: format!("{:?}", range.sample_format()),
            buffer_size_range: buffer_size_range(range.buffer_size()),
        })
        .collect();

    Ok(DeviceInfo {
        name,
        default_config,
        supported_configs,
    })
}

fn input_device_info(device: &cpal::Device) -> Result<DeviceInfo, AudioHostError> {
    let name = device.name()?;
    let default_config = device
        .default_input_config()
        .ok()
        .map(|cfg| StreamConfigInfo {
            sample_rate: cfg.sample_rate().0,
            channels: cfg.channels(),
            sample_format: format!("{:?}", cfg.sample_format()),
        });
    let supported_configs = device
        .supported_input_configs()?
        .map(|range| SupportedConfigRange {
            channels: range.channels(),
            min_sample_rate: range.min_sample_rate().0,
            max_sample_rate: range.max_sample_rate().0,
            sample_format: format!("{:?}", range.sample_format()),
            buffer_size_range: buffer_size_range(range.buffer_size()),
        })
        .collect();
    Ok(DeviceInfo {
        name,
        default_config,
        supported_configs,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Verify that `AudioHost` can be constructed.  Device availability is
    /// hardware-dependent, so we only assert that construction succeeds.
    #[test]
    fn audio_host_construction() {
        let _host = AudioHost::new();
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
