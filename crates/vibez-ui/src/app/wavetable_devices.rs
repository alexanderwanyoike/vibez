//! Native Wavetable Synth cards on the visible device chain.

use iced::widget::Space;
use iced::{Color, Element};
use vibez_core::id::TrackId;

use crate::domains::device_params::DeviceParams;
use crate::message::Message;
use crate::state::UiTrack;

use super::*;

impl App {
    /// The Wavetable Synth card at `width`, with its title and the latency
    /// line beneath it.
    pub(super) fn wavetable_card<'a>(
        &'a self,
        track_id: TrackId,
        track: &'a UiTrack,
        accent: Color,
        width: f32,
    ) -> Element<'a, Message> {
        let Some(device) = DeviceParams::instrument(track) else {
            return Space::new(0, 0).into();
        };
        let panel = self
            .instrument_panels
            .get(&track_id)
            .cloned()
            .unwrap_or_default();
        let body = iced::widget::responsive(move |size| {
            crate::widgets::wavetable_device::WavetableView {
                track_id,
                device,
                accent,
                panel: panel.clone(),
                entry: self.param_entry.as_ref(),
            }
            .view(
                self.view_device_preset_bar(
                    track_id,
                    device,
                    accent,
                    crate::widgets::wavetable_device::preset_bar_width(size),
                ),
                size,
            )
        });
        self.native_instrument_frame(
            "Wavetable Synth",
            track_id,
            track,
            accent,
            width,
            body.into(),
        )
        .into()
    }
}
