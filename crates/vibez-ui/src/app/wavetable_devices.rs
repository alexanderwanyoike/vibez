//! Native Wavetable Synth cards on the visible device chain.

use iced::{Color, Element};
use vibez_core::id::TrackId;

use crate::message::Message;
use crate::state::UiTrack;
use crate::widgets::wavetable_device::WavetableView;

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
        self.tabbed_instrument_card(
            "Wavetable Synth",
            track_id,
            track,
            accent,
            width,
            move |device, panel, entry, preset_bar, size| {
                WavetableView {
                    track_id,
                    device,
                    accent,
                    panel,
                    entry,
                }
                .view(preset_bar, size)
            },
        )
    }
}
