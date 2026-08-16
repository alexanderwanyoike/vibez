//! Audio and MIDI hardware settings surface.

use std::borrow::Borrow;

use iced::widget::{button, column, container, horizontal_space, pick_list, row, scrollable, text};
use iced::{Element, Length, Theme};

use crate::icons;
use crate::message::Message;
use crate::theme as th;

use super::*;

fn settings_pick_list<'a, T>(
    options: impl Borrow<[T]> + 'a,
    selected: Option<T>,
    on_selected: impl Fn(T) -> Message + 'a,
    width: f32,
) -> Element<'a, Message>
where
    T: ToString + PartialEq + Clone + 'a,
{
    pick_list(options, selected, on_selected)
        .placeholder("No device")
        .width(Length::Fixed(width))
        .padding([4, 8])
        .text_size(11)
        .style(|_theme: &Theme, status| {
            let engaged = matches!(
                status,
                pick_list::Status::Hovered | pick_list::Status::Opened
            );
            pick_list::Style {
                text_color: th::text(),
                placeholder_color: th::text_dim(),
                handle_color: if engaged {
                    th::accent()
                } else {
                    th::text_dim()
                },
                background: th::bg_dark().into(),
                border: iced::Border {
                    color: if engaged {
                        th::accent_dim()
                    } else {
                        th::border()
                    },
                    width: 1.0,
                    radius: 3.0.into(),
                },
            }
        })
        .menu_style(|_theme: &Theme| iced::widget::overlay::menu::Style {
            background: th::bg_elevated().into(),
            border: iced::Border {
                color: th::border_light(),
                width: 1.0,
                radius: 3.0.into(),
            },
            text_color: th::text(),
            selected_text_color: th::accent(),
            selected_background: th::bg_hover().into(),
        })
        .into()
}

fn settings_action_button(
    label: &'static str,
    message: Message,
    highlighted: bool,
) -> Element<'static, Message> {
    button(text(label).size(11).color(if highlighted {
        th::accent()
    } else {
        th::text_dim()
    }))
    .on_press(message)
    .padding([4, 10])
    .style(move |_theme: &Theme, status| {
        let engaged = matches!(status, button::Status::Hovered | button::Status::Pressed);
        button::Style {
            background: engaged.then(|| th::bg_hover().into()),
            text_color: if highlighted {
                th::accent()
            } else {
                th::text_dim()
            },
            border: iced::Border {
                color: if highlighted {
                    th::accent_dim()
                } else {
                    th::border()
                },
                width: 1.0,
                radius: 3.0.into(),
            },
            ..Default::default()
        }
    })
    .into()
}

fn settings_divider<'a>() -> Element<'a, Message> {
    container(column![].height(Length::Fixed(1.0)).width(Length::Fill))
        .style(|_theme: &Theme| container::Style {
            background: Some(th::border().into()),
            ..Default::default()
        })
        .into()
}

impl App {
    pub(super) fn view_settings_audio_tab(&self) -> Element<'_, Message> {
        let backend_picker = settings_pick_list(
            vibez_audio_io::audio_host::AudioBackend::available(),
            Some(self.state.audio_settings.backend),
            Message::SelectAudioBackend,
            132.0,
        );
        let output_picker = settings_pick_list(
            self.state.audio_settings.output_choices(),
            Some(self.state.audio_settings.selected_output_choice()),
            Message::SelectAudioOutput,
            260.0,
        );
        let input_picker = settings_pick_list(
            self.state.audio_settings.input_choices(),
            Some(self.state.audio_settings.selected_input_choice()),
            Message::SelectAudioInput,
            260.0,
        );

        let output_status: Element<'_, Message> = match &self.state.audio_stream_health {
            AudioStreamHealth::Running => {
                let active = self
                    .state
                    .audio_settings
                    .active_output_name
                    .as_deref()
                    .unwrap_or("System Default");
                let backend = self
                    .state
                    .audio_settings
                    .active_backend
                    .unwrap_or(self.state.audio_settings.backend);
                row![
                    icons::icon(icons::CIRCLE_DOT).size(9).color(th::success()),
                    text(format!("Running · {backend} · {active}"))
                        .size(10)
                        .color(th::text_dim())
                ]
                .spacing(5)
                .align_y(iced::Alignment::Center)
                .into()
            }
            AudioStreamHealth::Rebuilding => row![
                icons::icon(icons::CIRCLE).size(9).color(th::accent()),
                text("Applying configuration…")
                    .size(10)
                    .color(th::text_dim())
            ]
            .spacing(5)
            .align_y(iced::Alignment::Center)
            .into(),
            AudioStreamHealth::Error(cause) => row![
                icons::icon(icons::CIRCLE).size(9).color(th::danger()),
                text(format!("Output error · {cause}"))
                    .size(10)
                    .color(th::danger())
            ]
            .spacing(5)
            .align_y(iced::Alignment::Center)
            .into(),
        };
        let has_input = self.state.audio_settings.selected_input().is_some();
        let input_status: Element<'_, Message> = row![
            icons::icon(if has_input {
                icons::CIRCLE_DOT
            } else {
                icons::CIRCLE
            })
            .size(9)
            .color(if has_input {
                th::success()
            } else {
                th::text_muted()
            }),
            text(self.state.audio_settings.input_description())
                .size(10)
                .color(th::text_dim())
        ]
        .spacing(5)
        .align_y(iced::Alignment::Center)
        .into();

        let rescan_audio = settings_action_button("Rescan", Message::RescanAudioDevices, false);
        let reconnect_audio =
            settings_action_button("Reconnect", Message::ReconnectAudioOutput, true);

        let hardware_header = row![
            column![
                text("Audio Hardware").size(14).color(th::text()),
                text("Application-wide — never saved with projects")
                    .size(10)
                    .color(th::text_dim())
            ]
            .spacing(2)
            .width(Length::Fill),
            row![rescan_audio, reconnect_audio]
                .spacing(6)
                .align_y(iced::Alignment::Center)
        ]
        .align_y(iced::Alignment::Center);

        let backend_row = row![
            column![
                text("Driver Type").size(12).color(th::text()),
                text("The audio system used for input and output")
                    .size(9)
                    .color(th::text_muted())
            ]
            .spacing(1)
            .width(Length::Fill),
            backend_picker
        ]
        .spacing(18)
        .align_y(iced::Alignment::Center);
        let backend_section = if vibez_audio_io::audio_host::AudioBackend::available().len() > 1 {
            column![backend_row, settings_divider()]
        } else {
            column![]
        };

        let device_rows = if self.state.audio_settings.backend
            == vibez_audio_io::audio_host::AudioBackend::Asio
        {
            column![
                backend_section,
                row![
                    column![
                        text("Audio Device").size(12).color(th::text()),
                        output_status,
                        input_status
                    ]
                    .spacing(3)
                    .width(Length::Fill),
                    output_picker
                ]
                .spacing(18)
                .align_y(iced::Alignment::Center),
                settings_divider(),
                column![
                    text("ASIO Routing").size(12).color(th::text()),
                    text("Choose speakers and headphones in the driver's control panel. ASIO4ALL exposes it from the Windows tray while running")
                        .size(9)
                        .color(th::text_muted())
                ]
                .spacing(1)
                .width(Length::Fill)
            ]
        } else {
            column![
                backend_section,
                row![
                    column![
                        text("Output Device").size(12).color(th::text()),
                        output_status
                    ]
                    .spacing(3)
                    .width(Length::Fill),
                    output_picker
                ]
                .spacing(18)
                .align_y(iced::Alignment::Center),
                settings_divider(),
                row![
                    column![
                        text("Input Device").size(12).color(th::text()),
                        input_status
                    ]
                    .spacing(3)
                    .width(Length::Fill),
                    input_picker
                ]
                .spacing(18)
                .align_y(iced::Alignment::Center)
            ]
        };
        let device_group =
            container(device_rows.spacing(9))
                .padding([9, 10])
                .style(|_theme: &Theme| container::Style {
                    background: Some(th::bg_dark().into()),
                    border: iced::Border {
                        color: th::border(),
                        width: 1.0,
                        radius: 4.0.into(),
                    },
                    ..Default::default()
                });

        let sizes = self.state.audio_settings.buffer_size_choices();
        let mut buf_row = row![].spacing(3);
        for size in sizes {
            let is_selected = self.state.audio_settings.buffer_size == size;
            let label = format!("{size}");
            let btn = button(text(label).size(11).color(if is_selected {
                th::text()
            } else {
                th::text_dim()
            }))
            .on_press(Message::SetBufferSize(size))
            .width(Length::Fixed(40.0))
            .padding([4, 5])
            .style(move |_theme: &Theme, status| {
                if is_selected {
                    button::Style {
                        background: Some(th::accent_dim().into()),
                        text_color: th::text(),
                        border: iced::Border {
                            color: th::accent(),
                            width: 1.0,
                            radius: 3.0.into(),
                        },
                        ..Default::default()
                    }
                } else {
                    let bg = match status {
                        button::Status::Hovered | button::Status::Pressed => {
                            Some(th::bg_hover().into())
                        }
                        _ => None,
                    };
                    button::Style {
                        background: bg,
                        text_color: th::text_dim(),
                        border: iced::Border {
                            color: th::border(),
                            width: 1.0,
                            radius: 3.0.into(),
                        },
                        ..Default::default()
                    }
                }
            });
            buf_row = buf_row.push(btn);
        }

        let sr_picker = settings_pick_list(
            self.state.audio_settings.sample_rate_choices(),
            Some(crate::domains::audio_settings::AudioSampleRate(
                self.state.audio_settings.sample_rate,
            )),
            Message::SetAudioSampleRate,
            132.0,
        );
        let buffer_latency_ms = self.state.audio_settings.buffer_size as f64
            / self.state.audio_settings.sample_rate as f64
            * 1_000.0;
        let engine_header = row![
            column![
                text("Audio Engine").size(14).color(th::text()),
                text("Lower buffers feel faster; higher buffers leave more CPU headroom")
                    .size(10)
                    .color(th::text_dim())
            ]
            .spacing(2)
            .width(Length::Fill),
            text(format!("{buffer_latency_ms:.1} ms"))
                .size(10)
                .color(th::accent())
        ]
        .align_y(iced::Alignment::Center);
        let engine_group = container(
            column![
                row![
                    text("Sample Rate").size(12).color(th::text()),
                    horizontal_space(),
                    sr_picker
                ]
                .align_y(iced::Alignment::Center),
                settings_divider(),
                row![
                    column![
                        text("Buffer Size").size(12).color(th::text()),
                        text("frames").size(9).color(th::text_muted())
                    ]
                    .spacing(1)
                    .width(Length::Fill),
                    buf_row
                ]
                .spacing(18)
                .align_y(iced::Alignment::Center),
            ]
            .spacing(9),
        )
        .padding([9, 10])
        .style(|_theme: &Theme| container::Style {
            background: Some(th::bg_dark().into()),
            border: iced::Border {
                color: th::border(),
                width: 1.0,
                radius: 4.0.into(),
            },
            ..Default::default()
        });

        // ---- MIDI input picker ----
        let midi_hint = text(
            "External MIDI routes to the currently selected instrument track. \
             Plug your keyboard or Push in, hit Rescan, then pick the port.",
        )
        .size(11)
        .color(th::text_dim());

        let current_port_line: Element<'_, Message> = match self.midi_input.as_ref() {
            Some(h) => text(format!("Connected: {}", h.port_name))
                .size(12)
                .color(th::accent())
                .into(),
            None => text("Not connected").size(12).color(th::text_dim()).into(),
        };

        let rescan_btn = button(text("Rescan ports").size(11).color(th::text()))
            .on_press(Message::RescanMidiInputs)
            .padding([4, 10])
            .style(|_theme: &Theme, status| {
                let bg = match status {
                    button::Status::Hovered | button::Status::Pressed => {
                        Some(th::bg_hover().into())
                    }
                    _ => None,
                };
                button::Style {
                    background: bg,
                    text_color: th::text(),
                    border: iced::Border {
                        color: th::border(),
                        width: 1.0,
                        radius: 4.0.into(),
                    },
                    ..Default::default()
                }
            });

        let disconnect_btn = button(text("Disconnect").size(11).color(th::text_dim()))
            .on_press(Message::CloseMidiInput)
            .padding([4, 10])
            .style(|_theme: &Theme, status| {
                let bg = match status {
                    button::Status::Hovered | button::Status::Pressed => {
                        Some(th::bg_hover().into())
                    }
                    _ => None,
                };
                button::Style {
                    background: bg,
                    text_color: th::text_dim(),
                    border: iced::Border {
                        color: th::border(),
                        width: 1.0,
                        radius: 4.0.into(),
                    },
                    ..Default::default()
                }
            });

        let midi_actions = row![rescan_btn, disconnect_btn]
            .spacing(6)
            .align_y(iced::Alignment::Center);

        let mut port_list = column![].spacing(3);
        for name in &self.midi_input_ports {
            let is_current = self
                .midi_input
                .as_ref()
                .map(|h| h.port_name == *name)
                .unwrap_or(false);
            let label = name.clone();
            let port_btn = button(
                text(if is_current {
                    format!("● {name}")
                } else {
                    name.clone()
                })
                .size(11)
                .color(if is_current { th::accent() } else { th::text() }),
            )
            .on_press(Message::OpenMidiInput(label))
            .padding([4, 10])
            .width(Length::Fill)
            .style(move |_theme: &Theme, status| {
                let bg = if is_current {
                    Some(th::bg_hover().into())
                } else {
                    match status {
                        button::Status::Hovered | button::Status::Pressed => {
                            Some(th::bg_hover().into())
                        }
                        _ => None,
                    }
                };
                button::Style {
                    background: bg,
                    text_color: if is_current { th::accent() } else { th::text() },
                    border: iced::Border {
                        color: if is_current {
                            th::accent_dim()
                        } else {
                            th::border()
                        },
                        width: 1.0,
                        radius: 4.0.into(),
                    },
                    ..Default::default()
                }
            });
            port_list = port_list.push(port_btn);
        }

        let midi_header = row![
            column![text("MIDI Input").size(14).color(th::text()), midi_hint]
                .spacing(2)
                .width(Length::Fill),
            midi_actions
        ]
        .align_y(iced::Alignment::Center);

        let mut body = column![hardware_header].spacing(10);
        if let Some(error) = self.state.audio_settings.catalog_error.as_ref() {
            body = body.push(
                container(
                    text(format!("Device scan failed · {error}"))
                        .size(10)
                        .color(th::danger()),
                )
                .padding([5, 8])
                .width(Length::Fill)
                .style(|_theme: &Theme| container::Style {
                    background: Some(th::bg_dark().into()),
                    border: iced::Border {
                        color: th::danger(),
                        width: 1.0,
                        radius: 3.0.into(),
                    },
                    ..Default::default()
                }),
            );
        }
        body = body
            .push(device_group)
            .push(engine_header)
            .push(engine_group)
            .push(settings_divider())
            .push(midi_header)
            .push(current_port_line)
            .push(port_list);
        scrollable(container(body).padding([0, 10]))
            .height(Length::Fixed(440.0))
            .into()
    }
}
