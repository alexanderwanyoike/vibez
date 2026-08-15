//! Settings → Audio hardware actions.

use iced::Task;
use vibez_audio_io::audio_host::AudioHost;
use vibez_audio_io::audio_stream::{AudioOutputStream, OutputStreamConfig};
use vibez_engine::commands::EngineCommand;
use vibez_engine::engine::AudioEngine;

use crate::domains::audio_settings::{AudioDeviceChoice, AudioSampleRate};
use crate::message::Message;
use crate::state::AudioStreamHealth;

use super::App;

pub(super) struct InitialAudioOutput {
    pub stream: AudioOutputStream,
    pub sample_rate: u32,
    pub active_output_name: Option<String>,
    pub health: AudioStreamHealth,
    pub status: Option<String>,
}

pub(super) fn initialize_audio_output(
    engine: AudioEngine,
    preferred_output_name: Option<String>,
    sample_rate: Option<u32>,
    buffer_size: u32,
) -> InitialAudioOutput {
    let mut stream = AudioOutputStream::idle(engine);
    let requests = initial_output_requests(preferred_output_name.clone(), sample_rate, buffer_size);
    let mut errors = Vec::new();
    let mut successful_attempt = None;
    for (attempt, request) in requests.iter().cloned().enumerate() {
        match stream.reconfigure(request) {
            Ok(()) => {
                successful_attempt = Some(attempt);
                break;
            }
            Err(error) => {
                eprintln!("vibez: audio startup attempt rejected: {error}");
                errors.push(error.to_string());
            }
        }
    }
    let (health, status) = match successful_attempt {
        Some(0) => (AudioStreamHealth::Running, None),
        Some(attempt) => {
            let fallback = if requests[attempt].sample_rate.is_none()
                && requests[attempt].buffer_size.is_none()
            {
                "System Default device settings"
            } else {
                "System Default"
            };
            (
                AudioStreamHealth::Running,
                Some(format!(
                    "Saved audio configuration unavailable — using {fallback}: {}",
                    errors
                        .first()
                        .map(String::as_str)
                        .unwrap_or("unknown error")
                )),
            )
        }
        None => failed_initial_audio(
            errors
                .last()
                .map(String::as_str)
                .unwrap_or("no audio output configuration was attempted"),
        ),
    };
    // Startup owns the status above; lifecycle events are for later changes.
    while stream.try_next_event().is_some() {}
    InitialAudioOutput {
        sample_rate: stream.sample_rate().unwrap_or(44_100),
        active_output_name: stream.active_device_name().map(str::to_owned),
        stream,
        health,
        status,
    }
}

fn initial_output_requests(
    preferred_output_name: Option<String>,
    sample_rate: Option<u32>,
    buffer_size: u32,
) -> Vec<OutputStreamConfig> {
    let saved_request = OutputStreamConfig {
        device_name: preferred_output_name.clone(),
        sample_rate,
        buffer_size: Some(buffer_size),
    };
    let mut requests = vec![saved_request];
    if preferred_output_name.is_some() {
        requests.push(OutputStreamConfig {
            device_name: None,
            sample_rate,
            buffer_size: Some(buffer_size),
        });
    }
    let device_defaults = OutputStreamConfig {
        device_name: None,
        sample_rate: None,
        buffer_size: None,
    };
    if requests.last() != Some(&device_defaults) {
        requests.push(device_defaults);
    }
    requests
}

fn failed_initial_audio(error: impl std::fmt::Display) -> (AudioStreamHealth, Option<String>) {
    eprintln!("vibez: failed to open audio output: {error}");
    let cause = error.to_string();
    (
        AudioStreamHealth::Error(cause.clone()),
        Some(format!("Audio stream error: {cause}")),
    )
}

impl App {
    pub(super) fn handle_set_buffer_size(&mut self, size: u32) -> Task<Message> {
        if !self
            .state
            .audio_settings
            .buffer_size_choices()
            .contains(&size)
        {
            self.state.status_text = format!("{size} frames is not supported by this output");
            return Task::none();
        }
        self.apply_audio_output_configuration(
            self.state.audio_settings.preferred_output_name.clone(),
            self.state.audio_settings.sample_rate,
            size,
        );
        Task::none()
    }

    pub(super) fn handle_set_audio_sample_rate(
        &mut self,
        sample_rate: AudioSampleRate,
    ) -> Task<Message> {
        if !self
            .state
            .audio_settings
            .sample_rate_choices()
            .contains(&sample_rate)
        {
            self.state.status_text = format!("{} is not supported by this output", sample_rate);
            return Task::none();
        }
        let buffer_size = self
            .state
            .audio_settings
            .compatible_target_buffer_size(sample_rate.0);
        let Some(buffer_size) = buffer_size else {
            self.state.status_text = format!("{} has no usable Buffer Size", sample_rate);
            return Task::none();
        };
        self.apply_audio_output_configuration(
            self.state.audio_settings.preferred_output_name.clone(),
            sample_rate.0,
            buffer_size,
        );
        Task::none()
    }

    pub(super) fn handle_select_audio_output(
        &mut self,
        choice: AudioDeviceChoice,
    ) -> Task<Message> {
        if !choice.available {
            self.state.status_text =
                format!("{} is unavailable — rescan after reconnecting it", choice);
            return Task::none();
        }
        let Some((sample_rate, buffer_size)) = self
            .state
            .audio_settings
            .compatible_output_configuration(&choice)
        else {
            self.state.status_text = format!("{choice} has no usable output configuration");
            return Task::none();
        };
        self.apply_audio_output_configuration(choice.name, sample_rate, buffer_size);
        Task::none()
    }

    pub(super) fn handle_select_audio_input(&mut self, choice: AudioDeviceChoice) -> Task<Message> {
        if self.state.audio_recording.is_busy() {
            self.state.status_text = "Stop Audio Track Recording before changing hardware".into();
            return Task::none();
        }
        if !choice.available {
            self.state.status_text =
                format!("{} is unavailable — rescan after reconnecting it", choice);
            return Task::none();
        }
        self.state.audio_settings.preferred_input_name = choice.name;
        self.state.status_text = format!(
            "Audio Input selected — {}. Recording starts only when a track is armed",
            self.state.audio_settings.input_description()
        );
        self.persist_ui_settings();
        if let Err(error) = self.sync_audio_input_runtime() {
            self.state.status_text = format!("Audio Input selected but could not start — {error}");
        }
        Task::none()
    }

    pub(super) fn handle_rescan_audio_devices(&mut self) -> Task<Message> {
        match AudioHost::new().catalog() {
            Ok(catalog) => {
                let inputs = catalog.input_devices.len();
                let outputs = catalog.output_devices.len();
                self.state.audio_settings.catalog = catalog;
                self.state.audio_settings.catalog_error = None;
                self.state.status_text =
                    format!("Audio devices rescanned — {inputs} inputs, {outputs} outputs");
            }
            Err(error) => {
                self.state.audio_settings.catalog_error = Some(error.to_string());
                self.state.status_text = format!("Audio device scan failed: {error}");
            }
        }
        Task::none()
    }

    pub(super) fn handle_reconnect_audio_output(&mut self) -> Task<Message> {
        if self.state.audio_settings.preferred_output_is_missing() {
            let name = self
                .state
                .audio_settings
                .preferred_output_name
                .as_deref()
                .unwrap_or("Audio Output");
            self.state.status_text = format!("{name} is still unavailable — connect it and Rescan");
            return Task::none();
        }
        self.handle_select_audio_output(self.state.audio_settings.selected_output_choice())
    }

    fn apply_audio_output_configuration(
        &mut self,
        preferred_output_name: Option<String>,
        sample_rate: u32,
        buffer_size: u32,
    ) {
        if self.state.audio_recording.is_busy() {
            self.state.status_text = "Stop Audio Track Recording before changing hardware".into();
            return;
        }
        let request = OutputStreamConfig {
            device_name: preferred_output_name.clone(),
            sample_rate: Some(sample_rate),
            buffer_size: Some(buffer_size),
        };
        let result = self
            ._stream
            .as_mut()
            .ok_or_else(|| "audio engine is unavailable".to_string())
            .and_then(|stream| {
                stream
                    .reconfigure(request)
                    .map_err(|error| error.to_string())?;
                Ok((
                    stream.sample_rate().unwrap_or(sample_rate),
                    stream.active_device_name().map(str::to_owned),
                ))
            });
        // Reconfiguration reports synchronously through the stream's lifecycle
        // queue. Consume those events now so the next UI tick cannot overwrite
        // the more useful producer-facing result below with generic recovery
        // copy.
        self.poll_audio_stream_events();
        match result {
            Ok((actual_sample_rate, active_output_name)) => {
                self.state.audio_settings.preferred_output_name = preferred_output_name;
                self.state.audio_settings.active_output_name = active_output_name.clone();
                self.state.audio_settings.sample_rate = actual_sample_rate;
                self.state.audio_settings.buffer_size = buffer_size;
                self.state.transport.sample_rate = actual_sample_rate;
                self.state.audio_stream_health = AudioStreamHealth::Running;
                self.send_command(EngineCommand::SetSampleRate(actual_sample_rate));
                if let Err(error) = self.sync_audio_input_runtime() {
                    self.state.status_text =
                        format!("Audio Output changed, but Audio Input could not reopen — {error}");
                    return;
                }
                self.persist_ui_settings();
                self.state.status_text = format!(
                    "Audio Output running — {}, {} Hz, {buffer_size} frames",
                    active_output_name.as_deref().unwrap_or("System Default"),
                    actual_sample_rate
                );
            }
            Err(error) => {
                eprintln!("vibez: audio configuration rejected: {error}");
                if self._stream.is_none() {
                    self.state.status_text = format!("Audio configuration rejected: {error}");
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::initial_output_requests;
    use vibez_audio_io::audio_stream::OutputStreamConfig;

    #[test]
    fn startup_fallbacks_progress_from_saved_device_to_device_defaults() {
        assert_eq!(
            initial_output_requests(Some("Dock".into()), Some(96_000), 64),
            vec![
                OutputStreamConfig {
                    device_name: Some("Dock".into()),
                    sample_rate: Some(96_000),
                    buffer_size: Some(64),
                },
                OutputStreamConfig {
                    device_name: None,
                    sample_rate: Some(96_000),
                    buffer_size: Some(64),
                },
                OutputStreamConfig {
                    device_name: None,
                    sample_rate: None,
                    buffer_size: None,
                },
            ]
        );
    }

    #[test]
    fn system_default_saved_configuration_still_falls_back_to_device_defaults() {
        assert_eq!(
            initial_output_requests(None, Some(192_000), 64),
            vec![
                OutputStreamConfig {
                    device_name: None,
                    sample_rate: Some(192_000),
                    buffer_size: Some(64),
                },
                OutputStreamConfig {
                    device_name: None,
                    sample_rate: None,
                    buffer_size: None,
                },
            ]
        );
    }
}
