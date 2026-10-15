//! Delay preset files preserve complete sounds and existing device ownership through App transactions.

use super::*;
use crate::{
    domains::{automation::AutomationMsg, devices::DevicesMsg, project::ProjectMsg},
    state::ProjectTrack,
};
use vibez_core::{automation::AutomationTarget, effect::EffectType};
use vibez_dsp::delay::param;

fn delay_app() -> (App, TrackId, EffectId) {
    let mut app = super::super::test_support::app();
    let track = ProjectTrack::new(TrackId::new(), "Delay source".into(), 2);
    let track_id = track.id;
    let bus = ProjectTrack::new(TrackId::new(), "Existing return".into(), 4);
    let bus_id = bus.id;
    let project = Arc::make_mut(&mut app.state.project_tracks);
    project.tracks.push(track);
    project.buses.push(bus);
    project.tracks[0].sends.push((bus_id, 0.375));
    for kind in [EffectType::Gain, EffectType::Delay] {
        let _ = app.update(Message::Devices(DevicesMsg::AddEffect(track_id, kind)));
    }
    let effect = app.state.find_track(track_id).unwrap().effects[1].id;
    let _ = app.update(Message::Devices(DevicesMsg::ToggleEffectBypass(track_id, effect)));
    let _ = app.update(Message::Automation(AutomationMsg::AddLane {
        track_id,
        target: AutomationTarget::EffectParam { effect_id: effect, param_index: param::MIX },
    }));
    let lane_id = app.state.arrangement.timeline.get(track_id).unwrap().automation[0].id;
    for (beat, value) in [(0.0, 0.25), (4.0, 0.75)] {
        let _ = app.update(Message::Automation(AutomationMsg::AddPoint {
            track_id, lane_id, beat, value,
        }));
    }
    app.state.project.history = Default::default();
    app.state.project.dirty = false;
    (app, track_id, effect)
}

#[tokio::test]
async fn delay_file_roundtrip_loads_all_seventeen_parameters_as_one_undoable_edit() {
    let (mut app, track, effect) = delay_app();
    let custom = [
        723.5, 0.61, 0.42, 1.0, 2.0, 2.0, 1.0, 0.0, 991.25,
        5.0, 1.0, 1.0, 233.0, 1.0, 6311.0, 0.37, 1.0,
    ];
    let _ = app.update(Message::Devices(DevicesMsg::SetEffectParams(
        track, effect, custom.into_iter().enumerate().collect(),
    )));
    let save = app.begin_device_preset_save(track, effect).unwrap();
    assert_eq!(save.preset.parameters().len(), param::COUNT);
    let init = crate::domains::device_presets::factory_presets(EffectType::Delay)[0].clone();
    let _ = app.update(Message::ApplyDevicePreset(track, effect, init));
    let baseline = app.state.find_track(track).unwrap().clone();
    let lanes = app.state.arrangement.timeline.get(track).unwrap().automation.clone();
    app.state.project.history = Default::default();
    app.state.project.dirty = false;

    let directory = tempfile::Builder::new().prefix("vibez-delay-preset-").tempdir().unwrap();
    let path = directory.path().join("Custom musical Delay");
    let saved = super::super::tasks::save_device_preset_at(path, save.preset.clone()).await.unwrap();
    let loaded = vibez_project::device_preset::load_device_preset(
        &directory.path().join("Custom musical Delay.vzpreset"),
    ).unwrap();
    assert_eq!(loaded, saved);
    assert_eq!(loaded.name(), "Custom musical Delay");
    assert_eq!(loaded.parameters(), save.preset.parameters());
    let _ = app.update(Message::DevicePreset(DevicePresetMsg::Saved(save, Ok(Some(saved)))));
    assert_eq!(app.state.find_track(track).unwrap().effects[1].params, baseline.effects[1].params);
    assert!(!app.state.project.dirty);
    assert!(!app.state.project.history.can_undo());

    let load = app.begin_device_preset_load(track, effect).unwrap();
    let _ = app.update(Message::DevicePreset(DevicePresetMsg::Loaded(load, Ok(Some(loaded.clone())))));
    for (action, expected, undo, redo) in [
        (None, custom.to_vec(), 1, 0),
        (Some(ProjectMsg::Undo), baseline.effects[1].params.clone(), 0, 1),
        (Some(ProjectMsg::Redo), custom.to_vec(), 1, 0),
    ] {
        if let Some(action) = action { let _ = app.update(Message::Project(action)); }
        let current = app.state.find_track(track).unwrap();
        assert_eq!(current.effects.len(), 2);
        assert_eq!(current.effects[1].id, effect);
        assert_eq!(current.effects[1].params, expected);
        assert!(current.effects[1].bypass);
        assert_eq!(current.effects[0].id, baseline.effects[0].id);
        assert_eq!(current.effects[0].params, baseline.effects[0].params);
        assert_eq!(current.sends, baseline.sends);
        assert_eq!(app.state.arrangement.timeline.get(track).unwrap().automation, lanes);
        assert_eq!(app.state.project.history.undo.len(), undo);
        assert_eq!(app.state.project.history.redo.len(), redo);
    }
    app.state.project.history = Default::default();
    app.state.project.dirty = false;
    let unchanged = app.begin_device_preset_load(track, effect).unwrap();
    let _ = app.update(Message::DevicePreset(DevicePresetMsg::Loaded(unchanged, Ok(Some(loaded)))));
    assert!(!app.state.project.dirty);
    assert!(!app.state.project.history.can_undo());
}

#[test]
fn cancelled_or_stale_delay_loads_cannot_overwrite_current_device_or_new_request() {
    for action in ["cancel", "edit away and back", "remove and undo", "project reset and replay", "superseded"] {
        let (mut app, track, effect) = delay_app();
        let original = app.capture_device_preset(track, effect).unwrap();
        let stale = app.begin_device_preset_load(track, effect).unwrap();
        let mut superseding = None;
        match action {
            "cancel" => {
                let _ = app.update(Message::DevicePreset(DevicePresetMsg::Loaded(
                    stale.clone(), Ok(None),
                )));
            }
            "edit away and back" => {
                for value in [501.0, 500.0] {
                    let _ = app.update(Message::Devices(DevicesMsg::SetEffectParam(
                        track, effect, param::TIME_MS, value,
                    )));
                }
            }
            "remove and undo" => {
                let _ = app.update(Message::Devices(DevicesMsg::RemoveEffect(track, effect)));
                let _ = app.update(Message::Project(ProjectMsg::Undo));
            }
            "project reset and replay" => {
                let snapshot = app.take_snapshot();
                app.clear_project_runtime();
                app.apply_snapshot(snapshot);
            }
            "superseded" => superseding = app.begin_device_preset_load(track, effect),
            _ => unreachable!(),
        }
        let current = app.capture_device_preset(track, effect).unwrap();
        assert_eq!(current.parameters(), original.parameters(), "{action}");
        let selected = app.device_presets.selection.get(&(track, effect)).cloned();
        app.state.project.history = Default::default();
        app.state.project.dirty = false;
        let replacement = crate::domains::device_presets::factory_presets(EffectType::Delay)[6].clone();
        let _ = app.update(Message::DevicePreset(DevicePresetMsg::Loaded(
            stale.clone(), Ok(Some(replacement)),
        )));
        assert_eq!(app.capture_device_preset(track, effect).unwrap().parameters(),
            current.parameters(), "{action}");
        assert_eq!(app.state.find_track(track).unwrap().effects[1].id, effect, "{action}");
        assert_eq!(app.device_presets.selection.get(&(track, effect)), selected.as_ref(), "{action}");
        assert!(!app.state.project.dirty, "{action}");
        assert!(!app.state.project.history.can_undo(), "{action}");
        assert!(!app.device_presets.load_request.is_current(stale.token), "{action}");
        if let Some(request) = superseding {
            assert!(app.device_presets.load_request.is_current(request.token));
            assert_eq!(app.device_presets.load_target, Some((track, effect)));
        } else {
            assert!(!app.device_presets.load_request.is_active());
            assert!(app.device_presets.load_target.is_none());
        }
    }
}
