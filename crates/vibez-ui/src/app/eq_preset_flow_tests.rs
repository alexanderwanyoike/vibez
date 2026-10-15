//! Verifies EQ preset files preserve shared channel identity and undo semantics.

use super::*;
use crate::domains::{device_presets, devices::DevicesMsg};
use crate::state::ProjectTrack;
use vibez_core::effect::EffectType;

fn eq_app(destination: usize) -> (App, TrackId, EffectId) {
    let mut app = super::super::test_support::app();
    let ordinary = ProjectTrack::new(TrackId::new(), "Audio".into(), 3);
    let return_bus = ProjectTrack::new(TrackId::new(), "Return".into(), 6);
    let track = [
        ordinary.id,
        return_bus.id,
        app.state.project_tracks.master.id,
    ][destination];
    let project = Arc::make_mut(&mut app.state.project_tracks);
    project.tracks.push(ordinary);
    project.buses.push(return_bus);
    let _ = app.update(Message::Devices(DevicesMsg::AddEffect(
        track,
        EffectType::Eq,
    )));
    let effect = app
        .state
        .find_track(track)
        .unwrap()
        .effects
        .last()
        .unwrap()
        .id;
    let _ = app.update(Message::Devices(DevicesMsg::AddEffect(
        track,
        EffectType::Gain,
    )));
    app.state.project.history = Default::default();
    app.state.project.dirty = false;
    (app, track, effect)
}

#[tokio::test]
async fn eq_factory_files_load_in_place_on_tracks_returns_and_master_and_undo_once() {
    let presets = device_presets::factory_presets(EffectType::Eq);
    assert!(presets.len() >= 6, "EQ needs useful built-in presets");
    let folder = tempfile::tempdir().unwrap();
    for destination in 0..3 {
        let (mut app, track, effect) = eq_app(destination);
        for factory in &presets {
            app.state.project.history = Default::default();
            app.state.project.dirty = false;
            let before = app.state.find_track(track).unwrap().clone();
            let old = before.effects.iter().find(|fx| fx.id == effect).unwrap();
            let updates = device_presets::prepare_updates(old, factory).unwrap();
            let path = folder.path().join("Saved EQ.vzpreset");
            let saved = tasks::save_device_preset_at(path.clone(), factory.clone())
                .await
                .unwrap();
            let loaded = vibez_project::device_preset::load_device_preset(&path).unwrap();
            assert_eq!(saved, loaded);
            let request = app.begin_device_preset_load(track, effect).unwrap();
            let _ = app.finish_device_preset_load(request, Ok(loaded.clone()));
            let after = app.state.find_track(track).unwrap();
            assert_eq!(
                after.effects.iter().map(|fx| fx.id).collect::<Vec<_>>(),
                before.effects.iter().map(|fx| fx.id).collect::<Vec<_>>()
            );
            assert_eq!(after.sends, before.sends);
            assert_eq!(
                (after.gain, after.pan, after.mute),
                (before.gain, before.pan, before.mute)
            );
            let applied = after.effects.iter().find(|fx| fx.id == effect).unwrap();
            assert_eq!(applied.bypass, old.bypass);
            assert_eq!(applied.sidechains, old.sidechains);
            assert_eq!(applied.external_inputs, old.external_inputs);
            assert!(device_presets::prepare_updates(applied, &loaded)
                .unwrap()
                .is_empty());
            if updates.is_empty() {
                assert!(!app.state.project.dirty);
                assert!(!app.state.project.history.can_undo());
            } else {
                let snapshot = app.state.project.history.pop_undo().unwrap();
                assert!(!app.state.project.history.can_undo());
                app.apply_snapshot(snapshot);
                let restored = app
                    .state
                    .find_track(track)
                    .unwrap()
                    .effects
                    .iter()
                    .find(|fx| fx.id == effect)
                    .unwrap();
                assert_eq!(restored.params, old.params);
            }
        }
    }
}

#[test]
fn eq_files_reject_wrong_kind_invalid_choices_and_stale_completions_without_mutation() {
    let (mut app, track, effect) = eq_app(0);
    let before = app.state.find_track(track).unwrap().effects[0]
        .params
        .clone();
    let wrong = device_presets::factory_presets(EffectType::Gain).remove(0);
    let request = app
        .begin_device_preset_load(track, effect)
        .expect("EQ preset load is supported");
    let _ = app.finish_device_preset_load(request, Ok(wrong));
    assert_eq!(
        app.state.find_track(track).unwrap().effects[0].params,
        before
    );
    assert!(!app.state.project.dirty);
    for (key, value) in [
        ("lf_enabled", 0.5),
        ("audition_band", 1.5),
        ("hmf_gain_db", 40.0),
        ("unknown_key", 1.0),
    ] {
        let factory = device_presets::factory_presets(EffectType::Eq).remove(0);
        let mut parameters = factory.parameters().clone();
        parameters.insert(key.into(), value);
        let invalid = vibez_core::device_preset::DevicePreset::new(
            vibez_core::device_preset::DevicePresetKind::Effect(EffectType::Eq),
            "Invalid EQ",
            parameters,
        )
        .unwrap();
        let request = app.begin_device_preset_load(track, effect).unwrap();
        let _ = app.finish_device_preset_load(request, Ok(invalid));
        assert_eq!(
            app.state.find_track(track).unwrap().effects[0].params,
            before
        );
        assert!(!app.state.project.dirty);
        assert!(!app.state.project.history.can_undo());
    }
    let request = app.begin_device_preset_load(track, effect).unwrap();
    let _ = app.update(Message::set_effect_param(track, effect, 6, 3.0));
    let factory = device_presets::factory_presets(EffectType::Eq).remove(0);
    let _ = app.finish_device_preset_load(request, Ok(factory));
    assert_eq!(
        app.state.find_track(track).unwrap().effects[0].params[6],
        3.0
    );
}

#[test]
fn saved_eq_preserves_legacy_fractional_shelf_bell_parameters() {
    let (mut app, track, effect) = eq_app(0);
    let _ = app.update(Message::set_effect_params(
        track,
        effect,
        vec![(2, 0.35), (11, 0.65)],
    ));
    let effect = app
        .state
        .find_track(track)
        .unwrap()
        .effects
        .iter()
        .find(|fx| fx.id == effect)
        .unwrap();
    let saved = device_presets::capture(effect, "Legacy shape").unwrap();
    assert_eq!(saved.parameters()["lf_bell"], 0.35);
    assert_eq!(saved.parameters()["hf_bell"], 0.65);
    assert!(device_presets::prepare_updates(effect, &saved)
        .unwrap()
        .is_empty());
}
