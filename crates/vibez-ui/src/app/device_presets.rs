//! Applies device preset requests without replacing the destination instance.

use super::*;
use vibez_core::device_preset::DevicePreset;
use vibez_core::id::{EffectId, TrackId};

use crate::domains::device_presets::{DevicePresetMsg, PresetLoadRequest, PresetSaveRequest};

#[cfg(test)]
#[path = "device_preset_flow_tests.rs"]
mod flow_tests;

/// Preset state for every device: the remembered selection and the file
/// dialogs in flight, whose late completions must not touch a changed device.
#[derive(Default)]
pub(crate) struct DevicePresetRuntime {
    pub(super) load_request: super::tracked_request::TrackedRequest,
    pub(super) save_request: super::tracked_request::TrackedRequest,
    pub(super) load_target: Option<(TrackId, EffectId)>,
    pub(super) selection: std::collections::HashMap<(TrackId, EffectId), DevicePreset>,
}

impl App {
    pub(super) fn update_device_preset(&mut self, message: DevicePresetMsg) -> Task<Message> {
        match message {
            DevicePresetMsg::Load(track_id, effect_id) => {
                let Some(request) = self.begin_device_preset_load(track_id, effect_id) else {
                    return Task::none();
                };
                let task = Task::perform(tasks::load_device_preset_dialog(), move |result| {
                    Message::DevicePreset(DevicePresetMsg::Loaded(request.clone(), result))
                });
                self.device_presets.load_request.attach(task)
            }
            DevicePresetMsg::Save(track_id, effect_id) => {
                let Some(request) = self.begin_device_preset_save(track_id, effect_id) else {
                    return Task::none();
                };
                let task = Task::perform(
                    tasks::save_device_preset_dialog(request.preset.clone()),
                    move |result| {
                        Message::DevicePreset(DevicePresetMsg::Saved(request.clone(), result))
                    },
                );
                self.device_presets.save_request.attach(task)
            }
            DevicePresetMsg::Loaded(request, Ok(None)) => {
                if self.device_presets.load_request.finish(request.token) {
                    self.device_presets.load_target = None;
                }
                Task::none()
            }
            DevicePresetMsg::Loaded(request, Ok(Some(preset))) => {
                self.finish_device_preset_load(request, Ok(preset))
            }
            DevicePresetMsg::Loaded(request, Err(error)) => {
                self.finish_device_preset_load(request, Err(error))
            }
            DevicePresetMsg::Saved(request, result) => {
                self.finish_device_preset_save(request, result);
                Task::none()
            }
        }
    }

    fn capture_device_preset(
        &self,
        track_id: TrackId,
        effect_id: EffectId,
    ) -> Option<DevicePreset> {
        let effect = self
            .state
            .find_track(track_id)?
            .effects
            .iter()
            .find(|effect| effect.id == effect_id)?;
        let name = self
            .device_presets
            .selection
            .get(&(track_id, effect_id))
            .map_or("Custom", DevicePreset::name);
        crate::domains::device_presets::capture(effect, name).ok()
    }

    fn begin_device_preset_load(
        &mut self,
        track_id: TrackId,
        effect_id: EffectId,
    ) -> Option<PresetLoadRequest> {
        let expected = self.capture_device_preset(track_id, effect_id)?;
        self.device_presets.load_target = Some((track_id, effect_id));
        Some(PresetLoadRequest {
            token: self.device_presets.load_request.begin(),
            track_id,
            effect_id,
            expected,
        })
    }

    fn begin_device_preset_save(
        &mut self,
        track_id: TrackId,
        effect_id: EffectId,
    ) -> Option<PresetSaveRequest> {
        let preset = self.capture_device_preset(track_id, effect_id)?;
        Some(PresetSaveRequest {
            token: self.device_presets.save_request.begin(),
            track_id,
            effect_id,
            preset,
        })
    }

    fn finish_device_preset_save(
        &mut self,
        request: PresetSaveRequest,
        result: Result<Option<DevicePreset>, String>,
    ) {
        if !self.device_presets.save_request.finish(request.token) {
            return;
        }
        match result {
            Ok(Some(saved)) => {
                if self
                    .capture_device_preset(request.track_id, request.effect_id)
                    .is_some_and(|current| current == request.preset)
                {
                    self.device_presets
                        .selection
                        .insert((request.track_id, request.effect_id), saved.clone());
                }
                self.state.status_text = format!("Saved preset: {}", saved.name());
            }
            Ok(None) => {}
            Err(error) => self.state.status_text = format!("Preset save failed: {error}"),
        }
    }

    /// Undo and redo change values in place: only a pending load, which would
    /// apply over them, is stale. Remembered presets and saves still hold.
    pub(super) fn cancel_device_preset_load(&mut self) {
        self.device_presets.load_request.cancel();
        self.device_presets.load_target = None;
    }

    pub(super) fn reset_device_presets(&mut self) {
        self.device_presets.load_request.cancel();
        self.device_presets.save_request.cancel();
        self.device_presets.load_target = None;
        self.device_presets.selection.clear();
    }

    pub(super) fn invalidate_device_preset_load(&mut self, message: &Message) {
        use crate::domains::arrangement::ArrangementMsg;
        use crate::domains::devices::DevicesMsg;
        let Some((pending_track, pending_effect)) = self.device_presets.load_target else {
            return;
        };
        let invalidated = match message {
            Message::Devices(
                DevicesMsg::SetEffectParam(track, effect, ..)
                | DevicesMsg::SetEffectParams(track, effect, ..)
                | DevicesMsg::RemoveEffect(track, effect),
            )
            | Message::ApplyDevicePreset(track, effect, _) => {
                (*track, *effect) == (pending_track, pending_effect)
            }
            Message::Arrangement(
                ArrangementMsg::RemoveTrack(track)
                | ArrangementMsg::ConfirmRemoveTrack(track)
                | ArrangementMsg::RemoveBus(track),
            ) => *track == pending_track,
            _ => false,
        };
        if invalidated {
            self.device_presets.load_request.cancel();
            self.device_presets.load_target = None;
            self.state.status_text =
                "Preset load cancelled: the device changed while choosing a file".into();
        }
    }

    pub(super) fn view_device_preset_bar<'a>(
        &'a self,
        track_id: TrackId,
        effect: &'a crate::state::UiEffect,
        accent: iced::Color,
        available_width: f32,
    ) -> iced::Element<'a, Message> {
        crate::widgets::device_preset_bar::view(
            track_id,
            effect,
            self.device_presets.selection.get(&(track_id, effect.id)),
            accent,
            available_width,
        )
    }

    pub(super) fn finish_device_preset_load(
        &mut self,
        request: PresetLoadRequest,
        result: Result<DevicePreset, String>,
    ) -> Task<Message> {
        if !self.device_presets.load_request.finish(request.token) {
            return Task::none();
        }
        self.device_presets.load_target = None;
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
        self.device_presets
            .selection
            .insert((track_id, effect_id), preset);
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
    fn restoring_a_snapshot_cancels_a_pending_preset_load() {
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
        let request = PresetLoadRequest {
            token: app.device_presets.load_request.begin(),
            track_id,
            effect_id: before.id,
            expected: crate::domains::device_presets::capture(&before, "Current").unwrap(),
        };
        let snapshot = app.take_snapshot();

        app.apply_snapshot(snapshot);

        assert!(!app.device_presets.load_request.is_current(request.token));
        let preset = crate::domains::device_presets::factory_presets(EffectType::Gain)[1].clone();
        let _ = app.finish_device_preset_load(request, Ok(preset));
        assert_eq!(
            app.state.find_track(track_id).unwrap().effects[0].params,
            before.params
        );
    }

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
            token: app.device_presets.load_request.begin(),
            track_id,
            effect_id: before.id,
            expected: expected.clone(),
        };
        app.device_presets.load_request.cancel();
        let current = PresetLoadRequest {
            token: app.device_presets.load_request.begin(),
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
