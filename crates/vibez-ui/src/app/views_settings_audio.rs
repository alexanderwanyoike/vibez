//! Audio and MIDI hardware settings surface.

use iced::widget::{button, column, row, text, Space};
use iced::{Element, Length, Theme};

use crate::icons;
use crate::message::Message;
use crate::theme as th;
use crate::widgets::control_style;

use super::views_settings_rows::{action, group, hint, label_block, pick, setting_row, status};
use super::*;

const DEVICE_PICK_W: f32 = 260.0;
const SHORT_PICK_W: f32 = 140.0;

/// A status line with a dot coloured by how the hardware is doing.
fn health<'a>(dot: iced::Color, lit: bool, line: String) -> Element<'a, Message> {
    row![
        icons::icon(if lit {
            icons::CIRCLE_DOT
        } else {
            icons::CIRCLE
        })
        .size(9)
        .color(dot),
        hint(line),
    ]
    .spacing(5)
    .align_y(iced::Alignment::Center)
    .into()
}

impl App {
    pub(super) fn view_settings_audio_tab(&self) -> Element<'_, Message> {
        let settings = &self.state.audio_settings;
        let output_status = match &self.state.audio_stream_health {
            AudioStreamHealth::Running => {
                let active = settings
                    .active_output_name
                    .as_deref()
                    .unwrap_or("System Default");
                let backend = settings.active_backend.unwrap_or(settings.backend);
                health(
                    th::success(),
                    true,
                    format!("Running · {backend} · {active}"),
                )
            }
            AudioStreamHealth::Rebuilding => {
                health(th::accent(), false, "Applying configuration…".into())
            }
            AudioStreamHealth::Error(cause) => {
                health(th::danger(), false, format!("Output error · {cause}"))
            }
        };
        let has_input = settings.selected_input().is_some();
        let input_status = health(
            if has_input {
                th::success()
            } else {
                th::text_muted()
            },
            has_input,
            settings.input_description(),
        );
        let output_picker = pick(
            settings.output_choices(),
            Some(settings.selected_output_choice()),
            Message::SelectAudioOutput,
            DEVICE_PICK_W,
        );

        let mut hardware: Vec<Element<'_, Message>> = Vec::new();
        if vibez_audio_io::audio_host::AudioBackend::available().len() > 1 {
            hardware.push(setting_row(
                "Driver type",
                Some(hint("The audio system used for input and output.").into()),
                pick(
                    vibez_audio_io::audio_host::AudioBackend::available(),
                    Some(settings.backend),
                    Message::SelectAudioBackend,
                    SHORT_PICK_W,
                ),
            ));
        }
        if settings.backend == vibez_audio_io::audio_host::AudioBackend::Asio {
            hardware.push(setting_row(
                "Audio device",
                Some(column![output_status, input_status].spacing(3).into()),
                output_picker,
            ));
            hardware.push(label_block(
                "ASIO routing",
                Some(
                    hint(
                        "Choose speakers and headphones in the driver's control panel. \
                         ASIO4ALL exposes it from the Windows tray while running.",
                    )
                    .into(),
                ),
            ));
        } else {
            hardware.push(setting_row(
                "Output device",
                Some(output_status),
                output_picker,
            ));
            hardware.push(setting_row(
                "Input device",
                Some(input_status),
                pick(
                    settings.input_choices(),
                    Some(settings.selected_input_choice()),
                    Message::SelectAudioInput,
                    DEVICE_PICK_W,
                ),
            ));
        }
        hardware.push(setting_row(
            "Devices",
            Some(
                hint("Rescan after plugging hardware in; reconnect to restart the output.").into(),
            ),
            row![
                action("Rescan", Some(Message::RescanAudioDevices), false),
                action("Reconnect", Some(Message::ReconnectAudioOutput), true),
            ]
            .spacing(6),
        ));

        let mut buffers = row![].spacing(2);
        for size in settings.buffer_size_choices() {
            let chosen = settings.buffer_size == size;
            buffers = buffers.push(
                button(text(size.to_string()).size(11))
                    .on_press(Message::SetBufferSize(size))
                    .padding([4, 8])
                    .style(move |_theme: &Theme, status| button::Style {
                        border: iced::Border {
                            radius: 6.0.into(),
                            ..Default::default()
                        },
                        ..control_style::styled(chosen.then(th::accent), status)
                    }),
            );
        }
        let buffer_latency_ms = settings.buffer_size as f64 / settings.sample_rate as f64 * 1_000.0;
        let engine = group([
            setting_row(
                "Sample rate",
                None,
                pick(
                    settings.sample_rate_choices(),
                    Some(crate::domains::audio_settings::AudioSampleRate(
                        settings.sample_rate,
                    )),
                    Message::SetAudioSampleRate,
                    SHORT_PICK_W,
                ),
            ),
            setting_row(
                "Buffer size",
                Some(
                    hint(format!(
                        "{buffer_latency_ms:.1} ms. Lower feels faster; higher leaves more CPU headroom."
                    ))
                    .into(),
                ),
                buffers,
            ),
        ]);

        let connected = self
            .midi_input
            .as_ref()
            .map(|input| input.port_name.as_str());
        let mut midi: Vec<Element<'_, Message>> = vec![setting_row(
            "MIDI input",
            Some(
                column![
                    hint(
                        "External MIDI plays the selected instrument track. Plug your \
                         keyboard or Push in, rescan, then pick its port."
                    ),
                    hint(match connected {
                        Some(port) => format!("Connected: {port}"),
                        None => "Not connected".into(),
                    }),
                ]
                .spacing(2)
                .into(),
            ),
            row![
                action("Rescan ports", Some(Message::RescanMidiInputs), false),
                action(
                    "Disconnect",
                    connected.is_some().then_some(Message::CloseMidiInput),
                    false
                ),
            ]
            .spacing(6),
        )];
        for name in &self.midi_input_ports {
            let current = connected == Some(name.as_str());
            midi.push(
                button(
                    row![
                        icons::icon(if current {
                            icons::CIRCLE_DOT
                        } else {
                            icons::CIRCLE
                        })
                        .size(11)
                        .color(if current {
                            th::accent()
                        } else {
                            th::text_muted()
                        }),
                        text(name.clone()).size(11.5).color(th::text()),
                        Space::with_width(Length::Fill),
                    ]
                    .spacing(8)
                    .align_y(iced::Alignment::Center),
                )
                .on_press(Message::OpenMidiInput(name.clone()))
                .padding([5, 8])
                .width(Length::Fill)
                .style(move |_theme: &Theme, status| {
                    super::views_browser_style::browser_place_button_style(current, status)
                })
                .into(),
            );
        }

        let mut body = column![].spacing(16);
        if let Some(error) = settings.catalog_error.as_ref() {
            body = body.push(status(format!("Device scan failed · {error}"), true));
        }
        body.push(group(hardware))
            .push(engine)
            .push(group(midi))
            .into()
    }
}
