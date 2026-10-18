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
#[derive(Clone, Copy, Debug)]
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

/// A built-in instrument whose card takes a flexible share like the
/// effects do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NativeInstrument {
    Synth,
    Sampler,
    DrumRack,
    Wavetable,
    Fm,
}

impl NativeInstrument {
    fn of(track: &UiTrack) -> Option<Self> {
        if crate::domains::synth::is_native_synth(track) {
            Some(Self::Synth)
        } else if crate::domains::sampler::is_native_sampler(track) {
            Some(Self::Sampler)
        } else if crate::domains::sampler::is_native_drum_rack(track) {
            Some(Self::DrumRack)
        } else if crate::domains::wavetable::is_native_wavetable(track) {
            Some(Self::Wavetable)
        } else if crate::domains::fm::is_native_fm(track) {
            Some(Self::Fm)
        } else {
            None
        }
    }

    /// The card's width given a `share` of a chain `available` wide.
    fn card_width(self, share: f32, _available: f32) -> f32 {
        match self {
            Self::Synth => crate::widgets::synth_device::card_width(share),
            Self::Sampler => crate::widgets::sampler_device::card_width(share),
            Self::DrumRack => crate::widgets::drum_rack_device::card_width(share),
            Self::Wavetable => crate::widgets::wavetable_device::card_width(share),
            Self::Fm => crate::widgets::fm_device::card_width(share),
        }
    }
}

impl Flexible {
    /// The card's width given a `share` of a chain `available` wide.
    fn card_width(self, share: f32, available: f32) -> f32 {
        match self {
            Self::Gain => crate::widgets::gain_device::WIDTHS.chain_width(share, available),
            Self::Eq => crate::widgets::eq_device::WIDTHS.chain_width(share, available),
            Self::Compressor => {
                crate::widgets::compressor_device::WIDTHS.chain_width(share, available)
            }
            Self::Limiter => crate::widgets::limiter_device::card_width(share),
            Self::Filter => crate::widgets::filter_device::card_width(share),
            Self::Gate => crate::widgets::gate_device::card_width(share),
            Self::Drive => crate::widgets::drive_device::card_width(share),
            Self::Saturator => crate::widgets::saturator_device::card_width(share),
            Self::Bitcrush => crate::widgets::bitcrush_device::card_width(share),
            Self::Delay => crate::widgets::delay_device::card_width(share),
            Self::Echo => crate::widgets::echo_device::card_width(share),
            Self::Reverb => crate::widgets::reverb_device::card_width(share),
            Self::Phaser => crate::widgets::phaser_device::card_width(share),
            Self::AutoPan => crate::widgets::auto_pan_device::card_width(share),
        }
    }
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

        // The chain keeps the room under its cards itself, for its scrollbar.
        let content = column![header, chain]
            .spacing(6)
            .padding(iced::Padding {
                top: 8.0,
                right: 8.0,
                bottom: 0.0,
                left: 8.0,
            })
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
        let native = NativeInstrument::of(track);
        if native.is_some() {
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
        let shares = flexible.len() + usize::from(native.is_some());
        let spacing = CHAIN_SPACING * cards.len().saturating_sub(1) as f32;
        let mut chain_width = used + spacing;
        if shares > 0 {
            let share = (available_width - used - spacing) / shares as f32;
            if let Some(native) = native {
                let width = native.card_width(share, available_width);
                chain_width += width;
                cards[0] = Some(match native {
                    NativeInstrument::Synth => self.synth_card(track_id, track, track_color, width),
                    NativeInstrument::Sampler => {
                        self.sampler_card(track_id, track, track_color, width)
                    }
                    NativeInstrument::DrumRack => {
                        self.drum_rack_card(track_id, track, track_color, width)
                    }
                    NativeInstrument::Wavetable => {
                        self.wavetable_card(track_id, track, track_color, width)
                    }
                    NativeInstrument::Fm => self.fm_card(track_id, track, track_color, width),
                });
            }
            for (position, effect, sidechain, kind) in flexible {
                let width = kind.card_width(share, available_width);
                chain_width += width;
                let body = match kind {
                    Flexible::Gain => self.gain_device_body(track_id, effect, track_color, width),
                    Flexible::Eq => self.eq_device_body(track_id, effect, track_color, width),
                    Flexible::Compressor => {
                        self.compressor_device_body(track_id, effect, track_color, width)
                    }
                    Flexible::Limiter => {
                        self.limiter_device_body(track_id, effect, track_color, width)
                    }
                    Flexible::Filter => {
                        self.filter_device_body(track_id, effect, track_color, width)
                    }
                    Flexible::Gate => self.gate_device_body(track_id, effect, track_color, width),
                    Flexible::Drive => self.drive_device_body(track_id, effect, track_color, width),
                    Flexible::Saturator => {
                        self.saturator_device_body(track_id, effect, track_color, width)
                    }
                    Flexible::Bitcrush => {
                        self.bitcrush_device_body(track_id, effect, track_color, width)
                    }
                    Flexible::Delay => self.delay_device_body(track_id, effect, track_color, width),
                    Flexible::Echo => self.echo_device_body(track_id, effect, track_color, width),
                    Flexible::Reverb => {
                        self.reverb_device_body(track_id, effect, track_color, width)
                    }
                    Flexible::Phaser => {
                        self.phaser_device_body(track_id, effect, track_color, width)
                    }
                    Flexible::AutoPan => {
                        self.auto_pan_device_body(track_id, effect, track_color, width)
                    }
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

        // Built with its direction: the default vertical check rejects the
        // full-height cards before a later `.direction` could apply.
        use crate::widgets::device_chrome::{
            chain_bar_room, CHAIN_BAR_GAP, CHAIN_BAR_W, CHAIN_PAD_BELOW,
        };
        let overflows = chain_bar_room(chain_width, available_width) > 0.0;
        let bar = scrollable::Scrollbar::new()
            .width(CHAIN_BAR_W)
            .scroller_width(CHAIN_BAR_W);
        let bar = if overflows {
            bar.spacing(CHAIN_BAR_GAP)
        } else {
            bar
        };
        let chain = scrollable::Scrollable::with_direction(
            devices_row,
            scrollable::Direction::Horizontal(bar),
        )
        .width(Length::Fill)
        .height(Length::Fill);
        if overflows {
            chain.into()
        } else {
            column![chain, iced::widget::Space::with_height(CHAIN_PAD_BELOW)].into()
        }
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

#[cfg(test)]
mod width_tests {
    use super::{Flexible, NativeInstrument};
    use crate::widgets::device_chrome::CardWidths;

    const EFFECTS: [(Flexible, CardWidths); 14] = [
        (Flexible::Gain, crate::widgets::gain_device::WIDTHS),
        (Flexible::Eq, crate::widgets::eq_device::WIDTHS),
        (
            Flexible::Compressor,
            crate::widgets::compressor_device::WIDTHS,
        ),
        (Flexible::Limiter, crate::widgets::limiter_device::WIDTHS),
        (Flexible::Filter, crate::widgets::filter_device::WIDTHS),
        (Flexible::Gate, crate::widgets::gate_device::WIDTHS),
        (Flexible::Drive, crate::widgets::drive_device::WIDTHS),
        (
            Flexible::Saturator,
            crate::widgets::saturator_device::WIDTHS,
        ),
        (Flexible::Bitcrush, crate::widgets::bitcrush_device::WIDTHS),
        (Flexible::Delay, crate::widgets::delay_device::WIDTHS),
        (Flexible::Echo, crate::widgets::echo_device::WIDTHS),
        (Flexible::Reverb, crate::widgets::reverb_device::WIDTHS),
        (Flexible::Phaser, crate::widgets::phaser_device::WIDTHS),
        (Flexible::AutoPan, crate::widgets::auto_pan_device::WIDTHS),
    ];

    const INSTRUMENTS: [(NativeInstrument, CardWidths); 5] = [
        (
            NativeInstrument::Synth,
            crate::widgets::synth_device::WIDTHS,
        ),
        (
            NativeInstrument::Sampler,
            crate::widgets::sampler_device::WIDTHS,
        ),
        (
            NativeInstrument::DrumRack,
            crate::widgets::drum_rack_device::WIDTHS,
        ),
        (
            NativeInstrument::Wavetable,
            crate::widgets::instrument_bands::WIDTHS,
        ),
        (
            NativeInstrument::Fm,
            crate::widgets::instrument_bands::WIDTHS,
        ),
    ];

    /// A device's name, its width for a share of a section, and its table.
    type Device = (String, Box<dyn Fn(f32, f32) -> f32>, CardWidths);

    fn every_device() -> Vec<Device> {
        let mut all: Vec<Device> = Vec::new();
        for (kind, widths) in EFFECTS {
            all.push((
                format!("{kind:?}"),
                Box::new(move |s, a| kind.card_width(s, a)),
                widths,
            ));
        }
        for (kind, widths) in INSTRUMENTS {
            all.push((
                format!("{kind:?}"),
                Box::new(move |s, a| kind.card_width(s, a)),
                widths,
            ));
        }
        all
    }

    #[test]
    fn a_crowded_chain_keeps_every_device_expanded_and_scrolls() {
        for (name, width, widths) in every_device() {
            assert!(
                width(100.0, 1_900.0) >= widths.expanded,
                "{name}: {} under {}",
                width(100.0, 1_900.0),
                widths.expanded
            );
        }
    }

    #[test]
    fn a_roomy_chain_still_lets_every_device_grow_to_its_maximum() {
        for (name, width, widths) in every_device() {
            assert_eq!(width(5_000.0, 5_000.0), widths.max, "{name}");
        }
    }
}
