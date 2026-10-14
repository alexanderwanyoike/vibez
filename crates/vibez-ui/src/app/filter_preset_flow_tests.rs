//! Verifies Filter preset files, legacy hydration and one-step application undo.

use super::*;
use crate::domains::devices::DevicesMsg;
use crate::state::ProjectTrack;
use vibez_core::effect::EffectType;
use vibez_dsp::filter::param;

fn filter_app() -> (App, TrackId, EffectId) {
    let mut app = super::super::test_support::app();
    let track = ProjectTrack::new(TrackId::new(), "Filter audio".into(), 3);
    let id = track.id;
    Arc::make_mut(&mut app.state.project_tracks)
        .tracks
        .push(track);
    let _ = app.update(Message::Devices(DevicesMsg::AddEffect(
        id,
        EffectType::Filter,
    )));
    let effect = app.state.find_track(id).unwrap().effects[0].id;
    app.state.project.history = Default::default();
    app.state.project.dirty = false;
    (app, id, effect)
}

#[tokio::test]
async fn factory_files_load_into_the_same_instance_and_undo_once() {
    let (mut app, track, effect) = filter_app();
    let folder = tempfile::tempdir().unwrap();
    for factory in crate::domains::device_presets::factory_presets(EffectType::Filter) {
        let before = app.state.find_track(track).unwrap().effects[0].clone();
        let path = folder.path().join("Saved Filter.vzpreset");
        let saved = tasks::save_device_preset_at(path.clone(), factory)
            .await
            .unwrap();
        let loaded = vibez_project::device_preset::load_device_preset(&path).unwrap();
        assert_eq!(saved, loaded);
        let request = app.begin_device_preset_load(track, effect).unwrap();
        let _ = app.finish_device_preset_load(request, Ok(loaded.clone()));
        let after = &app.state.find_track(track).unwrap().effects[0];
        assert_eq!(after.id, before.id);
        assert_eq!(after.bypass, before.bypass);
        assert_eq!(after.sidechains, before.sidechains);
        assert!(
            crate::domains::device_presets::prepare_updates(after, &loaded)
                .unwrap()
                .is_empty()
        );
        let snapshot = app.state.project.history.pop_undo().unwrap();
        assert!(!app.state.project.history.can_undo());
        app.apply_snapshot(snapshot);
        assert_eq!(
            app.state.find_track(track).unwrap().effects[0].params,
            before.params
        );
    }
}

#[test]
fn stale_or_wrong_device_load_cannot_change_filter_values() {
    let (mut app, track, effect) = filter_app();
    let before = app.state.find_track(track).unwrap().effects[0]
        .params
        .clone();
    let request = app.begin_device_preset_load(track, effect).unwrap();
    let wrong = crate::domains::device_presets::factory_presets(EffectType::Gain)[0].clone();
    let _ = app.finish_device_preset_load(request, Ok(wrong));
    assert_eq!(
        app.state.find_track(track).unwrap().effects[0].params,
        before
    );
    assert!(!app.state.project.dirty);
    let request = app.begin_device_preset_load(track, effect).unwrap();
    let _ = app.update(Message::set_effect_param(
        track,
        effect,
        param::CUTOFF,
        800.0,
    ));
    let stale = crate::domains::device_presets::factory_presets(EffectType::Filter)[0].clone();
    let _ = app.finish_device_preset_load(request, Ok(stale));
    assert_eq!(
        app.state.find_track(track).unwrap().effects[0].params[param::CUTOFF],
        800.0
    );
}
