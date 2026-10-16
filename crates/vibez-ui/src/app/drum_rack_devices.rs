//! Native Drum Rack cards on the visible device chain.

use iced::widget::{column, text, Space};
use iced::{Color, Element, Length};
use vibez_core::id::TrackId;
use vibez_core::track::{drum_rack_pad_pitch, DRUM_RACK_BANK_COUNT, DRUM_RACK_BANK_SIZE};

use crate::domains::device_params::{DeviceParams, DeviceSlot};
use crate::message::Message;
use crate::state::{BrowserDropTarget, UiTrack};
use crate::theme as th;

use super::*;

impl App {
    /// The drum rack card at `width`, with its title and the latency line
    /// beneath it.
    pub(super) fn drum_rack_card<'a>(
        &'a self,
        track_id: TrackId,
        track: &'a UiTrack,
        accent: Color,
        width: f32,
    ) -> Element<'a, Message> {
        let selected = track
            .selected_drum_pad
            .min(track.drum_rack_pads.len().saturating_sub(1));
        let slot = DeviceSlot::DrumPad(selected);
        let Some(device) = DeviceParams::find(track, slot) else {
            return Space::new(0, 0).into();
        };
        let panel = self
            .instrument_panels
            .get(&track_id)
            .cloned()
            .unwrap_or_default();
        let now = std::time::Instant::now();
        let view = &self.state.view;
        let first = selected / DRUM_RACK_BANK_SIZE * DRUM_RACK_BANK_SIZE;
        let firing = std::array::from_fn(|offset| {
            drum_rack_pad_pitch(first + offset)
                .is_some_and(|pitch| view.drum_pad_is_flashing(track_id, pitch, now))
        });
        let banks_firing: [bool; DRUM_RACK_BANK_COUNT] =
            std::array::from_fn(|bank| view.drum_pad_bank_is_flashing(track_id, bank, now));
        let drop_target = match self.state.browser.drag_target {
            Some(BrowserDropTarget::DrumRackPad {
                track_id: target,
                pad_index,
            }) if target == track_id => Some(pad_index),
            _ => None,
        };
        let body = iced::widget::responsive(move |size| {
            let layout = crate::widgets::drum_rack_device::arrange(size);
            crate::widgets::drum_rack_device::DrumRackView {
                track_id,
                pads: &track.drum_rack_pads,
                selected,
                device,
                firing,
                banks_firing,
                drop_target,
                accent,
                panel: panel.clone(),
                entry: self.param_entry.as_ref(),
            }
            .view(
                self.view_device_preset_bar(
                    track_id,
                    device,
                    accent,
                    crate::widgets::sampler_device::preset_bar_width(layout.sampler),
                ),
                size,
            )
        });
        let title = Self::device_title_bar(Self::device_title_row(
            "Drum Rack",
            accent,
            Some(Message::remove_track_instrument(track_id)),
        ));
        let latency = crate::domains::device_latency::latency_label(
            track.instrument_latency_samples,
            self.state.transport.sample_rate,
        );
        column![
            Self::device_card(column![title, body].width(Length::Fixed(width))),
            text(latency).size(9).color(th::text_dim())
        ]
        .height(Length::Fill)
        .into()
    }
}
