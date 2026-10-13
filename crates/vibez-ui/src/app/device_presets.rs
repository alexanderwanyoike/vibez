//! Applies device preset requests without replacing the destination instance.

use super::*;
use vibez_core::device_preset::DevicePreset;
use vibez_core::id::{EffectId, TrackId};

#[derive(Debug, Clone)]
pub(crate) struct PresetLoadRequest {
    pub token: u64,
    pub track_id: TrackId,
    pub effect_id: EffectId,
    pub expected: DevicePreset,
}

impl App {
    pub(super) fn finish_device_preset_load(
        &mut self,
        request: PresetLoadRequest,
        result: Result<DevicePreset, String>,
    ) -> Task<Message> {
        if !self.device_preset_load_request.finish(request.token) {
            return Task::none();
        }
        let expected_is_current = self
            .state
            .find_track(request.track_id)
            .and_then(|track| {
                track
                    .effects
                    .iter()
                    .find(|effect| effect.id == request.effect_id)
            })
            .and_then(|effect| {
                crate::domains::device_presets::capture(effect, request.expected.name()).ok()
            })
            .is_some_and(|current| current == request.expected);
        if !expected_is_current {
            return Task::none();
        }
        match result {
            Ok(preset) => self.apply_device_preset(request.track_id, request.effect_id, preset),
            Err(error) => {
                self.state.status_text = format!("Preset load failed: {error}");
                Task::none()
            }
        }
    }

    pub(super) fn apply_device_preset(
        &mut self,
        track_id: TrackId,
        effect_id: EffectId,
        preset: DevicePreset,
    ) -> Task<Message> {
        let Some(effect) = self
            .state
            .find_track(track_id)
            .and_then(|track| track.effects.iter().find(|effect| effect.id == effect_id))
        else {
            return Task::none();
        };
        let updates = match crate::domains::device_presets::prepare_updates(effect, &preset) {
            Ok(updates) => updates,
            Err(error) => {
                self.state.status_text = error;
                return Task::none();
            }
        };
        if updates.is_empty() {
            return Task::none();
        }
        self.update(Message::Devices(
            crate::domains::devices::DevicesMsg::SetEffectParams(track_id, effect_id, updates),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domains::devices::DevicesMsg;
    use crate::state::ProjectTrack;
    use vibez_core::effect::EffectType;

    #[test]
    fn only_the_current_async_load_applies_one_undoable_preset() {
        let mut app = super::super::test_support::app();
        let track = ProjectTrack::new(TrackId::new(), "Bass".into(), 2);
        let track_id = track.id;
        Arc::make_mut(&mut app.state.project_tracks)
            .tracks
            .push(track);
        let _ = app.update(Message::Devices(DevicesMsg::AddEffect(
            track_id,
            EffectType::Gain,
        )));
        let before = app.state.find_track(track_id).unwrap().effects[0].clone();
        app.state.project.history = Default::default();
        app.state.project.dirty = false;
        let expected = crate::domains::device_presets::capture(&before, "Current").unwrap();
        let stale = PresetLoadRequest {
            token: app.device_preset_load_request.begin(),
            track_id,
            effect_id: before.id,
            expected: expected.clone(),
        };
        app.device_preset_load_request.cancel();
        let current = PresetLoadRequest {
            token: app.device_preset_load_request.begin(),
            track_id,
            effect_id: before.id,
            expected,
        };
        let presets = crate::domains::device_presets::factory_presets(EffectType::Gain);

        let _ = app.finish_device_preset_load(stale, Ok(presets[1].clone()));
        assert_eq!(
            app.state.find_track(track_id).unwrap().effects[0].params,
            before.params
        );
        assert!(!app.state.project.dirty);
        assert!(!app.state.project.history.can_undo());

        let _ = app.finish_device_preset_load(current, Ok(presets[2].clone()));
        let changed = &app.state.find_track(track_id).unwrap().effects[0];
        assert!(
            crate::domains::device_presets::prepare_updates(changed, &presets[2])
                .unwrap()
                .is_empty()
        );
        assert!(app.state.project.dirty);
        let snapshot = app.state.project.history.pop_undo().unwrap();
        assert!(!app.state.project.history.can_undo());
        app.apply_snapshot(snapshot);
        assert_eq!(
            app.state.find_track(track_id).unwrap().effects[0].params,
            before.params
        );
    }

    #[test]
    fn loading_gain_preset_is_one_undoable_edit_of_the_existing_instance() {
        let mut app = super::super::test_support::app();
        let track = ProjectTrack::new(TrackId::new(), "Bass".into(), 2);
        let track_id = track.id;
        Arc::make_mut(&mut app.state.project_tracks)
            .tracks
            .push(track);
        let _ = app.update(Message::Devices(DevicesMsg::AddEffect(
            track_id,
            EffectType::Gain,
        )));
        let before = app.state.find_track(track_id).unwrap().effects[0].clone();
        let preset = serde_json::from_value(serde_json::json!({
            "format_version": 1,
            "kind": {"effect": "Gain"},
            "name": "Quiet stereo",
            "parameters": {"gain": 0.5, "left_trim_db": -3.0, "right_trim_db": 2.0,
                "left_polarity": 1.0, "right_polarity": 0.0}
        }))
        .unwrap();
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
