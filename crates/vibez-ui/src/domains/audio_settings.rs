//! Application-scoped audio hardware configuration.

use std::fmt;

use serde::{Deserialize, Serialize};
use vibez_audio_io::audio_host::{
    AudioBackend, AudioDeviceCatalog, DeviceInfo, SupportedConfigRange,
};

pub const BUFFER_SIZE_CHOICES: [u32; 7] = [64, 128, 256, 512, 1024, 2048, 4096];
const COMMON_SAMPLE_RATES: [u32; 6] = [44_100, 48_000, 88_200, 96_000, 176_400, 192_000];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioDeviceChoice {
    pub name: Option<String>,
    pub available: bool,
}

impl AudioDeviceChoice {
    pub fn system_default(available: bool) -> Self {
        Self {
            name: None,
            available,
        }
    }

    pub fn named(name: String, available: bool) -> Self {
        Self {
            name: Some(name),
            available,
        }
    }
}

impl fmt::Display for AudioDeviceChoice {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let label = self.name.as_deref().unwrap_or("System Default");
        if self.available {
            formatter.write_str(label)
        } else {
            write!(formatter, "{label} (missing)")
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioSampleRate(pub u32);

impl fmt::Display for AudioSampleRate {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.0.is_multiple_of(1_000) {
            write!(formatter, "{} kHz", self.0 / 1_000)
        } else {
            write!(formatter, "{:.1} kHz", self.0 as f32 / 1_000.0)
        }
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioDevicePreferences {
    #[serde(default)]
    pub input_name: Option<String>,
    #[serde(default)]
    pub output_name: Option<String>,
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioBackendPreferences {
    #[serde(default)]
    pub system: AudioDevicePreferences,
    #[serde(default)]
    pub asio: AudioDevicePreferences,
}

impl AudioBackendPreferences {
    pub fn for_backend(&self, backend: AudioBackend) -> &AudioDevicePreferences {
        match backend {
            AudioBackend::System => &self.system,
            AudioBackend::Asio => &self.asio,
        }
    }

    pub fn for_backend_mut(&mut self, backend: AudioBackend) -> &mut AudioDevicePreferences {
        match backend {
            AudioBackend::System => &mut self.system,
            AudioBackend::Asio => &mut self.asio,
        }
    }
}

#[derive(Debug, Clone)]
pub struct AudioSettingsState {
    /// Persisted backend whose devices are displayed in Settings.
    pub backend: AudioBackend,
    /// Backend currently driving the engine callback. This may remain the
    /// previous backend when a requested driver is temporarily unavailable.
    pub active_backend: Option<AudioBackend>,
    pub catalog: AudioDeviceCatalog,
    /// Remembered targets for every backend, including inactive backends.
    pub backend_preferences: AudioBackendPreferences,
    /// Persisted target. It remains named when that device is temporarily absent.
    pub preferred_input_name: Option<String>,
    /// Persisted target. It remains named when fallback output is active.
    pub preferred_output_name: Option<String>,
    /// Concrete device currently driving the engine callback.
    pub active_output_name: Option<String>,
    pub sample_rate: u32,
    pub buffer_size: u32,
    pub catalog_error: Option<String>,
}

impl Default for AudioSettingsState {
    fn default() -> Self {
        Self {
            backend: AudioBackend::System,
            active_backend: None,
            catalog: AudioDeviceCatalog::default(),
            backend_preferences: AudioBackendPreferences::default(),
            preferred_input_name: None,
            preferred_output_name: None,
            active_output_name: None,
            sample_rate: 44_100,
            buffer_size: 512,
            catalog_error: None,
        }
    }
}

impl AudioSettingsState {
    pub fn remember_current_backend(&mut self) {
        let preferences = self.backend_preferences.for_backend_mut(self.backend);
        preferences
            .input_name
            .clone_from(&self.preferred_input_name);
        preferences
            .output_name
            .clone_from(&self.preferred_output_name);
    }

    pub fn restore_backend_preferences(&mut self, backend: AudioBackend) {
        let preferences = self.backend_preferences.for_backend(backend);
        self.preferred_input_name
            .clone_from(&preferences.input_name);
        self.preferred_output_name
            .clone_from(&preferences.output_name);
    }

    pub fn output_choices(&self) -> Vec<AudioDeviceChoice> {
        device_choices(
            self.usable_output_devices(),
            self.default_output_available(),
            self.preferred_output_name.as_deref(),
        )
    }

    pub fn input_choices(&self) -> Vec<AudioDeviceChoice> {
        device_choices(
            self.catalog.input_devices.iter(),
            self.catalog.default_input_name.is_some(),
            self.preferred_input_name.as_deref(),
        )
    }

    pub fn selected_output_choice(&self) -> AudioDeviceChoice {
        selected_choice(
            self.usable_output_devices(),
            self.default_output_available(),
            self.preferred_output_name.as_deref(),
        )
    }

    pub fn selected_input_choice(&self) -> AudioDeviceChoice {
        selected_choice(
            self.catalog.input_devices.iter(),
            self.catalog.default_input_name.is_some(),
            self.preferred_input_name.as_deref(),
        )
    }

    pub fn target_output(&self) -> Option<&DeviceInfo> {
        resolve_device(
            &self.catalog.output_devices,
            self.catalog.default_output_name.as_deref(),
            self.preferred_output_name.as_deref(),
        )
    }

    pub fn output_for_choice(&self, choice: &AudioDeviceChoice) -> Option<&DeviceInfo> {
        let name = choice
            .name
            .as_deref()
            .or(self.catalog.default_output_name.as_deref())?;
        self.catalog
            .output_devices
            .iter()
            .find(|device| device.name == name)
    }

    /// Keep the current rate/buffer when the new device supports them, then
    /// prefer the device/default DAW values before choosing its first option.
    pub fn compatible_output_configuration(
        &self,
        choice: &AudioDeviceChoice,
    ) -> Option<(u32, u32)> {
        let device = self.output_for_choice(choice)?;
        let rates = supported_sample_rates(device);
        let sample_rate = rates
            .contains(&AudioSampleRate(self.sample_rate))
            .then_some(self.sample_rate)
            .or_else(|| {
                device
                    .default_config
                    .as_ref()
                    .map(|config| config.sample_rate)
                    .filter(|rate| rates.contains(&AudioSampleRate(*rate)))
            })
            .or_else(|| rates.first().map(|rate| rate.0))?;
        self.compatible_buffer_size(device, sample_rate)
            .map(|buffer_size| (sample_rate, buffer_size))
    }

    pub fn compatible_target_buffer_size(&self, sample_rate: u32) -> Option<u32> {
        self.target_output()
            .and_then(|device| self.compatible_buffer_size(device, sample_rate))
    }

    pub fn selected_input(&self) -> Option<&DeviceInfo> {
        resolve_device(
            &self.catalog.input_devices,
            self.catalog.default_input_name.as_deref(),
            self.preferred_input_name.as_deref(),
        )
    }

    /// Concrete device name represented by the persisted picker choice.
    /// For System Default this follows the latest hardware catalog snapshot.
    pub fn selected_input_name(&self) -> Option<&str> {
        self.selected_input().map(|device| device.name.as_str())
    }

    pub fn input_channel_count(&self) -> u16 {
        self.selected_input()
            .and_then(|device| {
                device
                    .default_config
                    .as_ref()
                    .map(|config| config.channels)
                    .or_else(|| {
                        device
                            .supported_configs
                            .iter()
                            .map(|config| config.channels)
                            .max()
                    })
            })
            .unwrap_or(0)
    }

    pub fn sample_rate_choices(&self) -> Vec<AudioSampleRate> {
        self.target_output()
            .map(supported_sample_rates)
            .unwrap_or_else(|| vec![AudioSampleRate(self.sample_rate)])
    }

    pub fn buffer_size_choices(&self) -> Vec<u32> {
        self.target_output()
            .map(|device| supported_buffer_sizes(device, self.sample_rate))
            .unwrap_or_else(|| vec![self.buffer_size])
    }

    pub fn input_description(&self) -> String {
        if self.selected_input().is_none() {
            return self
                .preferred_input_name
                .as_ref()
                .map(|name| format!("{name} is unavailable"))
                .unwrap_or_else(|| "No Audio Input available".into());
        }
        let channels = self.input_channel_count();
        match channels {
            0 => "Input capabilities unavailable".into(),
            1 => "1 available input channel".into(),
            count => format!("{count} available input channels"),
        }
    }

    pub fn preferred_output_is_missing(&self) -> bool {
        self.preferred_output_name.as_ref().is_some_and(|name| {
            !self
                .catalog
                .output_devices
                .iter()
                .any(|device| device.name == *name)
        })
    }

    fn compatible_buffer_size(&self, device: &DeviceInfo, sample_rate: u32) -> Option<u32> {
        let sizes = supported_buffer_sizes(device, sample_rate);
        sizes
            .contains(&self.buffer_size)
            .then_some(self.buffer_size)
            .or_else(|| sizes.iter().copied().find(|size| *size == 512))
            .or_else(|| sizes.first().copied())
    }

    fn usable_output_devices(&self) -> impl Iterator<Item = &DeviceInfo> + Clone {
        self.catalog
            .output_devices
            .iter()
            .filter(|device| !device.supported_configs.is_empty())
    }

    fn default_output_available(&self) -> bool {
        self.catalog
            .default_output_name
            .as_deref()
            .is_some_and(|name| {
                self.usable_output_devices()
                    .any(|device| device.name == name)
            })
    }
}

fn device_choices<'a>(
    devices: impl Iterator<Item = &'a DeviceInfo> + Clone,
    default_available: bool,
    preferred_name: Option<&str>,
) -> Vec<AudioDeviceChoice> {
    let mut choices = vec![AudioDeviceChoice::system_default(default_available)];
    choices.extend(
        devices
            .clone()
            .map(|device| AudioDeviceChoice::named(device.name.clone(), true)),
    );
    if let Some(name) = preferred_name {
        if !devices.clone().any(|device| device.name == name) {
            choices.push(AudioDeviceChoice::named(name.to_string(), false));
        }
    }
    choices
}

fn selected_choice<'a>(
    mut devices: impl Iterator<Item = &'a DeviceInfo>,
    default_available: bool,
    preferred_name: Option<&str>,
) -> AudioDeviceChoice {
    match preferred_name {
        Some(name) => {
            AudioDeviceChoice::named(name.to_string(), devices.any(|device| device.name == name))
        }
        None => AudioDeviceChoice::system_default(default_available),
    }
}

fn resolve_device<'a>(
    devices: &'a [DeviceInfo],
    default_name: Option<&str>,
    preferred_name: Option<&str>,
) -> Option<&'a DeviceInfo> {
    let target = preferred_name.or(default_name)?;
    devices.iter().find(|device| device.name == target)
}

fn stream_ranges(device: &DeviceInfo) -> impl Iterator<Item = &SupportedConfigRange> {
    device.supported_configs.iter()
}

pub fn supported_sample_rates(device: &DeviceInfo) -> Vec<AudioSampleRate> {
    let mut rates: Vec<u32> = COMMON_SAMPLE_RATES
        .into_iter()
        .filter(|rate| {
            stream_ranges(device)
                .any(|config| (config.min_sample_rate..=config.max_sample_rate).contains(rate))
        })
        .collect();
    if let Some(default_rate) = device
        .default_config
        .as_ref()
        .map(|config| config.sample_rate)
    {
        rates.push(default_rate);
    }
    rates.extend(
        stream_ranges(device)
            .filter(|config| config.min_sample_rate == config.max_sample_rate)
            .map(|config| config.min_sample_rate),
    );
    rates.sort_unstable();
    rates.dedup();
    rates.into_iter().map(AudioSampleRate).collect()
}

pub fn supported_buffer_sizes(device: &DeviceInfo, sample_rate: u32) -> Vec<u32> {
    let matching: Vec<_> = stream_ranges(device)
        .filter(|config| (config.min_sample_rate..=config.max_sample_rate).contains(&sample_rate))
        .collect();
    let mut sizes: Vec<u32> = BUFFER_SIZE_CHOICES
        .into_iter()
        .filter(|size| {
            matching.iter().any(|config| {
                config
                    .buffer_size_range
                    .is_none_or(|(min, max)| (min..=max).contains(size))
            })
        })
        .collect();
    if sizes.is_empty() {
        // Some pro interfaces expose one unusual fixed size. Keep the
        // settings usable instead of requiring it to match our common list.
        if let Some(minimum) = matching
            .iter()
            .filter_map(|config| config.buffer_size_range.map(|(min, _)| min))
            .min()
        {
            sizes.push(minimum);
        }
    }
    sizes
}

#[cfg(test)]
mod tests {
    use super::*;
    use vibez_audio_io::audio_host::StreamConfigInfo;

    fn device(name: &str, min_rate: u32, max_rate: u32) -> DeviceInfo {
        DeviceInfo {
            name: name.into(),
            default_config: Some(StreamConfigInfo {
                sample_rate: 48_000,
                channels: 2,
                sample_format: "F32".into(),
            }),
            supported_configs: vec![SupportedConfigRange {
                channels: 2,
                min_sample_rate: min_rate,
                max_sample_rate: max_rate,
                sample_format: "F32".into(),
                buffer_size_range: Some((128, 1024)),
            }],
        }
    }

    #[test]
    fn missing_preference_stays_visible_for_reconnect() {
        let state = AudioSettingsState {
            catalog: AudioDeviceCatalog {
                default_output_name: Some("Built-in".into()),
                output_devices: vec![device("Built-in", 44_100, 96_000)],
                ..Default::default()
            },
            preferred_output_name: Some("USB Interface".into()),
            ..Default::default()
        };

        assert!(state.preferred_output_is_missing());
        assert_eq!(
            state.selected_output_choice().to_string(),
            "USB Interface (missing)"
        );
        assert!(state
            .output_choices()
            .iter()
            .any(|choice| choice.to_string() == "USB Interface (missing)"));
    }

    #[test]
    fn rates_and_buffers_come_from_selected_output_capabilities() {
        let output = device("Interface", 44_100, 96_000);
        assert_eq!(
            supported_sample_rates(&output),
            vec![
                AudioSampleRate(44_100),
                AudioSampleRate(48_000),
                AudioSampleRate(88_200),
                AudioSampleRate(96_000)
            ]
        );
        assert_eq!(
            supported_buffer_sizes(&output, 48_000),
            vec![128, 256, 512, 1024]
        );
    }

    #[test]
    fn integer_output_streams_are_offered_and_configured() {
        let mut integer_output = device("Integer output", 44_100, 48_000);
        integer_output.supported_configs[0].sample_format = "I16".into();
        integer_output
            .default_config
            .as_mut()
            .unwrap()
            .sample_format = "I16".into();
        let state = AudioSettingsState {
            catalog: AudioDeviceCatalog {
                output_devices: vec![integer_output],
                ..Default::default()
            },
            ..Default::default()
        };

        assert!(state
            .output_choices()
            .iter()
            .any(|choice| choice.name.as_deref() == Some("Integer output")));
        assert_eq!(
            state.compatible_output_configuration(&AudioDeviceChoice::named(
                "Integer output".into(),
                true
            )),
            Some((44_100, 512))
        );
    }

    #[test]
    fn backend_device_preferences_survive_round_trip_switching() {
        let mut state = AudioSettingsState {
            backend: AudioBackend::System,
            preferred_input_name: Some("UR12 Input".into()),
            preferred_output_name: Some("UR12 Speakers".into()),
            ..Default::default()
        };
        state.remember_current_backend();

        state.backend = AudioBackend::Asio;
        state.restore_backend_preferences(AudioBackend::Asio);
        assert_eq!(state.preferred_input_name, None);
        assert_eq!(state.preferred_output_name, None);

        state.preferred_input_name = Some("ASIO4ALL v2".into());
        state.preferred_output_name = Some("ASIO4ALL v2".into());
        state.remember_current_backend();

        state.backend = AudioBackend::System;
        state.restore_backend_preferences(AudioBackend::System);
        assert_eq!(state.preferred_input_name.as_deref(), Some("UR12 Input"));
        assert_eq!(
            state.preferred_output_name.as_deref(),
            Some("UR12 Speakers")
        );
        assert_eq!(
            state
                .backend_preferences
                .for_backend(AudioBackend::Asio)
                .output_name
                .as_deref(),
            Some("ASIO4ALL v2")
        );
    }
}
