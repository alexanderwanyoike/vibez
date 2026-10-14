//! Verifies Bitcrush sound files retain exact stored controls through the shared App flow.

use super::*;
use crate::domains::devices::DevicesMsg;
use crate::state::ProjectTrack;
use vibez_core::effect::EffectType;

fn bitcrush_app() -> (App, TrackId, EffectId) {
    let mut app = super::super::test_support::app();
    let channel = ProjectTrack::new(TrackId::new(), "Bitcrush".into(), 3);
    let track = channel.id;
    Arc::make_mut(&mut app.state.project_tracks)
        .tracks
        .push(channel);
    let _ = app.update(Message::Devices(DevicesMsg::AddEffect(
        track,
        EffectType::Bitcrush,
    )));
    let target = &mut app.state.find_track_mut(track).unwrap().effects[0];
    target.params = vec![8.75, 4.49, 0.37, 6.0, 2.0, 1.0, 0.35, 1.0, 3500.0];
    target.bypass = true;
    let effect = target.id;
    app.state.project.history = Default::default();
    app.state.project.dirty = false;
    (app, track, effect)
}

#[tokio::test]
async fn bitcrush_user_file_roundtrips_exact_sound_and_load_is_one_undoable_edit() {
    let (mut app, track, effect) = bitcrush_app();
    let original = app.state.find_track(track).unwrap().effects[0].clone();
    let request = app
        .begin_device_preset_save(track, effect)
        .expect("native Bitcrush must support user preset files");
    assert_eq!(request.preset.parameters()["bits"], 8.75);
    assert_eq!(request.preset.parameters()["sr_reduce"], 4.49);
    assert_eq!(request.preset.parameters().len(), 9);
    let folder = tempfile::tempdir().unwrap();
    let path = folder.path().join("My Bitcrush.vzpreset");
    let saved = tasks::save_device_preset_at(path.clone(), request.preset.clone())
        .await
        .unwrap();
    app.finish_device_preset_save(request, Ok(Some(saved.clone())));
    assert_eq!(
        app.state.find_track(track).unwrap().effects[0].params,
        original.params
    );
    assert!(!app.state.project.history.can_undo());
    assert!(!app.state.project.dirty);

    let _ = app.update(Message::set_effect_param(track, effect, 0, 5.0));
    let before_load = app.state.find_track(track).unwrap().effects[0]
        .params
        .clone();
    app.state.project.history = Default::default();
    app.state.project.dirty = false;
    let loaded = vibez_project::device_preset::load_device_preset(&path).unwrap();
    assert_eq!(loaded, saved);
    let request = app.begin_device_preset_load(track, effect).unwrap();
    let _ = app.finish_device_preset_load(request, Ok(loaded.clone()));
    let destination = &app.state.find_track(track).unwrap().effects[0];
    assert_eq!(destination.id, original.id);
    assert_eq!(destination.bypass, original.bypass);
    assert_eq!(destination.params, original.params);
    assert_eq!(destination.latency_samples, Some(0));
    assert!(app.state.project.dirty);

    let request = app.begin_device_preset_load(track, effect).unwrap();
    let _ = app.finish_device_preset_load(request, Ok(loaded));
    let snapshot = app
        .state
        .project
        .history
        .pop_undo()
        .expect("one sound edit");
    assert!(!app.state.project.history.can_undo());
    app.apply_snapshot(snapshot);
    assert_eq!(
        app.state.find_track(track).unwrap().effects[0].params,
        before_load
    );
}

#[test]
fn bitcrush_preset_rejects_fractional_choices_before_any_sound_or_history_change() {
    use vibez_core::device_preset::DevicePresetKind;
    let (mut app, track, effect) = bitcrush_app();
    let before = app.state.find_track(track).unwrap().effects[0]
        .params
        .clone();
    let factory = crate::domains::device_presets::factory_presets(EffectType::Bitcrush)
        .into_iter()
        .find(|preset| preset.name() == "Crunchy 12")
        .unwrap();
    for key in ["rounding", "dither", "post_lp_enabled"] {
        let mut parameters = factory.parameters().clone();
        parameters.insert(key.into(), 0.5);
        let invalid = DevicePreset::new(
            DevicePresetKind::Effect(EffectType::Bitcrush),
            "Invalid",
            parameters,
        )
        .unwrap();
        let request = app.begin_device_preset_load(track, effect).unwrap();
        let _ = app.finish_device_preset_load(request, Ok(invalid));
        assert_eq!(
            app.state.find_track(track).unwrap().effects[0].params,
            before,
            "fractional {key} changed the sound before complete validation"
        );
        assert!(!app.state.project.history.can_undo());
        assert!(!app.state.project.dirty);
    }
}
