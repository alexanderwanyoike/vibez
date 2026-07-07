//! Split out of app.rs; inherent methods on [`super::App`].

use iced::widget::{
    button, canvas, column, container, horizontal_space, mouse_area, row, scrollable, text,
};
use iced::{Color, Element, Length, Theme};

use vibez_core::id::TrackId;
use vibez_core::track::MediaSourceRef;
use vibez_plugin_host::gui::PluginGuiKey;

use crate::icons;
use crate::message::{DrumPadParam, Message};
use crate::state::UiTrack;
use crate::theme as th;
use crate::widgets::effect_slot::view_effect_slot;

use super::*;

impl App {
    /// Build the device chain for the detail panel.
    pub(super) fn view_device_chain<'a>(
        &'a self,
        track_id: TrackId,
        track: &'a UiTrack,
        track_color: Color,
    ) -> Element<'a, Message> {
        // Header: track name + right-click hint
        let track_label = text(format!("{} — Devices", track.name))
            .size(13)
            .color(th::TEXT);

        let hint_label = text("Right-click to add").size(10).color(th::TEXT_MUTED);

        let header = row![track_label, horizontal_space(), hint_label]
            .spacing(8)
            .align_y(iced::Alignment::Center);

        // Device cards
        let mut devices_row = row![].spacing(6);

        // Instrument device card (branched by kind)
        if track.has_instrument {
            if track.plugin_instrument_name.is_some() {
                // External plugin instrument — clickable card
                let card = self.view_plugin_instrument_device(track_id, track, track_color);
                devices_row = devices_row.push(card);
            } else {
                match track.instrument_kind {
                    Some(vibez_core::midi::InstrumentKind::Sampler) => {
                        let card = self.view_sampler_device(track_id, track, track_color);
                        devices_row = devices_row.push(card);
                    }
                    Some(vibez_core::midi::InstrumentKind::DrumRack) => {
                        let card = self.view_drum_rack_device(track_id, track, track_color);
                        devices_row = devices_row.push(card);
                    }
                    _ => {
                        let synth_card = self.view_synth_device(track_id, track, track_color);
                        devices_row = devices_row.push(synth_card);
                    }
                }
            }
        } else if track.kind.is_midi() {
            let placeholder = self.view_add_instrument_placeholder();
            devices_row = devices_row.push(placeholder);
        }

        // Effect cards
        for effect in &track.effects {
            let slot = view_effect_slot(track_id, effect, track_color);
            devices_row = devices_row.push(slot);
        }

        // Horizontal only: the panel is now a fixed-height strip the
        // cards fit exactly, Ableton-style.
        let scrollable_devices =
            scrollable(devices_row).direction(scrollable::Direction::Horizontal(
                scrollable::Scrollbar::new()
                    .width(5)
                    .scroller_width(5)
                    .spacing(2),
            ));

        let content = column![header, scrollable_devices]
            .spacing(6)
            .padding(8)
            .width(Length::Fill);

        // Wrap in mouse_area for right-click context menu
        mouse_area(content)
            .on_right_press(Message::Devices(
                crate::domains::devices::DevicesMsg::ShowContextMenu {
                    x: self.state.view.cursor_x,
                    y: self.state.view.cursor_y,
                    track_id,
                },
            ))
            .into()
    }

    // ── Shared device card helpers ──────────────────────────────────

    /// Dark title bar used by all device cards.
    pub(super) fn device_title_bar<'a>(
        content: impl Into<Element<'a, Message>>,
    ) -> iced::widget::Container<'a, Message> {
        container(content)
            .padding([4, 6])
            .width(Length::Fill)
            .style(|_theme: &Theme| container::Style {
                background: Some(th::BG_SURFACE.into()),
                ..Default::default()
            })
    }

    /// Standard device title row: colored dot, name, optional remove.
    pub(super) fn device_title_row(
        name: &str,
        track_color: Color,
        remove: Option<Message>,
    ) -> iced::widget::Row<'_, Message> {
        let dot = text("\u{25CF}").size(8).color(track_color);
        let name = text(name.to_string()).size(11).color(th::TEXT);
        let mut r = row![dot, name].spacing(5).align_y(iced::Alignment::Center);
        if let Some(msg) = remove {
            let remove_btn: Element<'_, Message> =
                Self::device_icon_btn(icons::X, th::TEXT_DIM, th::DANGER, msg).into();
            r = r.push(horizontal_space().width(Length::Fixed(12.0)));
            r = r.push(remove_btn);
        }
        r
    }

    /// Labeled section inside a device body, Ableton-style: a tiny
    /// uppercase header above the section's controls.
    pub(super) fn device_section<'a>(
        label: &'static str,
        content: Element<'a, Message>,
    ) -> Element<'a, Message> {
        column![text(label).size(8).color(th::TEXT_MUTED), content]
            .spacing(6)
            .align_x(iced::Alignment::Start)
            .into()
    }

    /// Thin vertical rule separating device sections.
    pub(super) fn device_divider() -> Element<'static, Message> {
        container(text(""))
            .width(Length::Fixed(1.0))
            .height(Length::Fixed(th::DEVICE_BODY_H - 12.0))
            .style(|_theme: &Theme| container::Style {
                background: Some(th::DIVIDER.into()),
                ..Default::default()
            })
            .into()
    }

    /// Standard device body: fixed rack height so every card in the
    /// chain lines up like a hardware rack, consistent padding.
    pub(super) fn device_body(content: Element<'_, Message>) -> Element<'_, Message> {
        container(content)
            .padding([8, 10])
            .height(Length::Fixed(th::DEVICE_BODY_H))
            .into()
    }

    /// Wrap card content in the standard device card container.
    pub(super) fn device_card(content: iced::widget::Column<'_, Message>) -> Element<'_, Message> {
        container(content)
            .style(|_theme: &Theme| container::Style {
                background: Some(th::BG_ELEVATED.into()),
                border: iced::Border {
                    color: th::BORDER,
                    width: 1.0,
                    radius: 4.0.into(),
                },
                ..Default::default()
            })
            .into()
    }

    /// Small icon-only button for device card actions.
    pub(super) fn device_icon_btn(
        icon_char: char,
        color: Color,
        hover_color: Color,
        msg: Message,
    ) -> iced::widget::Button<'static, Message> {
        button(icons::icon(icon_char).size(12).color(color))
            .on_press(msg)
            .padding([3, 5])
            .style(move |_theme: &Theme, status| {
                let (bg, tc) = match status {
                    button::Status::Hovered => (Some(th::BG_HOVER.into()), hover_color),
                    button::Status::Pressed => (Some(th::BG_DARK.into()), hover_color),
                    _ => (None, color),
                };
                button::Style {
                    background: bg,
                    text_color: tc,
                    border: iced::Border {
                        radius: 3.0.into(),
                        ..Default::default()
                    },
                    ..Default::default()
                }
            })
    }

    /// Device card for an external plugin instrument.
    pub(super) fn view_plugin_instrument_device<'a>(
        &'a self,
        track_id: TrackId,
        track: &'a UiTrack,
        track_color: Color,
    ) -> Element<'a, Message> {
        let dot = text("\u{25CF}").size(9).color(track_color);
        let plugin_name = track.plugin_instrument_name.as_deref().unwrap_or("Plugin");

        let name_section =
            container(text(plugin_name).size(11).color(th::TEXT)).width(Length::Fill);

        // Edit button for plugins with a native GUI
        let edit_btn: Option<iced::widget::Button<'_, Message>> = if track.has_plugin_instrument_gui
        {
            let gui_key = PluginGuiKey::Instrument { track_id };
            Some(
                button(text("Edit").size(9).color(th::TEXT_DIM))
                    .on_press(Message::OpenPluginGui(gui_key))
                    .padding([2, 5])
                    .style(|_theme: &Theme, status| {
                        let (bg, tc) = match status {
                            button::Status::Hovered => (Some(th::BG_HOVER.into()), th::ACCENT),
                            _ => (None, th::TEXT_DIM),
                        };
                        button::Style {
                            background: bg,
                            text_color: tc,
                            border: iced::Border {
                                color: th::BORDER,
                                width: 1.0,
                                radius: 3.0.into(),
                            },
                            ..Default::default()
                        }
                    }),
            )
        } else {
            None
        };

        let remove: Element<'a, Message> = Self::device_icon_btn(
            icons::X,
            th::TEXT_DIM,
            th::DANGER,
            Message::remove_track_instrument(track_id),
        )
        .into();

        let mut title_row = row![dot, name_section]
            .spacing(4)
            .align_y(iced::Alignment::Center);
        if let Some(eb) = edit_btn {
            title_row = title_row.push(eb);
        }
        title_row = title_row.push(remove);

        let title = Self::device_title_bar(title_row);

        Self::device_card(column![title].width(Length::Fixed(200.0)))
    }

    /// Synth device card for instrument tracks.
    pub(super) fn view_synth_device<'a>(
        &'a self,
        track_id: TrackId,
        track: &'a UiTrack,
        track_color: Color,
    ) -> Element<'a, Message> {
        use crate::widgets::effect_knob::{format_value, param_column, EffectKnobWidget};
        let title = Self::device_title_bar(Self::device_title_row(
            "Synth",
            track_color,
            Some(Message::remove_track_instrument(track_id)),
        ));

        let descriptors = vibez_instruments::synth::SYNTH_PARAMS;
        let value_of = |i: usize| {
            track
                .instrument_params
                .get(i)
                .copied()
                .unwrap_or(descriptors[i].default)
        };
        let knob = |i: usize, label: &str| {
            param_column(
                EffectKnobWidget::for_instrument(
                    track_id,
                    i,
                    value_of(i),
                    descriptors[i].min,
                    descriptors[i].max,
                    descriptors[i].default,
                    track_color,
                ),
                label.to_string(),
                format_value(value_of(i), descriptors[i].unit),
            )
        };

        // OSC section: 2x2 waveform selector grid.
        let wave_value = value_of(0).round() as usize;
        let wave_btn = |i: usize, label: &'static str| {
            let active = wave_value == i;
            button(
                text(label)
                    .size(9)
                    .width(Length::Fixed(30.0))
                    .align_x(iced::Alignment::Center)
                    .color(if active { th::BG_DARK } else { th::TEXT_DIM }),
            )
            .on_press(Message::set_instrument_param(track_id, 0, i as f32))
            .padding([3, 4])
            .style(move |_theme: &Theme, _status| button::Style {
                background: Some(if active { th::ACCENT } else { th::BG_DARK }.into()),
                text_color: if active { th::BG_DARK } else { th::TEXT_DIM },
                border: iced::Border {
                    color: if active { th::ACCENT } else { th::BORDER },
                    width: 1.0,
                    radius: 3.0.into(),
                },
                ..Default::default()
            })
        };
        let scope: Element<'a, Message> = canvas(crate::widgets::mini_waveform::OscScope {
            waveform_index: wave_value,
            color: track_color,
        })
        .width(Length::Fixed(66.0))
        .height(Length::Fixed(38.0))
        .into();
        let osc = column![
            scope,
            row![wave_btn(0, "Sin"), wave_btn(1, "Saw")].spacing(3),
            row![wave_btn(2, "Sqr"), wave_btn(3, "Tri")].spacing(3),
        ]
        .spacing(3)
        .align_x(iced::Alignment::Center);

        let adsr: Element<'a, Message> = canvas(crate::widgets::mini_waveform::AdsrScope {
            attack: value_of(1),
            decay: value_of(2),
            sustain: value_of(3),
            release: value_of(4),
            color: track_color,
        })
        .width(Length::Fixed(240.0))
        .height(Length::Fixed(34.0))
        .into();
        let envelope = column![
            adsr,
            row![
                knob(1, "Attack"),
                knob(2, "Decay"),
                knob(3, "Sustain"),
                knob(4, "Release")
            ]
            .spacing(6)
        ]
        .spacing(5)
        .align_x(iced::Alignment::Center);

        let body = row![
            Self::device_section("OSC", osc.into()),
            Self::device_divider(),
            Self::device_section("ENVELOPE", envelope.into()),
            Self::device_divider(),
            Self::device_section(
                "FILTER",
                row![knob(5, "Cutoff"), knob(6, "Res")].spacing(6).into()
            ),
            Self::device_divider(),
            Self::device_section("OUT", knob(7, "Volume")),
        ]
        .spacing(10)
        .align_y(iced::Alignment::Start);

        // OSC 66 + envelope 240 + filter 118 + out 56 + chrome.
        Self::device_card(
            column![title, Self::device_body(body.into())].width(Length::Fixed(570.0)),
        )
    }

    /// Sampler device card.
    pub(super) fn view_sampler_device<'a>(
        &'a self,
        track_id: TrackId,
        track: &'a UiTrack,
        track_color: Color,
    ) -> Element<'a, Message> {
        use crate::widgets::effect_knob::{format_value, param_column, EffectKnobWidget};
        let title = Self::device_title_bar(Self::device_title_row(
            "Sampler",
            track_color,
            Some(Message::remove_track_instrument(track_id)),
        ));

        let descriptors = vibez_instruments::sampler::SAMPLER_PARAMS;
        let value_of = |i: usize| {
            track
                .instrument_params
                .get(i)
                .copied()
                .unwrap_or(descriptors[i].default)
        };
        let knob = |i: usize, label: &str| {
            param_column(
                EffectKnobWidget::for_instrument(
                    track_id,
                    i,
                    value_of(i),
                    descriptors[i].min,
                    descriptors[i].max,
                    descriptors[i].default,
                    track_color,
                ),
                label.to_string(),
                format_value(value_of(i), descriptors[i].unit),
            )
        };

        // Long file names must not blow the section open; the
        // waveform display defines the section width.
        let sample_label = match &track.sample_name {
            Some(name) => {
                let display = if name.chars().count() > 24 {
                    let head: String = name.chars().take(21).collect();
                    format!("{head}...")
                } else {
                    name.clone()
                };
                text(display).size(10).color(th::TEXT)
            }
            None => text("No Sample").size(10).color(th::TEXT_MUTED),
        };
        let load_btn = button(text("Load").size(9).color(th::TEXT))
            .on_press(Message::LoadSamplerSample(track_id))
            .padding([2, 8])
            .style(|_theme: &Theme, status| {
                let bg = match status {
                    button::Status::Hovered => th::BG_HOVER,
                    _ => th::BG_DARK,
                };
                button::Style {
                    background: Some(bg.into()),
                    border: iced::Border {
                        color: th::BORDER,
                        width: 1.0,
                        radius: 3.0.into(),
                    },
                    text_color: th::TEXT,
                    ..Default::default()
                }
            });
        let waveform: Element<'a, Message> = canvas(crate::widgets::mini_waveform::MiniWaveform {
            audio: track.sample_audio.clone(),
            color: track_color,
            region: None,
        })
        .width(Length::Fixed(190.0))
        .height(Length::Fixed(56.0))
        .into();
        let sample = column![
            waveform,
            row![sample_label, load_btn]
                .spacing(8)
                .align_y(iced::Alignment::Center)
        ]
        .spacing(5)
        .width(Length::Fixed(190.0))
        .align_x(iced::Alignment::Start);

        let adsr: Element<'a, Message> = canvas(crate::widgets::mini_waveform::AdsrScope {
            attack: value_of(1),
            decay: value_of(2),
            sustain: value_of(3),
            release: value_of(4),
            color: track_color,
        })
        .width(Length::Fixed(240.0))
        .height(Length::Fixed(34.0))
        .into();
        let envelope = column![
            adsr,
            row![
                knob(1, "Attack"),
                knob(2, "Decay"),
                knob(3, "Sustain"),
                knob(4, "Release")
            ]
            .spacing(6)
        ]
        .spacing(5)
        .align_x(iced::Alignment::Center);

        let body = row![
            Self::device_section("SAMPLE", sample.into()),
            Self::device_divider(),
            Self::device_section("TUNE", knob(0, "Root")),
            Self::device_divider(),
            Self::device_section("ENVELOPE", envelope.into()),
        ]
        .spacing(10)
        .align_y(iced::Alignment::Start);

        // Width = sample 190 + tune 56 + envelope 240 + dividers,
        // spacing, padding. Fixed so the Fill title strip and card
        // background actually render (Fill inside a shrink column
        // collapses in iced).
        let card = Self::device_card(
            column![title, Self::device_body(body.into())].width(Length::Fixed(560.0)),
        );
        // The whole card accepts browser drops, like drum pads.
        mouse_area(card)
            .on_release(Message::DropSampleOnSampler { track_id })
            .into()
    }

    pub(super) fn view_drum_rack_device<'a>(
        &'a self,
        track_id: TrackId,
        track: &'a UiTrack,
        track_color: Color,
    ) -> Element<'a, Message> {
        use crate::widgets::effect_knob::{param_column, EffectKnobWidget};
        let selected_pad = track
            .selected_drum_pad
            .min(track.drum_rack_pads.len().saturating_sub(1));
        let title = Self::device_title_bar(Self::device_title_row(
            "Drum Rack",
            track_color,
            Some(Message::remove_track_instrument(track_id)),
        ));

        let mut grid = column![].spacing(4);
        for row_index in 0..4 {
            let mut pad_row = row![].spacing(4);
            for col_index in 0..4 {
                let pad_index = row_index * 4 + col_index;
                let pad = &track.drum_rack_pads[pad_index];
                let active = selected_pad == pad_index;
                // Hard-truncated single line: wrapping names change
                // the tile height and blow the card's height budget
                // (clipped knobs, dogfood screenshot 2026-07-06).
                let label = pad
                    .name
                    .as_deref()
                    .map(|name| {
                        let short: String = name.chars().take(6).collect();
                        if name.chars().count() > 6 {
                            format!("{short}..")
                        } else {
                            short
                        }
                    })
                    .unwrap_or_else(|| format!("Pad {}", pad_index + 1));
                // Use container + mouse_area so press events reach us and
                // drag-drop works. iced Button would capture ButtonPressed
                // and hide it from mouse_area.
                let pad_note = crate::widgets::piano_roll::pitch_name(36 + pad_index as u8);
                let pad_body = container(
                    column![
                        text(format!("{:02}  {pad_note}", pad_index + 1))
                            .size(9)
                            .color(if active { th::ACCENT } else { th::TEXT_DIM }),
                        text(label)
                            .size(8)
                            .color(if active { th::ACCENT } else { th::TEXT })
                    ]
                    .spacing(2)
                    .align_x(iced::Alignment::Center),
                )
                .padding([3, 4])
                .width(Length::Fixed(52.0))
                .height(Length::Fixed(30.0))
                .style(move |_theme: &Theme| container::Style {
                    background: Some(if active { th::ACCENT_DIM } else { th::BG_DARK }.into()),
                    text_color: Some(if active { th::ACCENT } else { th::TEXT }),
                    border: iced::Border {
                        color: if active { th::ACCENT_DIM } else { th::BORDER },
                        width: 1.0,
                        radius: 4.0.into(),
                    },
                    ..Default::default()
                });
                // on_release handler selects the pad when no drag is active,
                // otherwise routes through DropSampleOnDrumPad.
                let pad_cell: Element<'a, Message> = mouse_area(pad_body)
                    .on_release(Message::DropSampleOnDrumPad {
                        track_id,
                        pad_index,
                    })
                    .into();
                pad_row = pad_row.push(pad_cell);
            }
            grid = grid.push(pad_row);
        }

        let selected_name = track.drum_rack_pads[selected_pad]
            .name
            .clone()
            .unwrap_or_else(|| "No sample loaded".to_string());
        let source_hint = track.drum_rack_pads[selected_pad]
            .source
            .as_ref()
            .map(MediaSourceRef::display_name)
            .unwrap_or_else(|| "Use the browser or Load".to_string());
        let selected_pad_state = &track.drum_rack_pads[selected_pad];

        let load_btn = button(text("Load").size(9).color(th::TEXT))
            .on_press(Message::LoadDrumRackPadSample(track_id, selected_pad))
            .padding([2, 8])
            .style(|_theme: &Theme, status| {
                let bg = match status {
                    button::Status::Hovered => th::BG_HOVER,
                    _ => th::BG_DARK,
                };
                button::Style {
                    background: Some(bg.into()),
                    border: iced::Border {
                        color: th::BORDER,
                        width: 1.0,
                        radius: 3.0.into(),
                    },
                    text_color: th::TEXT,
                    ..Default::default()
                }
            });

        let clear_btn = button(text("Clear").size(9).color(th::TEXT_DIM))
            .on_press(Message::clear_drum_rack_pad(track_id, selected_pad))
            .padding([2, 8])
            .style(|_theme: &Theme, status| {
                let bg = match status {
                    button::Status::Hovered | button::Status::Pressed => Some(th::BG_HOVER.into()),
                    _ => Some(th::BG_DARK.into()),
                };
                button::Style {
                    background: bg,
                    border: iced::Border {
                        color: th::BORDER,
                        width: 1.0,
                        radius: 3.0.into(),
                    },
                    text_color: th::TEXT_DIM,
                    ..Default::default()
                }
            });

        let footer = column![
            text(truncate_end(&selected_name, 22))
                .size(10)
                .color(th::TEXT),
            text(truncate_end(&source_hint, 26))
                .size(9)
                .color(th::TEXT_DIM),
            row![load_btn, clear_btn]
                .spacing(6)
                .align_y(iced::Alignment::Center)
        ]
        .spacing(4);

        let drum_params = [
            (
                "Gain",
                format!("{:.2}", selected_pad_state.gain),
                selected_pad_state.gain,
                0.0,
                2.0,
                1.0,
                DrumPadParam::Gain,
            ),
            (
                "Pan",
                format!("{:.2}", selected_pad_state.pan),
                selected_pad_state.pan,
                -1.0,
                1.0,
                0.0,
                DrumPadParam::Pan,
            ),
            (
                "Start",
                format!("{:.0}%", selected_pad_state.start * 100.0),
                selected_pad_state.start,
                0.0,
                1.0,
                0.0,
                DrumPadParam::Start,
            ),
            (
                "End",
                format!("{:.0}%", selected_pad_state.end * 100.0),
                selected_pad_state.end,
                0.0,
                1.0,
                1.0,
                DrumPadParam::End,
            ),
            (
                "Coarse",
                format!("{}st", selected_pad_state.coarse_tune),
                selected_pad_state.coarse_tune as f32,
                -24.0,
                24.0,
                0.0,
                DrumPadParam::CoarseTune,
            ),
            (
                "Fine",
                format!("{:.0}ct", selected_pad_state.fine_tune),
                selected_pad_state.fine_tune,
                -100.0,
                100.0,
                0.0,
                DrumPadParam::FineTune,
            ),
        ];

        let mut knob_row = row![].spacing(6);
        for (label_text, value_text, value, min, max, default, param) in drum_params.iter() {
            let knob = EffectKnobWidget::for_drum_pad(
                track_id,
                selected_pad,
                *param,
                *value,
                *min,
                *max,
                *default,
                track_color,
            );
            knob_row = knob_row.push(param_column(
                knob,
                label_text.to_string(),
                value_text.clone(),
            ));
        }

        let one_shot_active = selected_pad_state.one_shot;
        let one_shot_btn = button(text("One-shot").size(9).color(if one_shot_active {
            th::ACCENT
        } else {
            th::TEXT_DIM
        }))
        .on_press(Message::Devices(
            crate::domains::devices::DevicesMsg::SetDrumPadOneShot {
                track_id,
                pad_index: selected_pad,
                one_shot: !one_shot_active,
            },
        ))
        .padding([2, 6])
        .style(move |_theme: &Theme, _status| button::Style {
            background: Some(
                if one_shot_active {
                    th::ACCENT_DIM
                } else {
                    th::BG_DARK
                }
                .into(),
            ),
            text_color: if one_shot_active {
                th::ACCENT
            } else {
                th::TEXT_DIM
            },
            border: iced::Border {
                color: if one_shot_active {
                    th::ACCENT_DIM
                } else {
                    th::BORDER
                },
                width: 1.0,
                radius: 3.0.into(),
            },
            ..Default::default()
        });

        let mut choke_row = row![text("Choke").size(9).color(th::TEXT_DIM)]
            .spacing(2)
            .align_y(iced::Alignment::Center);
        for (group, label) in [
            (None, "Off"),
            (Some(1), "1"),
            (Some(2), "2"),
            (Some(3), "3"),
            (Some(4), "4"),
        ] {
            let active = selected_pad_state.choke_group == group;
            let btn =
                button(
                    text(label)
                        .size(9)
                        .color(if active { th::ACCENT } else { th::TEXT_DIM }),
                )
                .on_press(Message::Devices(
                    crate::domains::devices::DevicesMsg::SetDrumPadChokeGroup {
                        track_id,
                        pad_index: selected_pad,
                        choke_group: group,
                    },
                ))
                .padding([2, 6])
                .style(move |_theme: &Theme, _status| button::Style {
                    background: Some(if active { th::ACCENT_DIM } else { th::BG_DARK }.into()),
                    text_color: if active { th::ACCENT } else { th::TEXT_DIM },
                    border: iced::Border {
                        color: if active { th::ACCENT_DIM } else { th::BORDER },
                        width: 1.0,
                        radius: 3.0.into(),
                    },
                    ..Default::default()
                });
            choke_row = choke_row.push(btn);
        }

        let pad_wave: Element<'a, Message> = canvas(crate::widgets::mini_waveform::MiniWaveform {
            audio: track.drum_rack_pads[selected_pad].audio.clone(),
            color: track_color,
            region: Some((selected_pad_state.start, selected_pad_state.end)),
        })
        .width(Length::Fixed(190.0))
        .height(Length::Fixed(40.0))
        .into();
        let editor = column![
            row![footer, pad_wave]
                .spacing(10)
                .align_y(iced::Alignment::Center),
            knob_row,
            row![one_shot_btn, choke_row]
                .spacing(8)
                .align_y(iced::Alignment::Center)
        ]
        .spacing(6);

        // Pads and the selected pad's editor sit side by side in
        // labeled sections; the panel scrolls horizontally so the
        // card grows sideways, never past the rack height.
        // Ableton-style pad bank: the grid lives in its own fixed
        // vertical viewport with a slim scrollbar; the card height
        // never depends on the pad count.
        let pads_view: Element<'a, Message> = scrollable(grid)
            .direction(scrollable::Direction::Vertical(
                scrollable::Scrollbar::new()
                    .width(4)
                    .scroller_width(4)
                    .spacing(2),
            ))
            .height(Length::Fixed(132.0))
            .width(Length::Fixed(232.0))
            .into();

        let body = row![
            Self::device_section("PADS", pads_view),
            Self::device_divider(),
            Self::device_section("PAD", editor.into()),
        ]
        .spacing(10)
        .align_y(iced::Alignment::Start);

        // Pads 220 + editor (six knob columns) + chrome.
        Self::device_card(
            column![title, Self::device_body(body.into())].width(Length::Fixed(650.0)),
        )
    }

    /// Placeholder card for MIDI tracks with no instrument attached.
    pub(super) fn view_add_instrument_placeholder(&self) -> Element<'_, Message> {
        let title = Self::device_title_bar(text("No Instrument").size(11).color(th::TEXT_DIM));
        let body = container(text("Right-click to add").size(9).color(th::TEXT_MUTED))
            .padding([8, 6])
            .width(Length::Fill);

        Self::device_card(
            column![title, body]
                .width(Length::Fixed(120.0))
                .align_x(iced::Alignment::Center),
        )
    }
}
