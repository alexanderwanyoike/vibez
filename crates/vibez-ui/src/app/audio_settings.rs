//! Settings → Audio hardware actions.

use iced::Task;
use vibez_audio_io::audio_host::{AudioBackend, AudioHost};
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
    pub active_backend: Option<AudioBackend>,
    pub health: AudioStreamHealth,
    pub status: Option<String>,
}

pub(super) fn initialize_audio_output(
    engine: AudioEngine,
    preferred_backend: AudioBackend,
    preferred_output_name: Option<String>,
    sample_rate: Option<u32>,
    buffer_size: u32,
) -> InitialAudioOutput {
    let mut stream = AudioOutputStream::idle(engine);
    let requests = initial_output_requests(
        preferred_backend,
        preferred_output_name.clone(),
        sample_rate,
        buffer_size,
    );
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
            let fallback = if requests[attempt].backend != preferred_backend {
                format!("{} with device defaults", requests[attempt].backend)
            } else if requests[attempt].sample_rate.is_none()
                && requests[attempt].buffer_size.is_none()
            {
                format!("{preferred_backend} device defaults")
            } else {
                format!("{preferred_backend} default device")
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
        active_backend: stream.active_backend(),
        stream,
        health,
        status,
    }
}

fn initial_output_requests(
    backend: AudioBackend,
    preferred_output_name: Option<String>,
    sample_rate: Option<u32>,
    buffer_size: u32,
) -> Vec<OutputStreamConfig> {
    let saved_request = OutputStreamConfig {
        backend,
        device_name: preferred_output_name.clone(),
        sample_rate,
        buffer_size: Some(buffer_size),
    };
    let mut requests = vec![saved_request];
    if preferred_output_name.is_some() {
        requests.push(OutputStreamConfig {
            backend,
            device_name: None,
            sample_rate,
            buffer_size: Some(buffer_size),
        });
    }
    let device_defaults = OutputStreamConfig {
        backend,
        device_name: None,
        sample_rate: None,
        buffer_size: None,
    };
    if requests.last() != Some(&device_defaults) {
        requests.push(device_defaults);
    }
    if backend != AudioBackend::System {
        requests.push(OutputStreamConfig {
            backend: AudioBackend::System,
            device_name: None,
            sample_rate: None,
            buffer_size: None,
        });
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

fn live_output_requests(
    backend: AudioBackend,
    output_name: Option<String>,
    sample_rate: u32,
    buffer_size: u32,
) -> Vec<OutputStreamConfig> {
    let preferred = OutputStreamConfig {
        backend,
        device_name: output_name.clone(),
        sample_rate: Some(sample_rate),
        buffer_size: Some(buffer_size),
    };
    if backend == AudioBackend::Asio {
        vec![
            preferred,
            OutputStreamConfig {
                backend,
                device_name: output_name,
                sample_rate: None,
                buffer_size: None,
            },
        ]
    } else {
        vec![preferred]
    }
}

impl App {
    pub(super) fn handle_select_audio_backend(&mut self, backend: AudioBackend) -> Task<Message> {
        if self.state.audio_recording.is_busy() {
            self.state.status_text = "Stop Audio Track Recording before changing hardware".into();
            return Task::none();
        }
        if !backend.is_available() {
            self.state.status_text = format!("{backend} is unavailable on this platform");
            return Task::none();
        }
        if backend == self.state.audio_settings.backend {
            return self.handle_rescan_audio_devices();
        }
        let catalog = match AudioHost::new(backend).and_then(|host| host.catalog()) {
            Ok(catalog) => catalog,
            Err(error) => {
                self.state.status_text = format!("{backend} could not start — {error}");
                return Task::none();
            }
        };
        let previous = self.state.audio_settings.clone();
        self.state.audio_settings.backend = backend;
        self.state.audio_settings.catalog = catalog;
        self.state.audio_settings.catalog_error = None;
        self.state.audio_settings.preferred_input_name = None;
        self.state.audio_settings.preferred_output_name = None;

        let choice = self.state.audio_settings.selected_output_choice();
        let Some((sample_rate, buffer_size)) = self
            .state
            .audio_settings
            .compatible_output_configuration(&choice)
        else {
            self.state.audio_settings = previous.clone();
            self.state.status_text = format!(
                "No {backend} output driver found — {} remains active",
                previous
                    .active_backend
                    .map(|active| active.to_string())
                    .unwrap_or_else(|| "the current audio output".into())
            );
            return Task::none();
        };
        if !self.apply_audio_output_configuration(None, sample_rate, buffer_size) {
            self.state.audio_settings = previous;
        }
        Task::none()
    }

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
        if self.state.audio_settings.backend == AudioBackend::Asio {
            self.state.status_text =
                "ASIO uses one driver for input and output — choose Audio Device".into();
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
        let backend = self.state.audio_settings.backend;
        match AudioHost::new(backend).and_then(|host| host.catalog()) {
            Ok(catalog) => {
                let inputs = catalog.input_devices.len();
                let outputs = catalog.output_devices.len();
                self.state.audio_settings.catalog = catalog;
                self.state.audio_settings.catalog_error = None;
                self.state.status_text =
                    format!("{backend} devices rescanned — {inputs} inputs, {outputs} outputs");
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
    ) -> bool {
        if self.state.audio_recording.is_busy() {
            self.state.status_text = "Stop Audio Track Recording before changing hardware".into();
            return false;
        }
        let backend = self.state.audio_settings.backend;
        let active_backend = self.state.audio_settings.active_backend;
        if backend == AudioBackend::Asio || active_backend == Some(AudioBackend::Asio) {
            // ASIO input and output share one exclusive driver buffer set.
            // Release on-demand monitoring before the output stream rebuilds;
            // the successful path reopens it through the new output device.
            self._input_stream = None;
        }
        let requests = live_output_requests(
            backend,
            preferred_output_name.clone(),
            sample_rate,
            buffer_size,
        );
        let result = self
            ._stream
            .as_mut()
            .ok_or_else(|| "audio engine is unavailable".to_string())
            .and_then(|stream| {
                let applied_request = stream
                    .reconfigure_candidates(&requests)
                    .map_err(|error| error.to_string())?;
                Ok((
                    stream.sample_rate().unwrap_or(sample_rate),
                    stream.active_device_name().map(str::to_owned),
                    applied_request,
                ))
            });
        // Reconfiguration reports synchronously through the stream's lifecycle
        // queue. Consume those events now so the next UI tick cannot overwrite
        // the more useful producer-facing result below with generic recovery
        // copy.
        self.poll_audio_stream_events();
        match result {
            Ok((actual_sample_rate, active_output_name, applied_request)) => {
                self.state.audio_settings.preferred_output_name = preferred_output_name;
                self.state.audio_settings.preferred_input_name = if backend == AudioBackend::Asio {
                    self.state.audio_settings.preferred_output_name.clone()
                } else {
                    self.state.audio_settings.preferred_input_name.clone()
                };
                self.state.audio_settings.active_backend = Some(backend);
                self.state.audio_settings.active_output_name = active_output_name.clone();
                self.state.audio_settings.sample_rate = actual_sample_rate;
                self.state.audio_settings.buffer_size = buffer_size;
                self.state.transport.sample_rate = actual_sample_rate;
                self.state.audio_stream_health = AudioStreamHealth::Running;
                self.send_command(EngineCommand::SetSampleRate(actual_sample_rate));
                if let Err(error) = self.sync_audio_input_runtime() {
                    self.state.status_text =
                        format!("Audio Output changed, but Audio Input could not reopen — {error}");
                    return true;
                }
                self.persist_ui_settings();
                self.state.status_text = if applied_request == 0 {
                    format!(
                        "{backend} Audio Output running — {}, {} Hz, {buffer_size} frames",
                        active_output_name.as_deref().unwrap_or("System Default"),
                        actual_sample_rate
                    )
                } else {
                    format!(
                        "{backend} Audio Output running — {} with driver defaults",
                        active_output_name.as_deref().unwrap_or("System Default")
                    )
                };
                true
            }
            Err(error) => {
                eprintln!("vibez: audio configuration rejected: {error}");
                if let Some(stream) = self._stream.as_ref() {
                    self.state.audio_settings.active_backend = stream.active_backend();
                    self.state.audio_settings.active_output_name =
                        stream.active_device_name().map(str::to_owned);
                    if stream.is_running() {
                        let active = stream
                            .active_device_name()
                            .unwrap_or("the previous Audio Output");
                        self.state.audio_stream_health = AudioStreamHealth::Running;
                        self.state.status_text =
                            format!("Could not switch Audio Output — {error}. {active} restored");
                    } else {
                        self.state.audio_stream_health = AudioStreamHealth::Error(error.clone());
                        self.state.status_text =
                            format!("Audio Output failed and could not be restored — {error}");
                    }
                }
                if self._stream.is_none() {
                    self.state.status_text = format!("Audio configuration rejected: {error}");
                }
                let _ = self.sync_audio_input_runtime();
                false
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{initial_output_requests, live_output_requests};
    use vibez_audio_io::audio_host::AudioBackend;
    use vibez_audio_io::audio_stream::OutputStreamConfig;

    #[test]
    fn startup_fallbacks_progress_from_saved_device_to_device_defaults() {
        assert_eq!(
            initial_output_requests(AudioBackend::System, Some("Dock".into()), Some(96_000), 64,),
            vec![
                OutputStreamConfig {
                    backend: AudioBackend::System,
                    device_name: Some("Dock".into()),
                    sample_rate: Some(96_000),
                    buffer_size: Some(64),
                },
                OutputStreamConfig {
                    backend: AudioBackend::System,
                    device_name: None,
                    sample_rate: Some(96_000),
                    buffer_size: Some(64),
                },
                OutputStreamConfig {
                    backend: AudioBackend::System,
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
            initial_output_requests(AudioBackend::System, None, Some(192_000), 64),
            vec![
                OutputStreamConfig {
                    backend: AudioBackend::System,
                    device_name: None,
                    sample_rate: Some(192_000),
                    buffer_size: Some(64),
                },
                OutputStreamConfig {
                    backend: AudioBackend::System,
                    device_name: None,
                    sample_rate: None,
                    buffer_size: None,
                },
            ]
        );
    }

    #[test]
    fn asio_startup_falls_back_to_native_audio_when_the_driver_is_missing() {
        let requests = initial_output_requests(
            AudioBackend::Asio,
            Some("Yamaha Steinberg USB ASIO".into()),
            Some(48_000),
            128,
        );

        assert_eq!(requests.last().unwrap().backend, AudioBackend::System);
        assert_eq!(requests.last().unwrap().device_name, None);
        assert_eq!(requests.last().unwrap().sample_rate, None);
        assert_eq!(requests.last().unwrap().buffer_size, None);
    }

    #[test]
    fn asio_live_switch_retries_the_same_driver_with_its_defaults() {
        assert_eq!(
            live_output_requests(AudioBackend::Asio, Some("Realtek ASIO".into()), 48_000, 128,),
            vec![
                OutputStreamConfig {
                    backend: AudioBackend::Asio,
                    device_name: Some("Realtek ASIO".into()),
                    sample_rate: Some(48_000),
                    buffer_size: Some(128),
                },
                OutputStreamConfig {
                    backend: AudioBackend::Asio,
                    device_name: Some("Realtek ASIO".into()),
                    sample_rate: None,
                    buffer_size: None,
                },
            ]
        );
    }
}
