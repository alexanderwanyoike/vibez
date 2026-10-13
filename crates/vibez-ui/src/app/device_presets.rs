//! Applies device preset requests without replacing the destination instance.

use super::*;
use vibez_core::device_preset::DevicePreset;
use vibez_core::id::{EffectId, TrackId};

impl App {
    pub(super) fn apply_device_preset(
        &mut self,
        _track_id: TrackId,
        _effect_id: EffectId,
        _preset: DevicePreset,
    ) -> Task<Message> {
        Task::none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domains::devices::DevicesMsg;
    use crate::state::ProjectTrack;
    use vibez_core::effect::EffectType;

    #[test]
    fn loading_gain_preset_is_one_undoable_edit_of_the_existing_instance() {
        let mut app = super::super::test_support::app();
        let track = ProjectTrack::new(TrackId::new(), "Bass".into(), 2);
        let track_id = track.id;
        Arc::make_mut(&mut app.state.project_tracks).tracks.push(track);
        let _ = app.update(Message::Devices(DevicesMsg::AddEffect(track_id, EffectType::Gain)));
        let before = app.state.find_track(track_id).unwrap().effects[0].clone();
        let preset = serde_json::from_value(serde_json::json!({
            "format_version": 1,
            "kind": {"effect": "Gain"},
            "name": "Quiet stereo",
            "parameters": {"gain": 0.5, "left_trim_db": -3.0, "right_trim_db": 2.0,
                "left_polarity": 1.0, "right_polarity": 0.0}
        })).unwrap();
        let _ = app.update(Message::ApplyDevicePreset(track_id, before.id, preset));

        let after = &app.state.find_track(track_id).unwrap().effects[0];
        assert_eq!(after.params, vec![0.5, -3.0, 2.0, 1.0, 0.0]);
        assert_eq!(after.id, before.id);
        assert_eq!(after.bypass, before.bypass);
        assert_eq!(after.sidechains, before.sidechains);
        let snapshot = app.state.project.history.pop_undo().unwrap();
        app.apply_snapshot(snapshot);
        let restored = &app.state.find_track(track_id).unwrap().effects[0];
        assert_eq!(restored.id, before.id);
        assert_eq!(restored.params, before.params);
    }
}
