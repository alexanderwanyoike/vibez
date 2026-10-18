//! Verifies preset dialog completion against project edits and cancellation.

use super::*;
use crate::domains::devices::DevicesMsg;
use crate::state::ProjectTrack;
use vibez_core::effect::EffectType;
use vibez_core::id::EffectId;

fn gain_app() -> (App, TrackId, EffectId) {
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
    let effect_id = app.state.find_track(track_id).unwrap().effects[0].id;
    app.state.project.history = Default::default();
    app.state.project.dirty = false;
    (app, track_id, effect_id)
}

#[test]
fn cancelled_and_unchanged_loads_do_not_create_project_edits() {
    let (mut app, track_id, effect_id) = gain_app();
    let original = app.capture_device_preset(track_id, effect_id).unwrap();
    let cancelled = app.begin_device_preset_load(track_id, effect_id).unwrap();

    let _ = app.update(Message::DevicePreset(DevicePresetMsg::Loaded(
        cancelled,
        Ok(None),
    )));

    assert!(app.device_presets.load_target.is_none());
    assert!(!app.state.project.dirty);
    assert!(!app.state.project.history.can_undo());

    let request = app.begin_device_preset_load(track_id, effect_id).unwrap();
    let unchanged =
        DevicePreset::new(original.kind(), "My unity", original.parameters().clone()).unwrap();
    let _ = app.update(Message::DevicePreset(DevicePresetMsg::Loaded(
        request,
        Ok(Some(unchanged.clone())),
    )));

    assert_eq!(
        app.capture_device_preset(track_id, effect_id)
            .unwrap()
            .parameters(),
        original.parameters()
    );
    assert_eq!(
        app.device_presets.selection[&(track_id, effect_id.into())],
        unchanged
    );
    assert!(!app.state.project.dirty);
    assert!(!app.state.project.history.can_undo());
}

#[test]
fn editing_away_and_back_still_cancels_a_pending_load() {
    let (mut app, track_id, effect_id) = gain_app();
    let request = app.begin_device_preset_load(track_id, effect_id).unwrap();
    for gain in [0.5, 1.0] {
        let _ = app.update(Message::Devices(DevicesMsg::SetEffectParam(
            track_id, effect_id, 0, gain,
        )));
    }
    app.state.project.history = Default::default();
    app.state.project.dirty = false;
    let preset = crate::domains::device_presets::factory_presets(EffectType::Gain)[1].clone();

    let _ = app.update(Message::DevicePreset(DevicePresetMsg::Loaded(
        request.clone(),
        Ok(Some(preset)),
    )));

    assert!(!app.device_presets.load_request.is_current(request.token));
    assert_eq!(
        app.capture_device_preset(track_id, effect_id)
            .unwrap()
            .parameters(),
        request.expected.parameters()
    );
    assert!(!app.state.project.dirty);
    assert!(!app.state.project.history.can_undo());
}

#[test]
fn unreadable_and_incompatible_presets_preserve_the_current_device() {
    let (mut app, track_id, effect_id) = gain_app();
    let original = app.capture_device_preset(track_id, effect_id).unwrap();
    let incompatible = DevicePreset::new(
        vibez_core::device_preset::DevicePresetKind::Effect(EffectType::Reverb),
        "Wrong device",
        original.parameters().clone(),
    )
    .unwrap();
    for result in [Err("Invalid preset JSON".into()), Ok(Some(incompatible))] {
        let request = app.begin_device_preset_load(track_id, effect_id).unwrap();
        let _ = app.update(Message::DevicePreset(DevicePresetMsg::Loaded(
            request, result,
        )));

        assert_eq!(
            app.capture_device_preset(track_id, effect_id).unwrap(),
            original
        );
        assert!(!app.state.status_text.is_empty());
        assert!(!app.state.project.dirty);
        assert!(!app.state.project.history.can_undo());
    }
}

#[test]
fn save_captures_before_the_dialog_and_never_reapplies_its_values() {
    let (mut app, track_id, effect_id) = gain_app();
    let request = app.begin_device_preset_save(track_id, effect_id).unwrap();
    let _ = app.update(Message::Devices(DevicesMsg::SetEffectParam(
        track_id, effect_id, 0, 0.25,
    )));
    let current = app.capture_device_preset(track_id, effect_id).unwrap();
    app.state.project.history = Default::default();
    app.state.project.dirty = false;
    let saved = DevicePreset::new(
        request.preset.kind(),
        "Saved before edit",
        request.preset.parameters().clone(),
    )
    .unwrap();

    let _ = app.update(Message::DevicePreset(DevicePresetMsg::Saved(
        request,
        Ok(Some(saved.clone())),
    )));

    assert_eq!(saved.parameters()["gain"], 1.0);
    assert_eq!(
        app.capture_device_preset(track_id, effect_id)
            .unwrap()
            .parameters(),
        current.parameters()
    );
    assert!(!app.state.project.dirty);
    assert!(!app.state.project.history.can_undo());
}

#[test]
fn a_late_save_does_not_replace_a_newly_selected_preset_name() {
    let (mut app, track_id, effect_id) = gain_app();
    let request = app.begin_device_preset_save(track_id, effect_id).unwrap();
    let selected = crate::domains::device_presets::factory_presets(EffectType::Gain)[1].clone();
    let _ = app.update(Message::ApplyDevicePreset(
        track_id,
        effect_id.into(),
        selected.clone(),
    ));
    app.state.project.history = Default::default();
    app.state.project.dirty = false;
    let saved = DevicePreset::new(
        request.preset.kind(),
        "Saved unity",
        request.preset.parameters().clone(),
    )
    .unwrap();

    let _ = app.update(Message::DevicePreset(DevicePresetMsg::Saved(
        request,
        Ok(Some(saved)),
    )));

    assert_eq!(
        app.device_presets.selection[&(track_id, effect_id.into())],
        selected
    );
    assert_eq!(
        app.capture_device_preset(track_id, effect_id)
            .unwrap()
            .parameters(),
        selected.parameters()
    );
    assert!(!app.state.project.dirty);
    assert!(!app.state.project.history.can_undo());
}

#[test]
fn project_reset_ignores_a_late_save_completion_even_when_ids_remain() {
    let (mut app, track_id, effect_id) = gain_app();
    let request = app.begin_device_preset_save(track_id, effect_id).unwrap();
    let saved = request.preset.clone();
    app.clear_project_runtime();
    let status = app.state.status_text.clone();

    let _ = app.update(Message::DevicePreset(DevicePresetMsg::Saved(
        request,
        Ok(Some(saved)),
    )));

    assert!(app.device_presets.selection.is_empty());
    assert_eq!(app.state.status_text, status);
    assert!(!app.state.project.dirty);
    assert!(!app.state.project.history.can_undo());
}

#[test]
fn undo_keeps_the_remembered_preset_and_lets_a_save_finish() {
    let (mut app, track_id, effect_id) = gain_app();
    let selected = crate::domains::device_presets::factory_presets(EffectType::Gain)[1].clone();
    let _ = app.update(Message::ApplyDevicePreset(
        track_id,
        effect_id.into(),
        selected.clone(),
    ));
    let request = app.begin_device_preset_save(track_id, effect_id).unwrap();
    let _ = app.update(Message::Devices(DevicesMsg::SetEffectParam(
        track_id, effect_id, 1, 3.0,
    )));
    let _ = app.update(Message::Project(crate::domains::project::ProjectMsg::Undo));
    assert_eq!(
        app.device_presets.selection[&(track_id, effect_id.into())],
        selected
    );

    let saved = DevicePreset::new(
        request.preset.kind(),
        "Saved after undo",
        request.preset.parameters().clone(),
    )
    .unwrap();
    let _ = app.update(Message::DevicePreset(DevicePresetMsg::Saved(
        request,
        Ok(Some(saved.clone())),
    )));
    assert_eq!(app.state.status_text, "Saved preset: Saved after undo");
}

#[test]
fn a_load_cancelled_by_an_edit_says_so() {
    let (mut app, track_id, effect_id) = gain_app();
    let _ = app.update(Message::DevicePreset(DevicePresetMsg::Load(
        track_id,
        effect_id.into(),
    )));
    let _ = app.update(Message::Devices(DevicesMsg::SetEffectParam(
        track_id, effect_id, 1, 3.0,
    )));
    assert!(app.device_presets.load_target.is_none());
    assert_eq!(
        app.state.status_text,
        "Preset load cancelled: the device changed while choosing a file"
    );
}
