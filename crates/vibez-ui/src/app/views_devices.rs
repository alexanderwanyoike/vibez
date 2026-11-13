//! Split out of app.rs; inherent methods on [`super::App`].

use iced::widget::{column, container, mouse_area, row, scrollable, text};
use iced::{Color, Element, Length};

use vibez_core::id::TrackId;

use crate::message::Message;
use crate::state::UiTrack;
use crate::theme as th;
use crate::widgets::effect_slot::{view_effect_slot, view_sidechain, DeviceBody};

use super::*;

const CHAIN_SPACING: f32 = 6.0;
const PLACEHOLDER_W: f32 = 150.0;

/// Room kept after the last card, so a full chain still has somewhere to
/// right-click for the add menu.
const CHAIN_END_ROOM: f32 = 140.0;

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
    fn card_width(self, share: f32, available: f32) -> f32 {
        use crate::widgets::*;
        match self {
            Self::Synth => synth_device::WIDTHS,
            Self::Sampler => sampler_device::WIDTHS,
            Self::DrumRack => drum_rack_device::WIDTHS,
            Self::Wavetable | Self::Fm => instrument_bands::WIDTHS,
        }
        .chain_width(share, available)
    }
}

impl Flexible {
    /// The card's width given a `share` of a chain `available` wide.
    fn card_width(self, share: f32, available: f32) -> f32 {
        use crate::widgets::*;
        match self {
            Self::Gain => gain_device::WIDTHS,
            Self::Eq => eq_device::WIDTHS,
            Self::Compressor => compressor_device::WIDTHS,
            Self::Limiter => limiter_device::WIDTHS,
            Self::Filter => filter_device::WIDTHS,
            Self::Gate => gate_device::WIDTHS,
            Self::Drive => drive_device::WIDTHS,
            Self::Saturator => saturator_device::WIDTHS,
            Self::Bitcrush => bitcrush_device::WIDTHS,
            Self::Delay => delay_device::WIDTHS,
            Self::Echo => echo_device::WIDTHS,
            Self::Reverb => reverb_device::WIDTHS,
            Self::Phaser => phaser_device::WIDTHS,
            Self::AutoPan => auto_pan_device::WIDTHS,
        }
        .chain_width(share, available)
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
        // The chain keeps the room under its cards itself, for its scrollbar.
        let content = iced::widget::responsive(move |size| {
            self.device_chain_row(track_id, track, track_color, size.width)
        });

        let highlighted = matches!(
            &self.state.browser.drag_target,
            Some(crate::state::BrowserDropTarget::DevicePanel { track_id: target, .. })
                if *target == track_id
        );
        use crate::widgets::device_chrome::{CARD_RADIUS, CHAIN_PAD_BELOW};
        use crate::widgets::drop_highlight::{drop_highlight, CARD_ZONE_INSET};
        let content = drop_highlight(
            content.into(),
            highlighted
                .then(|| self.state.browser.drop_zone())
                .flatten(),
            CARD_RADIUS - CARD_ZONE_INSET,
            iced::Padding::new(CARD_ZONE_INSET).bottom(CHAIN_PAD_BELOW + CARD_ZONE_INSET),
        );
        // Wrap in mouse_area for right-click context menu
        let panel = mouse_area(content).on_right_press(Message::Devices(
            crate::domains::devices::DevicesMsg::ShowContextMenu {
                x: self.state.view.cursor_x,
                y: self.state.view.cursor_y,
                track_id,
            },
        ));
        // Only while dragging, so an ordinary click's release still reaches
        // whatever the shell does with it.
        let drop =
            self.state.browser.drag_source.as_ref().and_then(|_| {
                crate::domains::device_panel_drop::DevicePanelDrop::for_channel(track)
            });
        let Some(drop) = drop else {
            return panel.into();
        };
        let left = Message::Browser(BrowserMsg::LeaveDragTarget(
            crate::state::BrowserDropTarget::DevicePanel {
                track_id,
                drop: drop.clone(),
            },
        ));
        panel
            .on_move(move |_| {
                Message::Browser(BrowserMsg::DragHoverDevicePanel {
                    track_id,
                    drop: drop.clone(),
                })
            })
            .on_exit(left)
            .on_release(Message::DropSampleOnDevicePanel { track_id })
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
        let spacing = CHAIN_SPACING * cards.len() as f32;
        let mut chain_width = used + spacing + CHAIN_END_ROOM;
        if shares > 0 {
            let share = flexible_share(available_width, used, cards.len(), shares);
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
            .push(
                container(
                    text(add_hint(track.kind.is_midi()))
                        .size(10)
                        .color(th::text_muted())
                        .align_x(iced::alignment::Horizontal::Center)
                        .width(Length::Fill),
                )
                .padding([0, 8])
                .width(CHAIN_END_ROOM)
                .center_y(Length::Fill),
            )
            .spacing(CHAIN_SPACING)
            .height(Length::Fill);

        use crate::widgets::device_chrome::{chain_bar_room, CHAIN_BAR_GAP, CHAIN_PAD_BELOW};
        use crate::widgets::quiet_scroll;
        let overflows = chain_bar_room(chain_width, available_width) > 0.0;
        let bar = if overflows {
            quiet_scroll::scrollbar().spacing(CHAIN_BAR_GAP)
        } else {
            quiet_scroll::scrollbar()
        };
        let chain = quiet_scroll::scroll(devices_row, scrollable::Direction::Horizontal(bar))
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
        self.native_device_body(
            track_id,
            effect,
            accent,
            width,
            crate::widgets::gain_device::advanced(effect, panel.advanced, accent),
            move |size| {
                crate::widgets::gain_device::view(
                    track_id,
                    effect,
                    accent,
                    panel,
                    self.param_entry.as_ref(),
                    size,
                )
            },
        )
    }

    /// A native effect's body, its preset controls and its Advanced switch
    /// for the card's header.
    fn native_device_body<'a>(
        &'a self,
        track_id: TrackId,
        effect: &'a crate::state::UiEffect,
        accent: Color,
        width: f32,
        advanced: crate::widgets::device_chrome::Action,
        body: impl Fn(iced::Size) -> Element<'a, Message> + 'a,
    ) -> DeviceBody<'a> {
        DeviceBody {
            content: iced::widget::responsive(body).into(),
            width,
            padding: iced::Padding::ZERO,
            preset: Some(Box::new(move |selector_w| {
                self.view_device_preset_bar(track_id, effect, accent, selector_w)
            })),
            advanced: Some(advanced),
        }
    }

    fn compressor_device_body<'a>(
        &'a self,
        track_id: TrackId,
        effect: &'a crate::state::UiEffect,
        accent: Color,
        width: f32,
    ) -> DeviceBody<'a> {
        let view = move || crate::widgets::compressor_device::CompressorView {
            track_id,
            effect,
            accent,
            panel: self.compressor_devices.panel(effect.id),
            entry: self.param_entry.as_ref(),
        };
        self.native_device_body(
            track_id,
            effect,
            accent,
            width,
            view().advanced_switch(),
            move |size| view().view(size),
        )
    }

    fn reverb_device_body<'a>(
        &'a self,
        track_id: TrackId,
        effect: &'a crate::state::UiEffect,
        accent: Color,
        width: f32,
    ) -> DeviceBody<'a> {
        let view = move || crate::widgets::reverb_device::ReverbView {
            track_id,
            effect,
            accent,
            panel: self.card_panels.panel(effect.id),
            entry: self.param_entry.as_ref(),
            bpm: self.state.transport.bpm,
        };
        self.native_device_body(
            track_id,
            effect,
            accent,
            width,
            view().advanced_switch(),
            move |size| view().view(size),
        )
    }

    fn phaser_device_body<'a>(
        &'a self,
        track_id: TrackId,
        effect: &'a crate::state::UiEffect,
        accent: Color,
        width: f32,
    ) -> DeviceBody<'a> {
        let view = move || crate::widgets::phaser_device::PhaserView {
            track_id,
            effect,
            accent,
            panel: self.card_panels.panel(effect.id),
            entry: self.param_entry.as_ref(),
            bpm: self.state.transport.bpm,
        };
        self.native_device_body(
            track_id,
            effect,
            accent,
            width,
            view().advanced_switch(),
            move |size| view().view(size),
        )
    }

    fn auto_pan_device_body<'a>(
        &'a self,
        track_id: TrackId,
        effect: &'a crate::state::UiEffect,
        accent: Color,
        width: f32,
    ) -> DeviceBody<'a> {
        let view = move || crate::widgets::auto_pan_device::AutoPanView {
            track_id,
            effect,
            accent,
            panel: self.card_panels.panel(effect.id),
            entry: self.param_entry.as_ref(),
            bpm: self.state.transport.bpm,
        };
        self.native_device_body(
            track_id,
            effect,
            accent,
            width,
            view().advanced_switch(),
            move |size| view().view(size),
        )
    }

    fn delay_device_body<'a>(
        &'a self,
        track_id: TrackId,
        effect: &'a crate::state::UiEffect,
        accent: Color,
        width: f32,
    ) -> DeviceBody<'a> {
        let view = move || crate::widgets::delay_device::DelayView {
            track_id,
            effect,
            accent,
            panel: self.card_panels.panel(effect.id),
            entry: self.param_entry.as_ref(),
            bpm: self.state.transport.bpm,
        };
        self.native_device_body(
            track_id,
            effect,
            accent,
            width,
            view().advanced_switch(),
            move |size| view().view(size),
        )
    }

    fn limiter_device_body<'a>(
        &'a self,
        track_id: TrackId,
        effect: &'a crate::state::UiEffect,
        accent: Color,
        width: f32,
    ) -> DeviceBody<'a> {
        let now = std::time::Instant::now();
        let view = move || crate::widgets::limiter_device::LimiterView {
            track_id,
            effect,
            accent,
            panel: self.card_panels.panel(effect.id),
            entry: self.param_entry.as_ref(),
            now,
        };
        self.native_device_body(
            track_id,
            effect,
            accent,
            width,
            view().advanced_switch(),
            move |size| view().view(size),
        )
    }

    fn echo_device_body<'a>(
        &'a self,
        track_id: TrackId,
        effect: &'a crate::state::UiEffect,
        accent: Color,
        width: f32,
    ) -> DeviceBody<'a> {
        let view = move || crate::widgets::echo_device::EchoView {
            track_id,
            effect,
            accent,
            panel: self.card_panels.panel(effect.id),
            entry: self.param_entry.as_ref(),
            bpm: self.state.transport.bpm,
        };
        self.native_device_body(
            track_id,
            effect,
            accent,
            width,
            view().advanced_switch(),
            move |size| view().view(size),
        )
    }

    fn saturator_device_body<'a>(
        &'a self,
        track_id: TrackId,
        effect: &'a crate::state::UiEffect,
        accent: Color,
        width: f32,
    ) -> DeviceBody<'a> {
        let view = move || crate::widgets::saturator_device::SaturatorView {
            track_id,
            effect,
            accent,
            panel: self.card_panels.panel(effect.id),
            entry: self.param_entry.as_ref(),
        };
        self.native_device_body(
            track_id,
            effect,
            accent,
            width,
            view().advanced_switch(),
            move |size| view().view(size),
        )
    }

    fn bitcrush_device_body<'a>(
        &'a self,
        track_id: TrackId,
        effect: &'a crate::state::UiEffect,
        accent: Color,
        width: f32,
    ) -> DeviceBody<'a> {
        let view = move || crate::widgets::bitcrush_device::BitcrushView {
            track_id,
            effect,
            accent,
            panel: self.card_panels.panel(effect.id),
            entry: self.param_entry.as_ref(),
            host_rate: self.state.transport.sample_rate,
        };
        self.native_device_body(
            track_id,
            effect,
            accent,
            width,
            view().advanced_switch(),
            move |size| view().view(size),
        )
    }

    fn drive_device_body<'a>(
        &'a self,
        track_id: TrackId,
        effect: &'a crate::state::UiEffect,
        accent: Color,
        width: f32,
    ) -> DeviceBody<'a> {
        let view = move || crate::widgets::drive_device::DriveView {
            track_id,
            effect,
            accent,
            panel: self.card_panels.panel(effect.id),
            entry: self.param_entry.as_ref(),
        };
        self.native_device_body(
            track_id,
            effect,
            accent,
            width,
            view().advanced_switch(),
            move |size| view().view(size),
        )
    }

    fn gate_device_body<'a>(
        &'a self,
        track_id: TrackId,
        effect: &'a crate::state::UiEffect,
        accent: Color,
        width: f32,
    ) -> DeviceBody<'a> {
        let view = move || crate::widgets::gate_device::GateView {
            track_id,
            effect,
            accent,
            panel: self.gate_panels.panel(effect.id),
            entry: self.param_entry.as_ref(),
        };
        self.native_device_body(
            track_id,
            effect,
            accent,
            width,
            view().advanced_switch(),
            move |size| view().view(size),
        )
    }

    fn filter_device_body<'a>(
        &'a self,
        track_id: TrackId,
        effect: &'a crate::state::UiEffect,
        accent: Color,
        width: f32,
    ) -> DeviceBody<'a> {
        let view = move || crate::widgets::filter_device::FilterView {
            track_id,
            effect,
            accent,
            panel: self.filter_panels.panel(effect.id),
            entry: self.param_entry.as_ref(),
            sample_rate: self.state.transport.sample_rate as f32,
        };
        self.native_device_body(
            track_id,
            effect,
            accent,
            width,
            view().advanced_switch(),
            move |size| view().view(size),
        )
    }

    fn eq_device_body<'a>(
        &'a self,
        track_id: TrackId,
        effect: &'a crate::state::UiEffect,
        accent: Color,
        width: f32,
    ) -> DeviceBody<'a> {
        let view = move || crate::widgets::eq_device::EqView {
            track_id,
            effect,
            accent,
            panel: self.eq_devices.panel(effect.id),
            entry: self.param_entry.as_ref(),
            spectrum: self.eq_devices.spectrum_for(effect.id),
            sample_rate: self.state.transport.sample_rate as f32,
        };
        self.native_device_body(
            track_id,
            effect,
            accent,
            width,
            view().advanced_switch(),
            move |size| view().view(size),
        )
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
        let header = crate::widgets::device_chrome::Header::new(
            "No Instrument",
            th::text_muted(),
            "Right-click the device panel to add an instrument".into(),
        );
        let body = container(text("Right-click to add").size(9).color(th::text_muted()))
            .padding([8, 12])
            .width(Length::Fill);
        crate::widgets::device_chrome::shell(header, body, PLACEHOLDER_W)
    }
}

/// How to add a device to a channel. Instruments are only offered where
/// the add menu has its Instruments tab.
fn add_hint(is_midi: bool) -> &'static str {
    if is_midi {
        "Right-click to add an instrument or effect"
    } else {
        "Right-click to add an effect"
    }
}

/// The width each of `shares` flexible cards gets in a section `available`
/// wide, beside `cards` cards in all whose fixed ones take `used`, and the
/// chain's end room.
fn flexible_share(available: f32, used: f32, cards: usize, shares: usize) -> f32 {
    let spacing = CHAIN_SPACING * cards as f32;
    (available - used - spacing - CHAIN_END_ROOM) / shares as f32
}

#[cfg(test)]
mod width_tests {
    use super::{
        add_hint, flexible_share, Flexible, NativeInstrument, CHAIN_END_ROOM, CHAIN_SPACING,
    };
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
    fn every_device_header_fits_its_narrowest_card() {
        use crate::widgets::device_chrome::{header_min_w, MOST_ACTIONS};
        for (name, _, widths) in every_device() {
            assert!(
                widths.compact >= header_min_w(MOST_ACTIONS),
                "{name}: {} under {}",
                widths.compact,
                header_min_w(MOST_ACTIONS)
            );
        }
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
    fn a_full_chain_leaves_room_after_its_last_card_to_right_click() {
        let fixed = 200.0;
        let share = flexible_share(1_000.0, fixed, 3, 2);
        let cards_end = fixed + 2.0 * share + 2.0 * CHAIN_SPACING;
        assert_eq!(1_000.0 - cards_end, CHAIN_SPACING + CHAIN_END_ROOM);
    }

    #[test]
    fn the_end_hint_offers_instruments_only_where_the_add_menu_does() {
        assert_eq!(add_hint(true), "Right-click to add an instrument or effect");
        assert_eq!(add_hint(false), "Right-click to add an effect");
    }

    #[test]
    fn a_roomy_chain_still_lets_every_device_grow_to_its_maximum() {
        for (name, width, widths) in every_device() {
            assert_eq!(width(5_000.0, 5_000.0), widths.max, "{name}");
        }
    }
}
