//! Native Filter cards on the visible device chain and their panel state.

use crate::domains::filter::FilterMsg;

use super::*;

impl App {
    pub(super) fn update_filter(&mut self, message: FilterMsg) -> Task<Message> {
        self.filter_panels
            .entry(message.effect_id())
            .update(message);
        Task::none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domains::devices::DevicesMsg;
    use crate::state::{DetailPanelTab, ProjectTrack};
    use vibez_core::effect::EffectType;
    use vibez_core::id::{EffectId, TrackId};
    use vibez_engine::metering::MeterValues;

    fn filter_app() -> (App, TrackId, EffectId) {
        let mut app = super::super::test_support::app();
        let track = ProjectTrack::new(TrackId::new(), "Pads".into(), 1);
        let id = track.id;
        Arc::make_mut(&mut app.state.project_tracks)
            .tracks
            .push(track);
        let _ = app.update(Message::Devices(DevicesMsg::AddEffect(
            id,
            EffectType::Filter,
        )));
        let effect = app.state.find_track(id).unwrap().effects[0].id;
        app.state.arrangement.selected_track = Some(id);
        app.state.view.detail_panel_tab = DetailPanelTab::Devices;
        app.poll_device_meters();
        (app, id, effect)
    }

    #[test]
    fn a_visible_filter_receives_its_output_level() {
        let (mut app, track, effect) = filter_app();
        let generation = app.device_meters.generation;
        let level = MeterValues {
            peak_l: 0.5,
            peak_r: 0.4,
            rms_l: 0.3,
            rms_r: 0.2,
        };
        app.accept_device_meter(track, effect, generation.wrapping_sub(1), level, level);
        assert_eq!(app.filter_panels.panel(effect).output, MeterValues::SILENT);
        app.accept_device_meter(track, effect, generation, MeterValues::SILENT, level);
        assert_eq!(app.filter_panels.panel(effect).output, level);
    }

    #[test]
    fn a_removed_filter_forgets_its_panel() {
        let (mut app, track, effect) = filter_app();
        let _ = app.update(Message::Filter(FilterMsg::ToggleAdvanced(effect)));
        assert!(app.filter_panels.panel(effect).advanced);
        let _ = app.update(Message::Devices(DevicesMsg::RemoveEffect(track, effect)));
        app.poll_device_meters();
        assert!(!app.filter_panels.contains(effect));
    }
}
