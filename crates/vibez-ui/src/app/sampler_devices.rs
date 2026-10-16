//! Native Sampler cards on the visible device chain.

use iced::widget::{column, mouse_area, text, Space};
use iced::{Color, Element, Length};
use vibez_core::id::TrackId;

use crate::domains::browser::BrowserMsg;
use crate::domains::device_params::DeviceParams;
use crate::message::Message;
use crate::state::UiTrack;
use crate::theme as th;

use super::*;

impl App {
    /// The sampler card at `width`, with its title and the latency line
    /// beneath it.
    pub(super) fn sampler_card<'a>(
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
            crate::widgets::sampler_device::SamplerView {
                track_id,
                device,
                load: Message::LoadSamplerSample(track_id),
                audio: track.sample_audio.as_ref(),
                sample_name: track.sample_name.as_deref(),
                accent,
                panel,
                entry: self.param_entry.as_ref(),
            }
            .view(
                self.view_device_preset_bar(
                    track_id,
                    device,
                    accent,
                    crate::widgets::sampler_device::preset_bar_width(size),
                ),
                size,
            )
        });
        let title = Self::device_title_bar(Self::device_title_row(
            "Sampler",
            accent,
            Some(Message::remove_track_instrument(track_id)),
        ));
        let latency = crate::domains::device_latency::latency_label(
            track.instrument_latency_samples,
            self.state.transport.sample_rate,
        );
        let card = column![
            Self::device_card(column![title, body].width(Length::Fixed(width))),
            text(latency).size(9).color(th::text_dim())
        ]
        .height(Length::Fill);
        // A sample dragged in from the browser loads onto the card.
        mouse_area(card)
            .on_enter(Message::Browser(BrowserMsg::DragHoverSampler { track_id }))
            .on_exit(Message::Browser(BrowserMsg::ClearDragTarget))
            .on_release(Message::DropSampleOnSampler { track_id })
            .into()
    }
}

#[cfg(test)]
mod tests {
    use super::super::*;
    use crate::domains::devices::DevicesMsg;
    use crate::domains::instrument_card::InstrumentCardMsg;
    use crate::domains::sampler::{Mode, SamplerPage};
    use crate::state::{DetailPanelTab, ProjectTrack};
    use vibez_core::id::TrackId;
    use vibez_core::midi::{InstrumentKind, TrackKind};

    fn sampler_app() -> (App, TrackId) {
        let mut app = super::super::test_support::app();
        let mut track = ProjectTrack::new(TrackId::new(), "Keys".into(), 2);
        track.kind = TrackKind::Midi;
        let id = track.id;
        Arc::make_mut(&mut app.state.project_tracks)
            .tracks
            .push(track);
        let _ = app.update(Message::Devices(DevicesMsg::SetTrackInstrument(
            id,
            InstrumentKind::Sampler,
        )));
        app.state.arrangement.selected_track = Some(id);
        app.state.view.detail_panel_tab = DetailPanelTab::Devices;
        app.state.project.history = Default::default();
        (app, id)
    }

    #[test]
    fn a_sampler_card_remembers_its_tab_until_removed() {
        let (mut app, track) = sampler_app();
        let _ = app.update(Message::InstrumentCard(InstrumentCardMsg::ShowPage(
            track,
            SamplerPage::Filter.index(),
        )));
        app.poll_device_meters();
        assert_eq!(
            app.instrument_panels[&track].page,
            SamplerPage::Filter.index()
        );
        let _ = app.update(Message::Devices(DevicesMsg::RemoveTrackInstrument(track)));
        app.poll_device_meters();
        assert!(!app.instrument_panels.contains_key(&track));
    }

    #[test]
    fn choosing_a_mode_is_one_undo_step() {
        let (mut app, track) = sampler_app();
        let _ = app.update(Message::Devices(DevicesMsg::SetInstrumentParams(
            track,
            Mode::Loop.updates(),
        )));
        let params = || {
            app.state
                .find_track(track)
                .unwrap()
                .instrument_params
                .clone()
        };
        assert_eq!(Mode::of(&params()), Mode::Loop);
        let _ = app.update(Message::Project(crate::domains::project::ProjectMsg::Undo));
        assert_eq!(
            Mode::of(&app.state.find_track(track).unwrap().instrument_params),
            Mode::Gate
        );
    }
}
