//! Audio host and device enumeration via cpal.
//!
//! This module wraps [`cpal::Host`] to provide a simple interface for
//! discovering output devices and their supported configurations.

use cpal::traits::{DeviceTrait, HostTrait};

/// Information about an available audio output device.
#[derive(Debug, Clone)]
pub struct DeviceInfo {
    /// Human-readable device name.
    pub name: String,
    /// The default output stream configuration (if available).
    pub default_config: Option<StreamConfigInfo>,
    /// All supported output stream configuration ranges.
    pub supported_configs: Vec<SupportedConfigRange>,
}

/// A snapshot of a stream config (sample rate, channels, buffer size).
#[derive(Debug, Clone)]
pub struct StreamConfigInfo {
    pub sample_rate: u32,
    pub channels: u16,
    pub sample_format: String,
}

/// A supported stream configuration range.
#[derive(Debug, Clone)]
pub struct SupportedConfigRange {
    pub channels: u16,
    pub min_sample_rate: u32,
    pub max_sample_rate: u32,
    pub sample_format: String,
}

/// Errors that can occur during host / device enumeration.
#[derive(Debug)]
pub enum AudioHostError {
    /// Failed to enumerate devices.
    DevicesError(cpal::DevicesError),
    /// No default output device found.
    NoDefaultOutputDevice,
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
            Self::NoDefaultOutputDevice => write!(f, "no default audio output device found"),
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
            Self::NoDefaultOutputDevice => None,
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
    /// Returns `Err(AudioHostError::NoDefaultOutputDevice)` if none is available.
    pub fn default_output_device(&self) -> Result<cpal::Device, AudioHostError> {
        self.host
            .default_output_device()
            .ok_or(AudioHostError::NoDefaultOutputDevice)
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
            let info = device_info(&device)?;
            result.push(info);
        }
        Ok(result)
    }

    /// Get info about the default output device.
    pub fn default_output_device_info(&self) -> Result<DeviceInfo, AudioHostError> {
        let device = self.default_output_device()?;
        device_info(&device)
    }
}

impl Default for AudioHost {
    fn default() -> Self {
        Self::new()
    }
}

/// Extract [`DeviceInfo`] from a cpal device.
fn device_info(device: &cpal::Device) -> Result<DeviceInfo, AudioHostError> {
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
            }],
        };

        let debug = format!("{:?}", info);
        assert!(debug.contains("Test Device"));
        assert!(debug.contains("44100"));
    }

    /// Verify that the error type implements Display and Error.
    #[test]
    fn error_display() {
        let err = AudioHostError::NoDefaultOutputDevice;
        let msg = format!("{}", err);
        assert!(msg.contains("no default"));
    }
}
