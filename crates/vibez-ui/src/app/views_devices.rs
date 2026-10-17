//! Split out of app.rs; inherent methods on [`super::App`].

use iced::widget::{column, container, horizontal_space, mouse_area, row, scrollable, text};
use iced::{Color, Element, Length};

use vibez_core::id::TrackId;

use crate::icons;
use crate::message::Message;
use crate::state::UiTrack;
use crate::theme as th;
use crate::widgets::effect_slot::{view_effect_slot, view_sidechain, DeviceBody};

use super::*;

const CHAIN_SPACING: f32 = 6.0;
const PLACEHOLDER_W: f32 = 120.0;

/// Native devices that lay themselves out from the width they are given.
#[derive(Clone, Copy)]
enum Flexible {
    Gain,
    Eq,
    Compressor,
    Limiter,
    Filter,
    Gate,
    Drive,
    Saturator,
    Bitcrush,
    Delay,
    Echo,
    Reverb,
    AutoPan,
    Phaser,
}

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
        // Every other card is built first and measured, so each flexible
        // device can take its share of whatever width the section has left.
        let mut cards: Vec<Option<Element<'a, Message>>> = Vec::new();
        let mut used = 0.0;
        // The native synth and sampler take a flexible share like the
        // effects do.
        let synth = crate::domains::synth::is_native_synth(track);
        let sampler = crate::domains::sampler::is_native_sampler(track);
        let drum_rack = crate::domains::sampler::is_native_drum_rack(track);
        let wavetable = crate::domains::wavetable::is_native_wavetable(track);
        let fm = crate::domains::fm::is_native_fm(track);
        let flexible_instrument = synth || sampler || drum_rack || wavetable || fm;
        if flexible_instrument {
            cards.push(None);
        } else if let Some((card, width)) = self.instrument_card(track_id, track, track_color) {
            cards.push(Some(card));
            used += width;
        }
        let mut flexible = Vec::new();
        for effect in &track.effects {
            let sidechain = view_sidechain(
                track_id,
                effect,
                &self.state.project_tracks.tracks,
                &self.state.project_tracks.buses,
                &self.state.devices,
            );
            let kind = if crate::domains::gain::is_native_gain(effect) {
                Some(Flexible::Gain)
            } else if crate::domains::eq::is_native_eq(effect) {
                Some(Flexible::Eq)
            } else if crate::domains::compressor::is_native_compressor(effect) {
                Some(Flexible::Compressor)
            } else if crate::domains::limiter::is_native_limiter(effect) {
                Some(Flexible::Limiter)
            } else if crate::domains::filter::is_native_filter(effect) {
                Some(Flexible::Filter)
            } else if crate::domains::gate::is_native_gate(effect) {
                Some(Flexible::Gate)
            } else if crate::domains::drive::is_native_drive(effect) {
                Some(Flexible::Drive)
            } else if crate::domains::saturator::is_native_saturator(effect) {
                Some(Flexible::Saturator)
            } else if crate::domains::bitcrush::is_native_bitcrush(effect) {
                Some(Flexible::Bitcrush)
            } else if crate::domains::delay::is_native_delay(effect) {
                Some(Flexible::Delay)
            } else if crate::domains::echo::is_native_echo(effect) {
                Some(Flexible::Echo)
            } else if crate::domains::reverb::is_native_reverb(effect) {
                Some(Flexible::Reverb)
            } else if crate::domains::auto_pan::is_native_auto_pan(effect) {
                Some(Flexible::AutoPan)
            } else if crate::domains::phaser::is_native_phaser(effect) {
                Some(Flexible::Phaser)
            } else {
                None
            };
            if let Some(kind) = kind {
                used += card_width(effect, Some(0.0), sidechain.is_some());
                flexible.push((cards.len(), effect, sidechain, kind));
                cards.push(None);
                continue;
            }
            used += card_width(effect, None, sidechain.is_some());
            cards.push(Some(view_effect_slot(
                track_id,
                effect,
                track_color,
                None,
                sidechain,
                self.state.transport.sample_rate,
            )));
        }
        let shares = flexible.len() + usize::from(flexible_instrument);
        if shares > 0 {
            let spacing = CHAIN_SPACING * cards.len().saturating_sub(1) as f32;
            let share = (available_width - used - spacing) / shares as f32;
            if synth {
                cards[0] = Some(self.synth_card(
                    track_id,
                    track,
                    track_color,
                    crate::widgets::synth_device::card_width(share),
                ));
            }
            if sampler {
                cards[0] = Some(self.sampler_card(
                    track_id,
                    track,
                    track_color,
                    crate::widgets::sampler_device::card_width(share),
                ));
            }
            if wavetable {
                cards[0] = Some(self.wavetable_card(
                    track_id,
                    track,
                    track_color,
                    crate::widgets::wavetable_device::card_width(share),
                ));
            }
            if fm {
                cards[0] = Some(self.fm_card(
                    track_id,
                    track,
                    track_color,
                    crate::widgets::fm_device::card_width(share),
                ));
            }
            if drum_rack {
                cards[0] = Some(self.drum_rack_card(
                    track_id,
                    track,
                    track_color,
                    crate::widgets::drum_rack_device::card_width(share),
                ));
            }
            for (position, effect, sidechain, kind) in flexible {
                let body = match kind {
                    Flexible::Gain => self.gain_device_body(
                        track_id,
                        effect,
                        track_color,
                        crate::widgets::gain_device::card_width(share),
                    ),
                    Flexible::Eq => self.eq_device_body(
                        track_id,
                        effect,
                        track_color,
                        crate::widgets::eq_device::card_width(share),
                    ),
                    Flexible::Compressor => self.compressor_device_body(
                        track_id,
                        effect,
                        track_color,
                        crate::widgets::compressor_device::card_width(share),
                    ),
                    Flexible::Limiter => self.limiter_device_body(
                        track_id,
                        effect,
                        track_color,
                        crate::widgets::limiter_device::card_width(share),
                    ),
                    Flexible::Filter => self.filter_device_body(
                        track_id,
                        effect,
                        track_color,
                        crate::widgets::filter_device::card_width(share),
                    ),
                    Flexible::Gate => self.gate_device_body(
                        track_id,
                        effect,
                        track_color,
                        crate::widgets::gate_device::card_width(share),
                    ),
                    Flexible::Drive => self.drive_device_body(
                        track_id,
                        effect,
                        track_color,
                        crate::widgets::drive_device::card_width(share),
                    ),
                    Flexible::Saturator => self.saturator_device_body(
                        track_id,
                        effect,
                        track_color,
                        crate::widgets::saturator_device::card_width(share),
                    ),
                    Flexible::Bitcrush => self.bitcrush_device_body(
                        track_id,
                        effect,
                        track_color,
                        crate::widgets::bitcrush_device::card_width(share),
                    ),
                    Flexible::Delay => self.delay_device_body(
                        track_id,
                        effect,
                        track_color,
                        crate::widgets::delay_device::card_width(share),
                    ),
                    Flexible::Echo => self.echo_device_body(
                        track_id,
                        effect,
                        track_color,
                        crate::widgets::echo_device::card_width(share),
                    ),
                    Flexible::Reverb => self.reverb_device_body(
                        track_id,
                        effect,
                        track_color,
                        crate::widgets::reverb_device::card_width(share),
                    ),
                    Flexible::Phaser => self.phaser_device_body(
                        track_id,
                        effect,
                        track_color,
                        crate::widgets::phaser_device::card_width(share),
                    ),
                    Flexible::AutoPan => self.auto_pan_device_body(
                        track_id,
                        effect,
                        track_color,
                        crate::widgets::auto_pan_device::card_width(share),
                    ),
                };
                cards[position] = Some(view_effect_slot(
                    track_id,
                    effect,
                    track_color,
                    Some(body),
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
        // The native synth, sampler and drum rack are laid out with the
        // flexible cards.
        None
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
                self.param_entry.as_ref(),
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

    fn compressor_device_body<'a>(
        &'a self,
        track_id: TrackId,
        effect: &'a crate::state::UiEffect,
        accent: Color,
        width: f32,
    ) -> DeviceBody<'a> {
        let content = iced::widget::responsive(move |size| {
            crate::widgets::compressor_device::CompressorView {
                track_id,
                effect,
                accent,
                panel: self.compressor_devices.panel(effect.id),
                entry: self.param_entry.as_ref(),
            }
            .view(
                self.view_device_preset_bar(
                    track_id,
                    effect,
                    accent,
                    crate::widgets::compressor_device::preset_bar_width(size),
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

    fn reverb_device_body<'a>(
        &'a self,
        track_id: TrackId,
        effect: &'a crate::state::UiEffect,
        accent: Color,
        width: f32,
    ) -> DeviceBody<'a> {
        let content = iced::widget::responsive(move |size| {
            crate::widgets::reverb_device::ReverbView {
                track_id,
                effect,
                accent,
                panel: self.card_panels.panel(effect.id),
                entry: self.param_entry.as_ref(),
                bpm: self.state.transport.bpm,
            }
            .view(
                self.view_device_preset_bar(
                    track_id,
                    effect,
                    accent,
                    crate::widgets::reverb_device::preset_bar_width(size),
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

    fn phaser_device_body<'a>(
        &'a self,
        track_id: TrackId,
        effect: &'a crate::state::UiEffect,
        accent: Color,
        width: f32,
    ) -> DeviceBody<'a> {
        let content = iced::widget::responsive(move |size| {
            crate::widgets::phaser_device::PhaserView {
                track_id,
                effect,
                accent,
                panel: self.card_panels.panel(effect.id),
                entry: self.param_entry.as_ref(),
                bpm: self.state.transport.bpm,
            }
            .view(
                self.view_device_preset_bar(
                    track_id,
                    effect,
                    accent,
                    crate::widgets::phaser_device::preset_bar_width(size),
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

    fn auto_pan_device_body<'a>(
        &'a self,
        track_id: TrackId,
        effect: &'a crate::state::UiEffect,
        accent: Color,
        width: f32,
    ) -> DeviceBody<'a> {
        let content = iced::widget::responsive(move |size| {
            crate::widgets::auto_pan_device::AutoPanView {
                track_id,
                effect,
                accent,
                panel: self.card_panels.panel(effect.id),
                entry: self.param_entry.as_ref(),
                bpm: self.state.transport.bpm,
            }
            .view(
                self.view_device_preset_bar(
                    track_id,
                    effect,
                    accent,
                    crate::widgets::auto_pan_device::preset_bar_width(size),
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

    fn delay_device_body<'a>(
        &'a self,
        track_id: TrackId,
        effect: &'a crate::state::UiEffect,
        accent: Color,
        width: f32,
    ) -> DeviceBody<'a> {
        let content = iced::widget::responsive(move |size| {
            crate::widgets::delay_device::DelayView {
                track_id,
                effect,
                accent,
                panel: self.card_panels.panel(effect.id),
                entry: self.param_entry.as_ref(),
                bpm: self.state.transport.bpm,
            }
            .view(
                self.view_device_preset_bar(
                    track_id,
                    effect,
                    accent,
                    crate::widgets::delay_device::preset_bar_width(size),
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

    fn limiter_device_body<'a>(
        &'a self,
        track_id: TrackId,
        effect: &'a crate::state::UiEffect,
        accent: Color,
        width: f32,
    ) -> DeviceBody<'a> {
        let now = std::time::Instant::now();
        let content = iced::widget::responsive(move |size| {
            crate::widgets::limiter_device::LimiterView {
                track_id,
                effect,
                accent,
                panel: self.card_panels.panel(effect.id),
                entry: self.param_entry.as_ref(),
                now,
            }
            .view(
                self.view_device_preset_bar(
                    track_id,
                    effect,
                    accent,
                    crate::widgets::limiter_device::preset_bar_width(size),
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

    fn echo_device_body<'a>(
        &'a self,
        track_id: TrackId,
        effect: &'a crate::state::UiEffect,
        accent: Color,
        width: f32,
    ) -> DeviceBody<'a> {
        let content = iced::widget::responsive(move |size| {
            crate::widgets::echo_device::EchoView {
                track_id,
                effect,
                accent,
                panel: self.card_panels.panel(effect.id),
                entry: self.param_entry.as_ref(),
                bpm: self.state.transport.bpm,
            }
            .view(
                self.view_device_preset_bar(
                    track_id,
                    effect,
                    accent,
                    crate::widgets::echo_device::preset_bar_width(size),
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

    fn saturator_device_body<'a>(
        &'a self,
        track_id: TrackId,
        effect: &'a crate::state::UiEffect,
        accent: Color,
        width: f32,
    ) -> DeviceBody<'a> {
        let content = iced::widget::responsive(move |size| {
            crate::widgets::saturator_device::SaturatorView {
                track_id,
                effect,
                accent,
                panel: self.card_panels.panel(effect.id),
                entry: self.param_entry.as_ref(),
            }
            .view(
                self.view_device_preset_bar(
                    track_id,
                    effect,
                    accent,
                    crate::widgets::saturator_device::preset_bar_width(size),
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

    fn bitcrush_device_body<'a>(
        &'a self,
        track_id: TrackId,
        effect: &'a crate::state::UiEffect,
        accent: Color,
        width: f32,
    ) -> DeviceBody<'a> {
        let content = iced::widget::responsive(move |size| {
            crate::widgets::bitcrush_device::BitcrushView {
                track_id,
                effect,
                accent,
                panel: self.card_panels.panel(effect.id),
                entry: self.param_entry.as_ref(),
                host_rate: self.state.transport.sample_rate,
            }
            .view(
                self.view_device_preset_bar(
                    track_id,
                    effect,
                    accent,
                    crate::widgets::bitcrush_device::preset_bar_width(size),
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

    fn drive_device_body<'a>(
        &'a self,
        track_id: TrackId,
        effect: &'a crate::state::UiEffect,
        accent: Color,
        width: f32,
    ) -> DeviceBody<'a> {
        let content = iced::widget::responsive(move |size| {
            crate::widgets::drive_device::DriveView {
                track_id,
                effect,
                accent,
                panel: self.card_panels.panel(effect.id),
                entry: self.param_entry.as_ref(),
            }
            .view(
                self.view_device_preset_bar(
                    track_id,
                    effect,
                    accent,
                    crate::widgets::drive_device::preset_bar_width(size),
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

    fn gate_device_body<'a>(
        &'a self,
        track_id: TrackId,
        effect: &'a crate::state::UiEffect,
        accent: Color,
        width: f32,
    ) -> DeviceBody<'a> {
        let content = iced::widget::responsive(move |size| {
            crate::widgets::gate_device::GateView {
                track_id,
                effect,
                accent,
                panel: self.gate_panels.panel(effect.id),
                entry: self.param_entry.as_ref(),
            }
            .view(
                self.view_device_preset_bar(
                    track_id,
                    effect,
                    accent,
                    crate::widgets::gate_device::preset_bar_width(size),
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

    fn filter_device_body<'a>(
        &'a self,
        track_id: TrackId,
        effect: &'a crate::state::UiEffect,
        accent: Color,
        width: f32,
    ) -> DeviceBody<'a> {
        let content = iced::widget::responsive(move |size| {
            crate::widgets::filter_device::FilterView {
                track_id,
                effect,
                accent,
                panel: self.filter_panels.panel(effect.id),
                entry: self.param_entry.as_ref(),
                sample_rate: self.state.transport.sample_rate as f32,
            }
            .view(
                self.view_device_preset_bar(
                    track_id,
                    effect,
                    accent,
                    crate::widgets::filter_device::preset_bar_width(size),
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

    fn eq_device_body<'a>(
        &'a self,
        track_id: TrackId,
        effect: &'a crate::state::UiEffect,
        accent: Color,
        width: f32,
    ) -> DeviceBody<'a> {
        let content = iced::widget::responsive(move |size| {
            crate::widgets::eq_device::EqView {
                track_id,
                effect,
                accent,
                panel: self.eq_devices.panel(effect.id),
                entry: self.param_entry.as_ref(),
                spectrum: self.eq_devices.spectrum_for(effect.id),
                sample_rate: self.state.transport.sample_rate as f32,
            }
            .view(
                self.view_device_preset_bar(
                    track_id,
                    effect,
                    accent,
                    crate::widgets::eq_device::preset_bar_width(size),
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

    // ── Shared device card helpers ──────────────────────────────────

    /// Dark title bar used by all device cards.
    pub(super) fn device_title_bar<'a>(
        content: impl Into<Element<'a, Message>>,
    ) -> iced::widget::Container<'a, Message> {
        crate::widgets::device_chrome::title_bar(content)
    }

    /// Standard device title row: colored dot, name, and the remove button
    /// at the far right, where the effect cards keep theirs.
    pub(super) fn device_title_row(
        name: &str,
        track_color: Color,
        remove: Option<Message>,
    ) -> iced::widget::Row<'_, Message> {
        let dot = text("\u{25CF}").size(8).color(track_color);
        let name = text(name.to_string()).size(11).color(th::text());
        let mut r = row![dot, name]
            .spacing(5)
            .width(Length::Fill)
            .align_y(iced::Alignment::Center);
        if let Some(msg) = remove {
            let remove_btn: Element<'_, Message> =
                Self::device_icon_btn(icons::X, th::text_dim(), th::danger(), msg).into();
            r = r.push(horizontal_space());
            r = r.push(remove_btn);
        }
        r
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
