//! Split out of app.rs; inherent methods on [`super::App`].

use iced::widget::{
    button, canvas, column, container, horizontal_space, mouse_area, row, scrollable, text,
};
use iced::{Color, Element, Length, Theme};

use vibez_core::id::TrackId;
use vibez_core::track::MediaSourceRef;

use crate::domains::browser::BrowserMsg;
use crate::icons;
use crate::message::Message;
use crate::state::UiTrack;
use crate::theme as th;
use crate::widgets::device_chrome::section_label;
use crate::widgets::effect_slot::{view_effect_slot, view_sidechain, DeviceBody};

use super::*;

const CHAIN_SPACING: f32 = 6.0;
const SYNTH_CARD_W: f32 = 570.0;
const SAMPLER_CARD_W: f32 = 560.0;
const PLACEHOLDER_W: f32 = 120.0;
const DISPLAY_W: f32 = 380.0;
const BAND_COL_W: f32 = 50.0;
/// Padding (20), display, divider, four band columns and their spacing.
const EQ_CARD_W: f32 = DISPLAY_W + 4.0 * (BAND_COL_W + 4.0) + 48.0;

impl App {
    /// Build the device chain for the detail panel.
    pub(super) fn view_device_chain<'a>(
        &'a self,
        track_id: TrackId,
        track: &'a UiTrack,
        track_color: Color,
    ) -> Element<'a, Message> {
        // Header: track name + right-click hint
        let track_label = text(format!("{} - Devices", track.name))
            .size(13)
            .color(th::text());

        let hint_label = text("Right-click to add").size(10).color(th::text_muted());

        let header = row![track_label, horizontal_space(), hint_label]
            .spacing(8)
            .align_y(iced::Alignment::Center);

        let chain = iced::widget::responsive(move |size| {
            self.device_chain_row(track_id, track, track_color, size.width)
        });

        let content = column![header, chain]
            .spacing(6)
            .padding(8)
            .width(Length::Fill)
            .height(Length::Fill);

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

    /// The cards of one channel, laid out for the section's measured width.
    fn device_chain_row<'a>(
        &'a self,
        track_id: TrackId,
        track: &'a UiTrack,
        track_color: Color,
        available_width: f32,
    ) -> Element<'a, Message> {
        use crate::widgets::effect_slot::card_width;
        // Every other card is built first and measured, so each Gain can take
        // its share of whatever width the section has left.
        let mut cards: Vec<Option<Element<'a, Message>>> = Vec::new();
        let mut used = 0.0;
        if let Some((card, width)) = self.instrument_card(track_id, track, track_color) {
            cards.push(Some(card));
            used += width;
        }
        let mut gains = Vec::new();
        for effect in &track.effects {
            let sidechain = view_sidechain(
                track_id,
                effect,
                &self.state.project_tracks.tracks,
                &self.state.project_tracks.buses,
                &self.state.devices,
            );
            if crate::domains::gain::is_native_gain(effect) {
                used += card_width(effect, Some(0.0), sidechain.is_some());
                gains.push((cards.len(), effect, sidechain));
                cards.push(None);
                continue;
            }
            let custom = (effect.effect_type == vibez_core::effect::EffectType::Eq
                && effect.plugin_ref.is_none())
            .then(|| {
                let (content, width) = self.eq_device_body(track_id, effect);
                DeviceBody {
                    content,
                    width,
                    padding: iced::Padding::from([8.0, 10.0]),
                }
            });
            used += card_width(
                effect,
                custom.as_ref().map(|body| body.width),
                sidechain.is_some(),
            );
            cards.push(Some(view_effect_slot(
                track_id,
                effect,
                track_color,
                custom,
                sidechain,
                self.state.transport.sample_rate,
            )));
        }
        if !gains.is_empty() {
            let spacing = CHAIN_SPACING * cards.len().saturating_sub(1) as f32;
            let share = (available_width - used - spacing) / gains.len() as f32;
            let width = crate::widgets::gain_device::card_width(share);
            for (position, effect, sidechain) in gains {
                cards[position] = Some(view_effect_slot(
                    track_id,
                    effect,
                    track_color,
                    Some(self.gain_device_body(track_id, effect, track_color, width)),
                    sidechain,
                    self.state.transport.sample_rate,
                ));
            }
        }
        let devices_row = row(cards.into_iter().flatten())
            .spacing(CHAIN_SPACING)
            .height(Length::Fill);

        // Only a chain wider than the section scrolls, behind a hairline that
        // appears only then: scrollbar spacing would show it permanently.
        // Built with its direction: the default vertical check rejects the
        // full-height cards before a later `.direction` could apply.
        scrollable::Scrollable::with_direction(
            devices_row,
            scrollable::Direction::Horizontal(
                scrollable::Scrollbar::new().width(3).scroller_width(3),
            ),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    /// The channel's instrument card and its width, or the placeholder that
    /// offers one on a MIDI track.
    fn instrument_card<'a>(
        &'a self,
        track_id: TrackId,
        track: &'a UiTrack,
        track_color: Color,
    ) -> Option<(Element<'a, Message>, f32)> {
        if !track.has_instrument {
            return track
                .kind
                .is_midi()
                .then(|| (self.view_add_instrument_placeholder(), PLACEHOLDER_W));
        }
        if track.plugin_instrument_name.is_some() {
            return Some((
                self.view_plugin_instrument_device(track_id, track, track_color),
                crate::widgets::device_chrome::PLUGIN_CARD_W,
            ));
        }
        let (card, width) = match track.instrument_kind {
            Some(vibez_core::midi::InstrumentKind::Sampler) => (
                self.view_sampler_device(track_id, track, track_color),
                SAMPLER_CARD_W,
            ),
            Some(vibez_core::midi::InstrumentKind::DrumRack) => (
                self.view_drum_rack_device(track_id, track, track_color),
                drum_rack::DRUM_RACK_CARD_W,
            ),
            _ => (
                self.view_synth_device(track_id, track, track_color),
                SYNTH_CARD_W,
            ),
        };
        let latency = crate::domains::device_latency::latency_label(
            track.instrument_latency_samples,
            self.state.transport.sample_rate,
        );
        Some((
            column![card, text(latency).size(9).color(th::text_dim())]
                .height(Length::Fill)
                .into(),
            width,
        ))
    }

    fn gain_device_body<'a>(
        &'a self,
        track_id: TrackId,
        effect: &'a crate::state::UiEffect,
        accent: Color,
        width: f32,
    ) -> DeviceBody<'a> {
        let panel = self
            .gain_devices
            .panels
            .get(&effect.id)
            .unwrap_or(&self.gain_devices.empty_panel);
        let content = iced::widget::responsive(move |size| {
            crate::widgets::gain_device::view(
                track_id,
                effect,
                accent,
                panel,
                self.view_device_preset_bar(
                    track_id,
                    effect,
                    accent,
                    crate::widgets::gain_device::preset_bar_width(size),
                ),
                size,
            )
        });
        DeviceBody {
            content: content.into(),
            width,
            padding: iced::Padding::ZERO,
        }
    }

    /// Channel EQ card body: response display over the live analyser
    /// on the left, four color-coded band columns on the right. The
    /// analyser follows the selected track, which is exactly the
    /// track whose device chain is on screen.
    fn eq_device_body<'a>(
        &'a self,
        track_id: TrackId,
        effect: &'a crate::state::UiEffect,
    ) -> (Element<'a, Message>, f32) {
        use crate::widgets::effect_knob::{format_value, EffectKnobWidget};
        use crate::widgets::eq_display::EqDisplayWidget;

        let display = EqDisplayWidget {
            track_id,
            effect_id: effect.id,
            params: effect.params.clone(),
            descriptors: effect.descriptors,
            bypass: effect.bypass,
            sample_rate: self.state.transport.sample_rate as f32,
            spectrum: self.state.spectrum.bins.clone(),
            spectrum_active: self.state.spectrum.active,
        };
        let display_canvas: Element<'a, Message> = canvas(display)
            .width(Length::Fixed(DISPLAY_W))
            .height(Length::Fill)
            .into();

        // Band columns, left to right along the frequency axis:
        // (label, gain idx, freq idx, third idx, bell?, color)
        let bands: [(&str, usize, usize, usize, bool, Color); 4] = [
            ("LF", 0, 1, 2, true, th::eq_lf()),
            ("LMF", 3, 4, 5, false, th::eq_lmf()),
            ("HMF", 6, 7, 8, false, th::eq_hmf()),
            ("HF", 9, 10, 11, true, th::eq_hf()),
        ];

        let knob_color = if effect.bypass {
            th::text_muted()
        } else {
            th::text_dim()
        };
        let mut band_row = row![].spacing(4);
        for (label, gain_i, freq_i, third_i, is_bell, color) in bands {
            let band_color = if effect.bypass { knob_color } else { color };
            let knob = |i: usize, size: f32| -> Element<'a, Message> {
                let d = &effect.descriptors[i];
                let w = EffectKnobWidget::new(
                    track_id,
                    effect.id,
                    i,
                    effect.params.get(i).copied().unwrap_or(d.default),
                    d.min,
                    d.max,
                    d.default,
                    band_color,
                );
                canvas(w)
                    .width(Length::Fixed(size))
                    .height(Length::Fixed(size))
                    .into()
            };
            let gain_v = effect.params.get(gain_i).copied().unwrap_or(0.0);
            let freq_v = effect.params.get(freq_i).copied().unwrap_or(0.0);

            let mut col = column![
                text(label).size(9).color(band_color),
                knob(gain_i, 28.0),
                text(format!("{gain_v:+.1}")).size(8).color(th::text_dim()),
                knob(freq_i, 24.0),
                text(format_value(freq_v, "Hz"))
                    .size(8)
                    .color(th::text_dim()),
            ]
            .spacing(2)
            .width(Length::Fixed(BAND_COL_W))
            .align_x(iced::Alignment::Center);

            if is_bell {
                let bell_on = effect.params.get(third_i).copied().unwrap_or(0.0) >= 0.5;
                let bell = button(text("BELL").size(6).color(if bell_on {
                    th::bg_dark()
                } else {
                    th::text_dim()
                }))
                .on_press(Message::set_effect_param(
                    track_id,
                    effect.id,
                    third_i,
                    if bell_on { 0.0 } else { 1.0 },
                ))
                .padding([3, 6])
                .style(move |_theme: &Theme, _status| button::Style {
                    background: Some(if bell_on {
                        band_color.into()
                    } else {
                        th::bg_elevated().into()
                    }),
                    text_color: if bell_on {
                        th::bg_dark()
                    } else {
                        th::text_dim()
                    },
                    border: iced::Border {
                        color: th::border(),
                        width: 1.0,
                        radius: 2.0.into(),
                    },
                    ..Default::default()
                });
                col = col.push(bell);
            } else {
                let q_v = effect.params.get(third_i).copied().unwrap_or(0.7);
                col = col.push(
                    row![
                        text("Q").size(8).color(th::text_dim()),
                        knob(third_i, 20.0),
                        text(format!("{q_v:.2}")).size(8).color(th::text_dim()),
                    ]
                    .spacing(2)
                    .align_y(iced::Alignment::Center),
                );
            }
            band_row = band_row.push(col);
        }

        let body: Element<'a, Message> = row![
            display_canvas,
            Self::device_divider(),
            container(band_row).padding([0, 2]),
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center)
        .into();

        (body, EQ_CARD_W)
    }

    // ── Shared device card helpers ──────────────────────────────────

    /// Dark title bar used by all device cards.
    pub(super) fn device_title_bar<'a>(
        content: impl Into<Element<'a, Message>>,
    ) -> iced::widget::Container<'a, Message> {
        crate::widgets::device_chrome::title_bar(content)
    }

    /// Standard device title row: colored dot, name, optional remove.
    pub(super) fn device_title_row(
        name: &str,
        track_color: Color,
        remove: Option<Message>,
    ) -> iced::widget::Row<'_, Message> {
        let dot = text("\u{25CF}").size(8).color(track_color);
        let name = text(name.to_string()).size(11).color(th::text());
        let mut r = row![dot, name].spacing(5).align_y(iced::Alignment::Center);
        if let Some(msg) = remove {
            let remove_btn: Element<'_, Message> =
                Self::device_icon_btn(icons::X, th::text_dim(), th::danger(), msg).into();
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
        column![section_label(label), content]
            .spacing(6)
            .align_x(iced::Alignment::Start)
            .into()
    }

    /// Thin vertical rule separating device sections.
    pub(super) fn device_divider() -> Element<'static, Message> {
        crate::widgets::device_chrome::divider()
    }

    /// Standard device body: every card takes the section's height so the
    /// chain lines up like a hardware rack, content held at the top.
    pub(super) fn device_body(content: Element<'_, Message>) -> Element<'_, Message> {
        container(content)
            .padding([8, 10])
            .height(Length::Fill)
            .clip(true)
            .into()
    }

    /// Wrap card content in the standard device card container.
    pub(super) fn device_card(content: iced::widget::Column<'_, Message>) -> Element<'_, Message> {
        crate::widgets::device_chrome::card(content)
    }

    /// Small icon-only button for device card actions.
    pub(super) fn device_icon_btn(
        icon_char: char,
        color: Color,
        hover_color: Color,
        msg: Message,
    ) -> iced::widget::Button<'static, Message> {
        crate::widgets::device_chrome::icon_button(icon_char, color, hover_color, msg)
    }

    /// Device card for an external plugin instrument.
    pub(super) fn view_plugin_instrument_device<'a>(
        &'a self,
        track_id: TrackId,
        track: &'a UiTrack,
        track_color: Color,
    ) -> Element<'a, Message> {
        crate::widgets::device_chrome::plugin_instrument_card(
            track_id,
            track.plugin_instrument_name.as_deref().unwrap_or("Plugin"),
            track
                .plugin_instrument_ref
                .as_ref()
                .map(|device| device.format.as_str()),
            track.has_plugin_instrument_gui,
            crate::domains::device_latency::latency_label(
                track.instrument_latency_samples,
                self.state.transport.sample_rate,
            ),
            track_color,
        )
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
                    .color(if active {
                        th::bg_dark()
                    } else {
                        th::text_dim()
                    }),
            )
            .on_press(Message::set_instrument_param(track_id, 0, i as f32))
            .padding([3, 4])
            .style(move |_theme: &Theme, _status| button::Style {
                background: Some(if active { th::accent() } else { th::bg_dark() }.into()),
                text_color: if active {
                    th::bg_dark()
                } else {
                    th::text_dim()
                },
                border: iced::Border {
                    color: if active { th::accent() } else { th::border() },
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
            column![title, Self::device_body(body.into())].width(Length::Fixed(SYNTH_CARD_W)),
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
                text(display).size(10).color(th::text())
            }
            None => text("No Sample").size(10).color(th::text_muted()),
        };
        let load_btn = button(text("Load").size(9).color(th::text()))
            .on_press(Message::LoadSamplerSample(track_id))
            .padding([2, 8])
            .style(|_theme: &Theme, status| {
                let bg = match status {
                    button::Status::Hovered => th::bg_hover(),
                    _ => th::bg_dark(),
                };
                button::Style {
                    background: Some(bg.into()),
                    border: iced::Border {
                        color: th::border(),
                        width: 1.0,
                        radius: 3.0.into(),
                    },
                    text_color: th::text(),
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
            column![title, Self::device_body(body.into())].width(Length::Fixed(SAMPLER_CARD_W)),
        );
        let drop_target = matches!(
            self.state.browser.drag_target,
            Some(crate::state::BrowserDropTarget::Sampler { track_id: target }) if target == track_id
        );
        let card = container(card).style(move |_theme: &Theme| container::Style {
            border: iced::Border {
                color: if drop_target {
                    th::accent()
                } else {
                    iced::Color::TRANSPARENT
                },
                width: if drop_target { 2.0 } else { 0.0 },
                radius: 0.0.into(),
            },
            ..Default::default()
        });
        // The whole card accepts browser drops, like drum pads.
        mouse_area(card)
            .on_enter(Message::Browser(BrowserMsg::DragHoverSampler { track_id }))
            .on_exit(Message::Browser(BrowserMsg::ClearDragTarget))
            .on_release(Message::DropSampleOnSampler { track_id })
            .into()
    }

    /// Placeholder card for MIDI tracks with no instrument attached.
    pub(super) fn view_add_instrument_placeholder(&self) -> Element<'_, Message> {
        let title = Self::device_title_bar(text("No Instrument").size(11).color(th::text_dim()));
        let body = container(text("Right-click to add").size(9).color(th::text_muted()))
            .padding([8, 6])
            .width(Length::Fill);

        Self::device_card(
            column![title, body]
                .width(Length::Fixed(PLACEHOLDER_W))
                .align_x(iced::Alignment::Center),
        )
    }
}

#[path = "views_drum_rack.rs"]
mod drum_rack;
