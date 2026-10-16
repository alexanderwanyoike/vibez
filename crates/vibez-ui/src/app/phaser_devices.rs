//! Native Phaser cards on the visible device chain.

#[cfg(test)]
mod tests {
    use super::super::*;
    use crate::domains::device_card::CardMsg;
    use crate::domains::devices::DevicesMsg;
    use crate::state::{DetailPanelTab, ProjectTrack};
    use vibez_core::effect::EffectType;
    use vibez_core::id::{EffectId, TrackId};

    fn phaser_app() -> (App, TrackId, EffectId) {
        let mut app = super::super::test_support::app();
        let track = ProjectTrack::new(TrackId::new(), "Keys".into(), 1);
        let id = track.id;
        Arc::make_mut(&mut app.state.project_tracks)
            .tracks
            .push(track);
        let _ = app.update(Message::Devices(DevicesMsg::AddEffect(
            id,
            EffectType::Phaser,
        )));
        let effect = app.state.find_track(id).unwrap().effects[0].id;
        app.state.arrangement.selected_track = Some(id);
        app.state.view.detail_panel_tab = DetailPanelTab::Devices;
        app.poll_device_meters();
        (app, id, effect)
    }

    #[test]
    fn a_phaser_opens_its_advanced_controls() {
        let (mut app, _, effect) = phaser_app();
        assert!(!app.card_panels.panel(effect).advanced);
        let _ = app.update(Message::Card(CardMsg::ToggleAdvanced(effect)));
        assert!(app.card_panels.panel(effect).advanced);
    }

    #[test]
    fn a_removed_phaser_forgets_its_panel() {
        let (mut app, track, effect) = phaser_app();
        let _ = app.update(Message::Card(CardMsg::ToggleAdvanced(effect)));
        let _ = app.update(Message::Devices(DevicesMsg::RemoveEffect(track, effect)));
        app.poll_device_meters();
        assert!(!app.card_panels.contains(effect));
    }
}
