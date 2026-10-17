//! Native Wavetable Synth cards on the visible device chain.

use iced::widget::{column, text, Space};
use iced::{Color, Element, Length};
use vibez_core::id::TrackId;

use crate::domains::device_params::DeviceParams;
use crate::message::Message;
use crate::state::UiTrack;
use crate::theme as th;

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
        let title = Self::device_title_bar(Self::device_title_row(
            "Wavetable Synth",
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
