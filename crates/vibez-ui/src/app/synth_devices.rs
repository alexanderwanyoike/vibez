//! Native Subtractive Synth cards on the visible device chain.

use iced::widget::{column, text, Space};
use iced::{Color, Element, Length};
use vibez_core::id::TrackId;

use crate::domains::device_params::DeviceParams;
use crate::message::Message;
use crate::state::UiTrack;
use crate::theme as th;

use super::*;

impl App {
    /// The synth card at `width`, with its title and the latency line
    /// beneath it.
    pub(super) fn synth_card<'a>(
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
            .copied()
            .unwrap_or_default();
        let body = iced::widget::responsive(move |size| {
            crate::widgets::synth_device::SynthView {
                track_id,
                device,
                accent,
                panel,
                entry: self.param_entry.as_ref(),
            }
            .view(
                self.view_device_preset_bar(
                    track_id,
                    device,
                    accent,
                    crate::widgets::synth_device::preset_bar_width(size),
                ),
                size,
            )
        });
        let title = Self::device_title_bar(Self::device_title_row(
            "Subtractive Synth",
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

#[cfg(test)]
mod tests {
    use super::super::*;
    use crate::domains::devices::DevicesMsg;
    use crate::domains::instrument_card::InstrumentCardMsg;
    use crate::domains::synth::SynthPage;
    use crate::state::{DetailPanelTab, ProjectTrack};
    use vibez_core::id::TrackId;
    use vibez_core::midi::{InstrumentKind, TrackKind};

    fn synth_app() -> (App, TrackId) {
        let mut app = super::super::test_support::app();
        let mut track = ProjectTrack::new(TrackId::new(), "Keys".into(), 2);
        track.kind = TrackKind::Midi;
        let id = track.id;
        Arc::make_mut(&mut app.state.project_tracks)
            .tracks
            .push(track);
        let _ = app.update(Message::Devices(DevicesMsg::SetTrackInstrument(
            id,
            InstrumentKind::SubtractiveSynth,
        )));
        app.state.arrangement.selected_track = Some(id);
        app.state.view.detail_panel_tab = DetailPanelTab::Devices;
        (app, id)
    }

    #[test]
    fn a_synth_card_remembers_its_tab() {
        let (mut app, track) = synth_app();
        let _ = app.update(Message::InstrumentCard(InstrumentCardMsg::ShowPage(
            track,
            SynthPage::Filter.index(),
        )));
        app.poll_device_meters();
        assert_eq!(
            app.instrument_panels[&track].page,
            SynthPage::Filter.index()
        );
    }

    #[test]
    fn a_removed_synth_forgets_its_panel() {
        let (mut app, track) = synth_app();
        let _ = app.update(Message::InstrumentCard(InstrumentCardMsg::ShowPage(
            track,
            SynthPage::Filter.index(),
        )));
        let _ = app.update(Message::Devices(DevicesMsg::RemoveTrackInstrument(track)));
        app.poll_device_meters();
        assert!(!app.instrument_panels.contains_key(&track));
    }
}
