//! Native Auto-Pan cards on the visible device chain.

#[cfg(test)]
mod tests {
    use super::super::*;
    use crate::domains::device_card::CardMsg;
    use crate::domains::devices::DevicesMsg;
    use crate::state::{DetailPanelTab, ProjectTrack};
    use vibez_core::effect::EffectType;
    use vibez_core::id::{EffectId, TrackId};
    use vibez_engine::metering::MeterValues;

    fn auto_pan_app() -> (App, TrackId, EffectId) {
        let mut app = super::super::test_support::app();
        let track = ProjectTrack::new(TrackId::new(), "Drums".into(), 1);
        let id = track.id;
        Arc::make_mut(&mut app.state.project_tracks)
            .tracks
            .push(track);
        let _ = app.update(Message::Devices(DevicesMsg::AddEffect(
            id,
            EffectType::AutoPan,
        )));
        let effect = app.state.find_track(id).unwrap().effects[0].id;
        app.state.arrangement.selected_track = Some(id);
        app.state.view.detail_panel_tab = DetailPanelTab::Devices;
        app.poll_device_meters();
        (app, id, effect)
    }

    #[test]
    fn a_visible_auto_pan_receives_its_levels() {
        let (mut app, track, effect) = auto_pan_app();
        let generation = app.device_meters.generation;
        let level = MeterValues {
            peak_l: 0.5,
            peak_r: 0.4,
            rms_l: 0.3,
            rms_r: 0.2,
        };
        app.accept_device_meter(track, effect, generation.wrapping_sub(1), level, level);
        assert_eq!(app.card_panels.panel(effect).input, MeterValues::SILENT);
        app.accept_device_meter(track, effect, generation, level, MeterValues::SILENT);
        assert_eq!(app.card_panels.panel(effect).input, level);
    }

    #[test]
    fn a_removed_auto_pan_forgets_its_panel() {
        let (mut app, track, effect) = auto_pan_app();
        let _ = app.update(Message::Card(CardMsg::ToggleAdvanced(effect)));
        let _ = app.update(Message::Devices(DevicesMsg::RemoveEffect(track, effect)));
        app.poll_device_meters();
        assert!(!app.card_panels.contains(effect));
    }
}
