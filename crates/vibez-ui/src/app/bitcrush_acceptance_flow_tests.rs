//! Verifies complete Bitcrush files, project reopen and native owner identity through actual App operations.

use super::*;
use crate::domains::{device_presets, project::ProjectMsg};
use vibez_core::automation::{AutomationLane, AutomationPoint, AutomationTarget};
use vibez_dsp::bitcrush::param;

const SOUND: [f32; 9] = [8.75, 4.49, 0.37, 6.0, 2.0, 1.0, 0.35, 1.0, 3500.0];
const KEYS: [&str; 9] = [
    "bits",
    "sr_reduce",
    "mix",
    "drive_db",
    "rounding",
    "dither",
    "jitter",
    "post_lp_enabled",
    "post_lp_frequency_hz",
];

#[derive(Clone, Copy, Debug)]
enum Owner {
    Track,
    Return,
    Master,
}

fn owner_app(owner: Owner) -> (App, TrackId, EffectId) {
    let mut app = super::super::super::test_support::app();
    app.state.transport.sample_rate = 96_000;
    app.send_command(EngineCommand::SetSampleRate(96_000));
    let send = ProjectTrack::new(TrackId::new(), "Parallel return".into(), 6);
    let send_id = send.id;
    Arc::make_mut(&mut app.state.project_tracks)
        .buses
        .push(send);
    let track = match owner {
        Owner::Master => app.state.project_tracks.master.id,
        _ => {
            let channel = ProjectTrack::new(TrackId::new(), "Bitcrush owner".into(), 3);
            let id = channel.id;
            let project = Arc::make_mut(&mut app.state.project_tracks);
            match owner {
                Owner::Track => project.tracks.push(channel),
                Owner::Return => project.buses.push(channel),
                Owner::Master => unreachable!(),
            }
            id
        }
    };
    for kind in [EffectType::Gain, EffectType::Bitcrush, EffectType::Gain] {
        let _ = app.update(Message::Devices(DevicesMsg::AddEffect(track, kind)));
    }
    app.ensure_channel_eq(track);
    app.ensure_channel_eq(send_id);
    let target = app.state.find_track_mut(track).unwrap();
    target.gain = 0.7;
    if !matches!(owner, Owner::Master) {
        target.pan = 0.37;
    }
    let effect = target.effects[1].id;
    target.effects[1].params = SOUND.to_vec();
    target.effects[1].bypass = true;
    if matches!(owner, Owner::Track) {
        let _ = app.update(Message::Arrangement(
            crate::domains::arrangement::ArrangementMsg::SetSend {
                track_id: track,
                bus_id: send_id,
                amount: 0.3,
            },
        ));
        assert_eq!(app.state.find_track(track).unwrap().sends, [(send_id, 0.3)]);
    }
    let mut lane = AutomationLane::new(AutomationTarget::EffectParam {
        effect_id: effect,
        param_index: param::SR_REDUCE,
    });
    lane.insert_point(AutomationPoint {
        beat: 0.0,
        value: (6.25 - 1.0) / 31.0,
        curve: 0.0,
    });
    app.state.arrange_content_mut(track).automation.push(lane);
    app.state.project.history = Default::default();
    app.state.project.dirty = false;
    (app, track, effect)
}

#[tokio::test]
async fn all_eight_factory_files_apply_all_nine_keys_on_every_owner_with_one_undo_and_redo() {
    let factories = device_presets::factory_presets(EffectType::Bitcrush);
    assert_eq!(
        factories.iter().map(DevicePreset::name).collect::<Vec<_>>(),
        [
            "Init",
            "Crunchy 12",
            "Drum Crunch",
            "Dusty Bass",
            "Pocket Sampler",
            "Broken Clock",
            "Parallel Arcade",
            "Digital Air",
        ]
    );
    let folder = tempfile::tempdir().unwrap();
    for owner in [Owner::Track, Owner::Return, Owner::Master] {
        let (mut app, track, effect) = owner_app(owner);
        for (index, factory) in factories.iter().enumerate() {
            app.state.project.history = Default::default();
            app.state.project.dirty = false;
            let before = app.state.find_track(track).unwrap().clone();
            let automation = app.state.arrange_content(track).unwrap().automation.clone();
            let path = folder.path().join(format!("{owner:?}-{index}.vzpreset"));
            let saved = tasks::save_device_preset_at(path.clone(), factory.clone())
                .await
                .unwrap();
            let loaded = vibez_project::device_preset::load_device_preset(&path).unwrap();
            assert_eq!(saved, loaded);
            let expected: Vec<_> = KEYS.iter().map(|key| loaded.parameters()[*key]).collect();
            assert_eq!(loaded.parameters().len(), 9);
            let request = app.begin_device_preset_load(track, effect).unwrap();
            let _ = app.update(Message::DevicePreset(DevicePresetMsg::Loaded(
                request,
                Ok(Some(loaded.clone())),
            )));
            let after = app.state.find_track(track).unwrap();
            assert_eq!(
                after.effects[1].params,
                expected,
                "{owner:?}: {}",
                factory.name()
            );
            assert_eq!(
                after
                    .effects
                    .iter()
                    .map(|effect| effect.id)
                    .collect::<Vec<_>>(),
                before
                    .effects
                    .iter()
                    .map(|effect| effect.id)
                    .collect::<Vec<_>>()
            );
            assert_eq!(
                (after.gain, after.pan, &after.sends),
                (before.gain, before.pan, &before.sends)
            );
            assert_eq!(after.effects[1].bypass, before.effects[1].bypass);
            assert_eq!(after.effects[1].sidechains, before.effects[1].sidechains);
            assert_eq!(
                after.effects[1].inactive_sidechains,
                before.effects[1].inactive_sidechains
            );
            assert_eq!(
                after.effects[1].external_inputs,
                before.effects[1].external_inputs
            );
            assert_eq!(after.effects[1].latency_samples, Some(0));
            assert_eq!(
                app.state.arrange_content(track).unwrap().automation,
                automation
            );
            assert_eq!(app.state.project.history.undo.len(), 1);
            assert!(app.state.project.dirty);
            app.state.project.dirty = false;
            let request = app.begin_device_preset_load(track, effect).unwrap();
            let _ = app.update(Message::DevicePreset(DevicePresetMsg::Loaded(
                request,
                Ok(Some(loaded)),
            )));
            assert!(!app.state.project.dirty, "same sound is a no-op");
            assert_eq!(app.state.project.history.undo.len(), 1);
            let _ = app.update(Message::Project(ProjectMsg::Undo));
            assert_eq!(
                app.state.find_track(track).unwrap().effects[1].params,
                before.effects[1].params
            );
            assert!(!app.state.project.history.can_undo());
            assert_eq!(app.state.project.history.redo.len(), 1);
            let _ = app.update(Message::Project(ProjectMsg::Redo));
            assert_eq!(
                app.state.find_track(track).unwrap().effects[1].params,
                expected
            );
            assert_eq!(app.state.project.history.undo.len(), 1);
            assert!(app.state.project.history.redo.is_empty());
            assert_eq!(
                app.state.arrange_content(track).unwrap().automation,
                automation
            );
            assert_eq!(
                app.cmd_tx.prepared_effect_sample_rate(track, effect),
                Some(96_000)
            );
        }
    }
}

#[tokio::test]
async fn fractional_user_sound_survives_files_project_reopen_and_replay_on_every_owner() {
    let folder = tempfile::Builder::new()
        .prefix(".bitcrush-file-project-")
        .tempdir_in(std::env::current_dir().unwrap())
        .unwrap();
    for owner in [Owner::Track, Owner::Return, Owner::Master] {
        let (mut app, track, effect) = owner_app(owner);
        let before = app.state.find_track(track).unwrap().clone();
        let automation = app.state.arrange_content(track).unwrap().automation.clone();
        let request = app.begin_device_preset_save(track, effect).unwrap();
        assert_eq!(request.preset.parameters().len(), KEYS.len());
        let path = folder.path().join(format!("User-{owner:?}.vzpreset"));
        let saved = tasks::save_device_preset_at(path.clone(), request.preset.clone())
            .await
            .unwrap();
        app.finish_device_preset_save(request, Ok(Some(saved.clone())));
        assert_eq!(
            KEYS.iter()
                .map(|key| saved.parameters()[*key])
                .collect::<Vec<_>>(),
            SOUND
        );
        assert_eq!(
            app.state.find_track(track).unwrap().effects[1].params,
            before.effects[1].params
        );
        assert!(!app.state.project.history.can_undo());
        assert!(!app.state.project.dirty);
        let _ = app.update(Message::set_effect_param(track, effect, param::BITS, 5.0));
        let before_load = app.state.find_track(track).unwrap().effects[1]
            .params
            .clone();
        app.state.project.history = Default::default();
        app.state.project.dirty = false;
        let request = app.begin_device_preset_load(track, effect).unwrap();
        let loaded = vibez_project::device_preset::load_device_preset(&path).unwrap();
        assert_eq!(loaded, saved);
        let _ = app.update(Message::DevicePreset(DevicePresetMsg::Loaded(
            request,
            Ok(Some(loaded.clone())),
        )));
        assert_eq!(
            app.state.find_track(track).unwrap().effects[1].params,
            SOUND
        );
        let destination = &app.state.find_track(track).unwrap().effects[1];
        assert_eq!(
            (
                destination.id,
                destination.bypass,
                destination.latency_samples
            ),
            (effect, before.effects[1].bypass, Some(0))
        );
        assert!(app.state.project.dirty);
        assert_eq!(app.state.project.history.undo.len(), 1);
        app.state.project.dirty = false;
        let request = app.begin_device_preset_load(track, effect).unwrap();
        let _ = app.update(Message::DevicePreset(DevicePresetMsg::Loaded(
            request,
            Ok(Some(loaded)),
        )));
        assert!(!app.state.project.dirty, "same user sound is a no-op");
        assert_eq!(app.state.project.history.undo.len(), 1);
        let _ = app.update(Message::Project(ProjectMsg::Undo));
        assert_eq!(
            app.state.find_track(track).unwrap().effects[1].params,
            before_load
        );
        assert!(!app.state.project.history.can_undo());
        let _ = app.update(Message::Project(ProjectMsg::Redo));
        assert_eq!(
            app.state.find_track(track).unwrap().effects[1].params,
            SOUND
        );
        for extension in ["vibez", "vzp"] {
            let path = folder.path().join(format!("Project-{owner:?}.{extension}"));
            super::super::super::save_project_async(path.clone(), None, app.project_for_save())
                .await
                .unwrap();
            let loaded = super::super::super::load_project_async(path, None)
                .await
                .unwrap();
            let mut restored = super::super::super::test_support::app();
            restored.state.transport.sample_rate = 96_000;
            restored.send_command(EngineCommand::SetSampleRate(96_000));
            restored.rebuild_from_loaded_project(loaded);
            let channel = restored.state.find_track(track).unwrap();
            assert_eq!(
                channel
                    .effects
                    .iter()
                    .map(|effect| effect.id)
                    .collect::<Vec<_>>(),
                before
                    .effects
                    .iter()
                    .map(|effect| effect.id)
                    .collect::<Vec<_>>()
            );
            assert_eq!(channel.effects[1].params, SOUND, "{owner:?} .{extension}");
            assert_eq!(channel.effects[1].bypass, before.effects[1].bypass);
            assert_eq!(
                (channel.gain, channel.pan, &channel.sends),
                (before.gain, before.pan, &before.sends),
                "{owner:?} .{extension}: mixer state"
            );
            assert_eq!(channel.effects[1].sidechains, before.effects[1].sidechains);
            assert_eq!(
                channel.effects[1].inactive_sidechains,
                before.effects[1].inactive_sidechains
            );
            assert_eq!(channel.effects[1].latency_samples, Some(0));
            assert_eq!(
                restored.state.arrange_content(track).unwrap().automation,
                automation
            );
            assert_eq!(
                restored.cmd_tx.prepared_effect_sample_rate(track, effect),
                Some(96_000)
            );
            restored.state.project.history = Default::default();
            let _ = restored.update(Message::set_effect_param(
                track,
                effect,
                param::DRIVE_DB,
                3.0,
            ));
            let _ = restored.update(Message::Project(ProjectMsg::Undo));
            assert_eq!(
                restored.state.find_track(track).unwrap().effects[1].params,
                SOUND
            );
            let _ = restored.update(Message::Project(ProjectMsg::Redo));
            assert_eq!(
                restored.state.find_track(track).unwrap().effects[1].params[param::DRIVE_DB],
                3.0
            );
            assert_eq!(
                restored.state.find_track(track).unwrap().effects[1].id,
                effect
            );
            assert_eq!(
                restored.state.arrange_content(track).unwrap().automation,
                automation
            );
            assert_eq!(
                restored.cmd_tx.prepared_effect_sample_rate(track, effect),
                Some(96_000)
            );
        }
    }
}

#[tokio::test]
async fn real_bitcrush_file_failures_and_stale_completions_never_partially_apply_a_sound() {
    let (mut app, track, effect) = owner_app(Owner::Track);
    let before = serde_json::to_value(app.project_for_save()).unwrap();
    let factory = device_presets::factory_presets(EffectType::Bitcrush).remove(1);
    let base = serde_json::to_value(&factory).unwrap();
    let mut invalid = Vec::new();
    for key in KEYS {
        let mut missing = base.clone();
        missing["parameters"].as_object_mut().unwrap().remove(key);
        invalid.push((format!("missing-{key}"), missing));
        let mut renamed = base.clone();
        let value = renamed["parameters"]
            .as_object_mut()
            .unwrap()
            .remove(key)
            .unwrap();
        renamed["parameters"]["unknown_key"] = value;
        invalid.push((format!("unknown-{key}"), renamed));
    }
    for key in ["rounding", "dither", "post_lp_enabled"] {
        let mut value = base.clone();
        value["parameters"][key] = serde_json::json!(0.5);
        invalid.push((format!("fractional-{key}"), value));
    }
    let mut version = base.clone();
    version["format_version"] = serde_json::json!(999);
    invalid.push(("version".into(), version));
    let mut wrong = base.clone();
    wrong["kind"] = serde_json::to_value(vibez_core::device_preset::DevicePresetKind::Effect(
        EffectType::Gain,
    ))
    .unwrap();
    invalid.push(("wrong-kind".into(), wrong));
    let mut range = base.clone();
    range["parameters"]["bits"] = serde_json::json!(17.0);
    invalid.push(("range".into(), range));
    let folder = tempfile::tempdir().unwrap();
    for (name, document) in invalid {
        let path = folder.path().join(format!("{name}.vzpreset"));
        std::fs::write(&path, serde_json::to_vec(&document).unwrap()).unwrap();
        let result = vibez_project::device_preset::load_device_preset(&path)
            .map(Some)
            .map_err(|error| error.to_string());
        if name.starts_with("fractional-") {
            assert!(
                result.is_ok(),
                "fractional choices must reach App validation"
            );
        }
        let request = app.begin_device_preset_load(track, effect).unwrap();
        let _ = app.update(Message::DevicePreset(DevicePresetMsg::Loaded(
            request, result,
        )));
        assert_eq!(
            serde_json::to_value(app.project_for_save()).unwrap(),
            before,
            "{name}"
        );
        assert!(!app.state.project.history.can_undo());
        assert!(!app.state.project.dirty);
    }
    let request = app.begin_device_preset_load(track, effect).unwrap();
    let _ = app.update(Message::DevicePreset(DevicePresetMsg::Loaded(
        request,
        Ok(None),
    )));
    let stale = app.begin_device_preset_load(track, effect).unwrap();
    let current = app.begin_device_preset_load(track, effect).unwrap();
    let _ = app.update(Message::DevicePreset(DevicePresetMsg::Loaded(
        stale,
        Ok(Some(factory.clone())),
    )));
    let _ = app.update(Message::DevicePreset(DevicePresetMsg::Loaded(
        current,
        Ok(None),
    )));
    assert_eq!(
        serde_json::to_value(app.project_for_save()).unwrap(),
        before
    );
    let stale = app.begin_device_preset_load(track, effect).unwrap();
    for value in [5.0, SOUND[param::BITS]] {
        let _ = app.update(Message::set_effect_param(track, effect, param::BITS, value));
    }
    app.state.project.history = Default::default();
    app.state.project.dirty = false;
    let _ = app.update(Message::DevicePreset(DevicePresetMsg::Loaded(
        stale,
        Ok(Some(factory)),
    )));
    assert_eq!(
        serde_json::to_value(app.project_for_save()).unwrap(),
        before
    );
    let request = app.begin_device_preset_save(track, effect).unwrap();
    let result = tasks::save_device_preset_at(
        folder.path().join("absent/failed.vzpreset"),
        request.preset.clone(),
    )
    .await
    .map(Some);
    assert!(result.is_err());
    app.finish_device_preset_save(request, result);
    let request = app.begin_device_preset_save(track, effect).unwrap();
    app.finish_device_preset_save(request, Ok(None));
    assert_eq!(
        serde_json::to_value(app.project_for_save()).unwrap(),
        before
    );
    assert!(!app.state.project.history.can_undo());
    assert!(!app.state.project.dirty);
}

#[tokio::test]
async fn legacy_three_value_projects_hydrate_edit_and_undo_appended_controls_on_every_owner() {
    use crate::domains::device_controls::DeviceControlMsg;

    const LEGACY: [f32; 3] = [8.75, 4.49, 0.37];
    const HYDRATED: [f32; 9] = [8.75, 4.49, 0.37, 0.0, 0.0, 0.0, 0.0, 0.0, 12_000.0];
    let folder = tempfile::Builder::new()
        .prefix(".bitcrush-legacy-project-")
        .tempdir_in(std::env::current_dir().unwrap())
        .unwrap();
    for owner in [Owner::Track, Owner::Return, Owner::Master] {
        let (mut app, track, effect) = owner_app(owner);
        app.state.find_track_mut(track).unwrap().effects[1].params = LEGACY.to_vec();
        for extension in ["vibez", "vzp"] {
            let path = folder.path().join(format!("Legacy-{owner:?}.{extension}"));
            super::super::super::save_project_async(path.clone(), None, app.project_for_save())
                .await
                .unwrap();
            let loaded = super::super::super::load_project_async(path, None)
                .await
                .unwrap();
            let saved_channel = loaded
                .project
                .tracks
                .iter()
                .chain(&loaded.project.buses)
                .chain(loaded.project.master.iter())
                .find(|channel| channel.id == track)
                .unwrap();
            assert_eq!(
                saved_channel
                    .effects
                    .iter()
                    .find(|slot| slot.id == effect)
                    .unwrap()
                    .params,
                LEGACY,
                "{owner:?} .{extension}: the file really contains only three values"
            );
            let mut restored = super::super::super::test_support::app();
            restored.state.transport.sample_rate = 96_000;
            restored.send_command(EngineCommand::SetSampleRate(96_000));
            restored.rebuild_from_loaded_project(loaded);
            let target = &restored.state.find_track(track).unwrap().effects[1];
            assert_eq!(target.id, effect);
            assert_eq!(
                target.params, HYDRATED,
                "{owner:?} .{extension}: frozen neutral defaults"
            );
            assert!(target.bypass);
            assert_eq!(target.latency_samples, Some(0));
            assert_eq!(
                restored.cmd_tx.prepared_effect_sample_rate(track, effect),
                Some(96_000)
            );
            assert!(
                !restored.state.project.dirty,
                "merely opening an old sound is not an edit"
            );
            assert!(!restored.state.project.history.can_undo());
            assert!(restored.state.project.history.redo.is_empty());
            for index in 3..9 {
                let presentation = restored
                    .device_parameter_presentation(track, effect, index)
                    .unwrap();
                assert!(
                    presentation.is_editable(),
                    "{owner:?} .{extension}: appended index{index}"
                );
                assert!(presentation.range.is_some());
            }
            for (index, shown, unit) in [
                (param::DRIVE_DB, "0.00", "dB"),
                (param::JITTER, "0", "%"),
                (param::POST_LP_HZ, "12.00k", "Hz"),
            ] {
                let presentation = restored
                    .device_parameter_presentation(track, effect, index)
                    .unwrap();
                assert_eq!(presentation.value.as_deref(), Some(shown));
                assert_eq!(presentation.unit, unit);
            }
            assert!(!restored.state.project.dirty);
            assert!(!restored.state.project.history.can_undo());
            for (index, value, numeric) in [
                (param::DRIVE_DB, 6.0, Some("6 dB")),
                (param::ROUNDING, 2.0, None),
                (param::DITHER, 1.0, None),
                (param::JITTER, 0.35, Some("35 %")),
                (param::POST_LP_ENABLED, 1.0, None),
                (param::POST_LP_HZ, 3500.0, Some("3.5 kHz")),
            ] {
                if let Some(text) = numeric {
                    let _ = restored.update(Message::DeviceControl(DeviceControlMsg::Input(
                        track,
                        effect,
                        index,
                        text.into(),
                    )));
                    let _ = restored.update(Message::DeviceControl(DeviceControlMsg::Submit(
                        track, effect, index,
                    )));
                    assert!(restored.device_controls[&effect].errors.is_empty());
                } else {
                    let _ = restored.update(Message::set_effect_param(track, effect, index, value));
                }
                let mut edited = HYDRATED;
                edited[index] = value;
                assert_eq!(
                    restored.state.find_track(track).unwrap().effects[1].params,
                    edited,
                    "{owner:?} .{extension}: appended index{index} must be editable"
                );
                assert_eq!(restored.state.project.history.undo.len(), 1);
                let _ = restored.update(Message::Project(ProjectMsg::Undo));
                assert_eq!(
                    restored.state.find_track(track).unwrap().effects[1].params,
                    HYDRATED
                );
                assert!(!restored.state.project.history.can_undo());
            }
            let factory = device_presets::factory_presets(EffectType::Bitcrush).remove(5);
            let expected: Vec<_> = KEYS.iter().map(|key| factory.parameters()[*key]).collect();
            let request = restored.begin_device_preset_load(track, effect).unwrap();
            let _ = restored.update(Message::DevicePreset(DevicePresetMsg::Loaded(
                request,
                Ok(Some(factory)),
            )));
            assert_eq!(
                restored.state.find_track(track).unwrap().effects[1].params,
                expected
            );
            assert_eq!(restored.state.project.history.undo.len(), 1);
            assert!(restored.state.project.history.redo.is_empty());
            let _ = restored.update(Message::Project(ProjectMsg::Undo));
            let target = &restored.state.find_track(track).unwrap().effects[1];
            assert_eq!(target.id, effect);
            assert_eq!(
                target.params, HYDRATED,
                "{owner:?} .{extension}: preset Undo restores old sound and defaults"
            );
            assert!(target.bypass);
            assert_eq!(
                restored.cmd_tx.prepared_effect_sample_rate(track, effect),
                Some(96_000)
            );
            assert!(!restored.state.project.history.can_undo());
        }
    }
}
