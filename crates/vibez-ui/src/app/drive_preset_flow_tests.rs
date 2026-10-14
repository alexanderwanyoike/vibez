//! Verifies Drive sound files preserve destination identity and undo through the shared preset flow.

use super::*;
use crate::domains::{device_presets, devices::DevicesMsg};
use crate::state::ProjectTrack;
use vibez_core::{automation::{AutomationLane, AutomationTarget}, effect::EffectType};

fn drive_app(channel: usize) -> (App, TrackId, EffectId) {
    let mut app = super::super::test_support::app();
    let send_bus = ProjectTrack::new(TrackId::new(), "Parallel return".into(), 2);
    let send_bus_id = send_bus.id;
    Arc::make_mut(&mut app.state.project_tracks).buses.push(send_bus);
    let track_id = if channel == 2 {
        app.state.project_tracks.master.id
    } else {
        let track = ProjectTrack::new(TrackId::new(), "Driven".into(), 3);
        let id = track.id;
        let project = Arc::make_mut(&mut app.state.project_tracks);
        if channel == 0 { project.tracks.push(track); } else { project.buses.push(track); }
        id
    };
    for kind in [EffectType::Gain, EffectType::Drive, EffectType::Gain] {
        let _ = app.update(Message::Devices(DevicesMsg::AddEffect(track_id, kind)));
    }
    let track = app.state.find_track_mut(track_id).unwrap();
    track.gain = 0.7;
    track.pan = -0.2;
    if channel != 2 { track.sends.push((send_bus_id, 0.3)); }
    let effect = &mut track.effects[1];
    effect.params[0] = 0.731_273;
    effect.bypass = true;
    let effect_id = effect.id;
    app.state.arrange_content_mut(track_id).automation.push(AutomationLane::new(AutomationTarget::EffectParam { effect_id, param_index: 0 }));
    app.state.project.history = Default::default();
    app.state.project.dirty = false;
    (app, track_id, effect_id)
}

#[tokio::test]
async fn drive_factory_files_preserve_each_channel_owner_and_undo_as_one_sound_edit() {
    let factories = device_presets::factory_presets(EffectType::Drive);
    assert_eq!(factories.len(), 9);
    let folder = tempfile::tempdir().unwrap();
    for channel in 0..3 {
        let (mut app, track, effect) = drive_app(channel);
        for factory in &factories {
            app.state.project.history = Default::default();
            let before = app.state.find_track(track).unwrap().clone();
            let automation = app.state.arrange_content(track).unwrap().automation.clone();
            let path = folder.path().join("Drive.vzpreset");
            let saved = tasks::save_device_preset_at(path.clone(), factory.clone()).await.unwrap();
            let loaded = vibez_project::device_preset::load_device_preset(&path).unwrap();
            assert_eq!(saved, loaded);
            let request = app.begin_device_preset_load(track, effect).unwrap();
            let _ = app.finish_device_preset_load(request, Ok(loaded.clone()));
            let after = app.state.find_track(track).unwrap();
            assert_eq!(after.effects.iter().map(|effect| effect.id).collect::<Vec<_>>(), before.effects.iter().map(|effect| effect.id).collect::<Vec<_>>());
            assert_eq!((after.gain, after.pan, &after.sends), (before.gain, before.pan, &before.sends));
            assert_eq!(after.effects[1].bypass, before.effects[1].bypass);
            assert_eq!(after.effects[1].sidechains, before.effects[1].sidechains);
            assert_eq!(after.effects[1].external_inputs, before.effects[1].external_inputs);
            assert_eq!(after.effects[1].latency_samples, Some(64));
            assert_eq!(app.state.arrange_content(track).unwrap().automation, automation);
            assert_eq!(device_presets::capture(&after.effects[1], loaded.name()).unwrap(), loaded);
            let request = app.begin_device_preset_load(track, effect).unwrap();
            let _ = app.finish_device_preset_load(request, Ok(loaded));
            let snapshot = app.state.project.history.pop_undo().expect("one sound edit");
            assert!(!app.state.project.history.can_undo(), "same sound added history");
            app.apply_snapshot(snapshot);
            assert_eq!(app.state.find_track(track).unwrap().effects[1].params, before.effects[1].params);
        }
    }
}

#[test]
fn drive_factory_catalogue_has_neutral_legacy_init_explicit_modern_init_and_concept_values() {
    let presets = device_presets::factory_presets(EffectType::Drive);
    let preset = |name: &str| presets.iter().find(|preset| preset.name() == name).unwrap();
    assert_eq!(presets.len(), 9);
    let mut names: Vec<_> = presets.iter().map(|preset| preset.name()).collect();
    names.sort_unstable(); names.dedup(); assert_eq!(names.len(), presets.len());
    assert_eq!(preset("Init").parameters()["character"], 0.0);
    assert_eq!(preset("Soft Init").parameters()["character"], 1.0);
    for modern in presets.iter().filter(|preset| preset.name() != "Init") { assert_eq!(modern.parameters()["quality"], 2.0); }
    let warm = preset("Warm Edge").parameters();
    assert!((crate::domains::drive_values::amount_db(warm["amount"]) - 6.0).abs() < 1e-5);
    assert_eq!(warm["tone"], 0.55);
    assert_eq!(warm["mix"], 0.8);
    assert!((20.0 * warm["output"].log10() + 3.0).abs() < 1e-5);
}

#[test]
fn drive_load_cancellation_errors_stale_requests_and_invalid_discrete_values_leave_sound_intact() {
    use vibez_core::device_preset::{DevicePreset, DevicePresetKind};
    let (mut app, track, effect) = drive_app(0);
    let before = app.state.find_track(track).unwrap().effects[1].params.clone();
    let factory = device_presets::factory_presets(EffectType::Drive).into_iter().find(|preset| preset.name() == "Warm Edge").unwrap();
    let stale = app.begin_device_preset_load(track, effect).unwrap();
    let current = app.begin_device_preset_load(track, effect).unwrap();
    let _ = app.finish_device_preset_load(stale, Ok(factory.clone()));
    assert_eq!(app.state.find_track(track).unwrap().effects[1].params, before);
    let _ = app.update_device_preset(DevicePresetMsg::Loaded(current, Ok(None)));
    let request = app.begin_device_preset_load(track, effect).unwrap();
    let _ = app.finish_device_preset_load(request, Err("read failed".into()));
    assert_eq!(app.state.find_track(track).unwrap().effects[1].params, before);
    let request = app.begin_device_preset_load(track, effect).unwrap();
    let wrong = device_presets::factory_presets(EffectType::Gain).remove(0);
    let _ = app.finish_device_preset_load(request, Ok(wrong));
    assert_eq!(app.state.find_track(track).unwrap().effects[1].params, before);
    for key in ["character", "quality", "pre_hp_enabled", "post_lp_enabled"] {
        let mut parameters = factory.parameters().clone();
        parameters.insert(key.into(), 0.5);
        let invalid = DevicePreset::new(DevicePresetKind::Effect(EffectType::Drive), "Invalid", parameters).unwrap();
        let request = app.begin_device_preset_load(track, effect).unwrap();
        let _ = app.finish_device_preset_load(request, Ok(invalid));
        assert_eq!(app.state.find_track(track).unwrap().effects[1].params, before, "accepted fractional {key}");
    }
    let mut incomplete = factory.parameters().clone();
    incomplete.remove("output");
    let request = app.begin_device_preset_load(track, effect).unwrap();
    let _ = app.finish_device_preset_load(request, Ok(DevicePreset::new(DevicePresetKind::Effect(EffectType::Drive), "Incomplete", incomplete).unwrap()));
    assert_eq!(app.state.find_track(track).unwrap().effects[1].params, before);
    assert!(!app.state.project.history.can_undo());
    assert!(!app.state.project.dirty);
}

#[tokio::test]
async fn drive_user_save_roundtrips_real_files_and_late_save_cannot_replace_a_changed_sound() {
    let (mut app, track, effect) = drive_app(0);
    let before = app.state.find_track(track).unwrap().effects[1].params.clone();
    let request = app.begin_device_preset_save(track, effect).unwrap();
    let folder = tempfile::tempdir().unwrap();
    let path = folder.path().join("My Drive.vzpreset");
    let saved = tasks::save_device_preset_at(path.clone(), request.preset.clone()).await.unwrap();
    assert_eq!(vibez_project::device_preset::load_device_preset(&path).unwrap(), saved);
    assert_eq!(saved.name(), "My Drive");
    app.finish_device_preset_save(request, Ok(Some(saved)));
    assert_eq!(app.state.find_track(track).unwrap().effects[1].params, before);
    assert!(!app.state.project.dirty);
    assert!(!app.state.project.history.can_undo());
    let request = app.begin_device_preset_save(track, effect).unwrap();
    let saved_snapshot = request.preset.clone();
    let _ = app.update(Message::set_effect_param(track, effect, 0, 0.4));
    app.finish_device_preset_save(request, Ok(Some(saved_snapshot)));
    assert_eq!(app.state.find_track(track).unwrap().effects[1].params[0], 0.4);
    let snapshot = app.state.project.history.pop_undo().unwrap();
    assert!(!app.state.project.history.can_undo());
    app.apply_snapshot(snapshot);
    assert_eq!(app.state.find_track(track).unwrap().effects[1].params, before);
    let request = app.begin_device_preset_save(track, effect).unwrap();
    app.finish_device_preset_save(request, Ok(None));
    let request = app.begin_device_preset_save(track, effect).unwrap();
    app.finish_device_preset_save(request, Err("write failed".into()));
    assert_eq!(app.state.find_track(track).unwrap().effects[1].params, before);
}

#[test]
fn drive_neutral_init_renders_uncolored_audio_after_the_declared_delay() {
    use vibez_dsp::drive::{param, DRIVE_PARAMS};
    let init = device_presets::factory_presets(EffectType::Drive)
        .into_iter().find(|preset| preset.name() == "Init").unwrap();
    let parameters: Vec<_> = crate::domains::drive_presets::KEYS.iter()
        .map(|key| init.parameters()[*key]).collect();
    assert_eq!(DRIVE_PARAMS[param::AMOUNT].default, 0.35);
    for rate in [44_100.0, 48_000.0, 96_000.0] {
        let mut effect = vibez_dsp::factory::create_effect_with_params(EffectType::Drive, rate, &parameters);
        let latency = effect.latency_samples() as usize;
        assert_eq!(latency, 64);
        let input: Vec<_> = (0..2048).map(|frame| {
            let phase = std::f32::consts::TAU * frame as f32 / 127.0;
            [0.7 * phase.sin(), 0.4 * (3.0 * phase).cos()]
        }).flatten().collect();
        let mut output = input.clone();
        output.resize(input.len() + latency * 2, 0.0);
        for block in output.chunks_mut(74) { effect.process(block, 2); }
        assert!(output[..latency * 2].iter().all(|sample| *sample == 0.0));
        for (actual, expected) in output[latency * 2..].iter().zip(&input) {
            assert!((actual - expected).abs() < 1e-6, "rate={rate}, {actual} != {expected}");
        }
    }
}
