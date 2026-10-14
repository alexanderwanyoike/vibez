//! Compressor sound presets preserve channel and detector routing across file loads.

use super::*;
use crate::domains::{device_presets, devices::DevicesMsg};
use crate::state::ProjectTrack;
use vibez_core::automation::{AutomationLane, AutomationTarget};
use vibez_core::effect::EffectType;
use vibez_core::routing::{ExternalInputId, SidechainAssignment, SourceTap};

fn compressor_app(channel: usize) -> (App, TrackId, EffectId) {
    let mut app = super::super::test_support::app();
    let source = ProjectTrack::new(TrackId::new(), "Detector".into(), 6);
    let source_id = source.id;
    Arc::make_mut(&mut app.state.project_tracks)
        .tracks
        .push(source);
    let track_id = if channel == 2 {
        app.state.project_tracks.master.id
    } else {
        let track = ProjectTrack::new(TrackId::new(), "Compressed".into(), 3);
        let id = track.id;
        let project = Arc::make_mut(&mut app.state.project_tracks);
        if channel == 0 {
            project.tracks.push(track);
        } else {
            project.buses.push(track);
        }
        id
    };
    for kind in [EffectType::Gain, EffectType::Compressor, EffectType::Gain] {
        let _ = app.update(Message::Devices(DevicesMsg::AddEffect(track_id, kind)));
    }
    let track = app.state.find_track_mut(track_id).unwrap();
    track.gain = 0.7;
    track.pan = -0.2;
    track.sends.push((source_id, 0.3));
    let effect = track
        .effects
        .iter_mut()
        .find(|e| e.effect_type == EffectType::Compressor)
        .unwrap();
    effect.params[0] = -23.0;
    effect.bypass = true;
    effect.sidechains.push(SidechainAssignment {
        input_id: ExternalInputId(0),
        input_name: "Sidechain".into(),
        source: source_id,
        source_name: "Detector".into(),
        tap: SourceTap::AfterFader,
    });
    let effect_id = effect.id;
    app.state
        .arrange_content_mut(track_id)
        .automation
        .push(AutomationLane::new(AutomationTarget::EffectParam {
            effect_id,
            param_index: 0,
        }));
    app.state.project.history = Default::default();
    app.state.project.dirty = false;
    (app, track_id, effect_id)
}

#[tokio::test]
async fn compressor_factory_files_preserve_routing_and_undo_once_on_every_channel_kind() {
    let factories = device_presets::factory_presets(EffectType::Compressor);
    assert!(
        factories.len() >= 6,
        "Compressor needs useful factory presets"
    );
    let folder = tempfile::tempdir().unwrap();
    for channel in 0..3 {
        let (mut app, track_id, effect_id) = compressor_app(channel);
        for factory in &factories {
            let before = app.state.find_track(track_id).unwrap().clone();
            let before_automation = app
                .state
                .arrange_content(track_id)
                .unwrap()
                .automation
                .clone();
            let path = folder.path().join("Compressor.vzpreset");
            let saved = tasks::save_device_preset_at(path.clone(), factory.clone())
                .await
                .unwrap();
            let loaded = vibez_project::device_preset::load_device_preset(&path).unwrap();
            assert_eq!(saved, loaded);
            let request = app.begin_device_preset_load(track_id, effect_id).unwrap();
            let _ = app.finish_device_preset_load(request, Ok(loaded.clone()));
            let after = app.state.find_track(track_id).unwrap();
            assert_eq!(
                after.effects.iter().map(|e| e.id).collect::<Vec<_>>(),
                before.effects.iter().map(|e| e.id).collect::<Vec<_>>()
            );
            assert_eq!(
                (after.gain, after.pan, &after.sends),
                (before.gain, before.pan, &before.sends)
            );
            let effect = after.effects.iter().find(|e| e.id == effect_id).unwrap();
            let old_effect = before.effects.iter().find(|e| e.id == effect_id).unwrap();
            assert_eq!(effect.bypass, old_effect.bypass);
            assert_eq!(effect.sidechains, old_effect.sidechains);
            assert_eq!(effect.external_inputs, old_effect.external_inputs);
            assert_eq!(
                app.state.arrange_content(track_id).unwrap().automation,
                before_automation
            );
            assert!(device_presets::prepare_updates(effect, &loaded)
                .unwrap()
                .is_empty());
            assert_eq!(
                device_presets::capture(effect, loaded.name()).unwrap(),
                loaded
            );
            let request = app.begin_device_preset_load(track_id, effect_id).unwrap();
            let _ = app.finish_device_preset_load(request, Ok(loaded));
            let snapshot = app
                .state
                .project
                .history
                .pop_undo()
                .expect("one sound edit");
            assert!(
                !app.state.project.history.can_undo(),
                "identical load must not add undo"
            );
            app.apply_snapshot(snapshot);
            assert_eq!(
                app.state
                    .find_track(track_id)
                    .unwrap()
                    .effects
                    .iter()
                    .find(|e| e.id == effect_id)
                    .unwrap()
                    .params,
                old_effect.params
            );
        }
    }
}

#[tokio::test]
async fn compressor_save_captures_a_snapshot_without_changing_sound_or_history() {
    let (mut app, track, effect) = compressor_app(0);
    let before = app
        .state
        .find_track(track)
        .unwrap()
        .effects
        .iter()
        .find(|e| e.id == effect)
        .unwrap()
        .params
        .clone();
    let request = app
        .begin_device_preset_save(track, effect)
        .expect("native Compressor save");
    let folder = tempfile::tempdir().unwrap();
    let path = folder.path().join("Saved Compressor.vzpreset");
    let saved = tasks::save_device_preset_at(path.clone(), request.preset.clone())
        .await
        .unwrap();
    assert_eq!(
        vibez_project::device_preset::load_device_preset(&path).unwrap(),
        request.preset
    );
    app.finish_device_preset_save(request, Ok(Some(saved)));
    assert_eq!(
        app.state
            .find_track(track)
            .unwrap()
            .effects
            .iter()
            .find(|e| e.id == effect)
            .unwrap()
            .params,
        before
    );
    assert!(!app.state.project.dirty);
    assert!(!app.state.project.history.can_undo());
}

#[test]
fn compressor_rejects_bad_sound_documents_and_late_loads_without_partial_mutation() {
    let (mut app, track, effect_id) = compressor_app(0);
    let effect = app
        .state
        .find_track(track)
        .unwrap()
        .effects
        .iter()
        .find(|e| e.id == effect_id)
        .unwrap()
        .clone();
    let current = device_presets::capture(&effect, "Current").expect("Compressor capture");
    for key in [
        "hp_enabled",
        "lp_enabled",
        "detector_mode",
        "auto_release",
        "stereo_link",
        "detector_listen",
        "range_enabled",
    ] {
        let mut values = current.parameters().clone();
        values.insert(key.into(), 0.5);
        let invalid = DevicePreset::new(current.kind(), "Fractional", values).unwrap();
        assert!(
            device_presets::prepare_updates(&effect, &invalid).is_err(),
            "{key}"
        );
    }
    for (key, value) in [("threshold_db", -61.0), ("unknown", 0.0)] {
        let mut values = current.parameters().clone();
        values.insert(key.into(), value);
        let invalid = DevicePreset::new(current.kind(), "Invalid", values).unwrap();
        let request = app.begin_device_preset_load(track, effect_id).unwrap();
        let _ = app.finish_device_preset_load(request, Ok(invalid));
        assert_eq!(
            app.state
                .find_track(track)
                .unwrap()
                .effects
                .iter()
                .find(|e| e.id == effect_id)
                .unwrap()
                .params,
            effect.params
        );
        assert!(!app.state.project.dirty);
        assert!(!app.state.project.history.can_undo());
    }
    let request = app.begin_device_preset_load(track, effect_id).unwrap();
    let wrong = device_presets::factory_presets(EffectType::Gain)[0].clone();
    let _ = app.finish_device_preset_load(request, Ok(wrong));
    let request = app.begin_device_preset_load(track, effect_id).unwrap();
    let _ = app.update_device_preset(DevicePresetMsg::Loaded(request, Ok(None)));
    let request = app.begin_device_preset_load(track, effect_id).unwrap();
    let _ = app.finish_device_preset_load(request, Err("read failed".into()));
    assert!(!app.state.project.dirty);
    assert!(!app.state.project.history.can_undo());
    let stale = app.begin_device_preset_load(track, effect_id).unwrap();
    let _ = app.update(Message::set_effect_param(track, effect_id, 0, -20.0));
    let preset = device_presets::factory_presets(EffectType::Compressor)
        .into_iter()
        .next()
        .unwrap();
    let _ = app.finish_device_preset_load(stale, Ok(preset));
    assert_eq!(
        app.state
            .find_track(track)
            .unwrap()
            .effects
            .iter()
            .find(|e| e.id == effect_id)
            .unwrap()
            .params[0],
        -20.0
    );
}
