//! Native Subtractive Synth cards on the visible device chain.

use iced::widget::Space;
use iced::{Color, Element};
use vibez_core::id::TrackId;

use crate::domains::device_params::DeviceParams;
use crate::message::Message;
use crate::state::UiTrack;

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
            .cloned()
            .unwrap_or_default();
        let body = iced::widget::responsive(move |size| {
            crate::widgets::synth_device::SynthView {
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
                    crate::widgets::synth_device::preset_bar_width(size),
                ),
                size,
            )
        });
        self.native_instrument_frame(
            "Subtractive Synth",
            track_id,
            track,
            accent,
            width,
            body.into(),
        )
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
