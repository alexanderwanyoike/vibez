//! Verifies legacy Gain documents restore editable neutral stereo controls.

use super::*;
use vibez_core::{effect::{EffectInfo, EffectType}, id::EffectId, track::TrackInfo};

#[tokio::test]
async fn legacy_gain_project_keeps_its_level_and_accepts_new_stereo_controls() {
    let directory = tempfile::Builder::new()
        .prefix(".gain-project-test-")
        .tempdir_in(std::env::current_dir().unwrap())
        .unwrap();
    for extension in ["vibez", "vzp"] {
        let mut project = vibez_project::Project::default();
        let mut track = TrackInfo::new("Legacy Gain");
        let track_id = track.id;
        let effect_id = EffectId::new();
        track.effects.push(EffectInfo {
            id: effect_id,
            effect_type: EffectType::Gain,
            bypass: false,
            params: vec![0.5],
            plugin: None,
            sidechains: vec![],
            inactive_sidechains: vec![],
        });
        project.tracks.push(track);
        let path = directory.path().join(format!("legacy.{extension}"));
        save_project_async(path.clone(), None, project)
            .await
            .unwrap();
        let loaded = load_project_async(path, None).await.unwrap();
        let mut app = super::test_support::app();
        app.rebuild_from_loaded_project(loaded);
        let effect = app
            .state
            .find_track(track_id)
            .unwrap()
            .effects
            .iter()
            .find(|effect| effect.id == effect_id)
            .unwrap();
        assert_eq!(effect.params, vec![0.5, 0.0, 0.0, 0.0, 0.0]);
        let _ = app.update(Message::set_effect_param(track_id, effect_id, 1, -6.0));
        let effect = app
            .state
            .find_track(track_id)
            .unwrap()
            .effects
            .iter()
            .find(|effect| effect.id == effect_id)
            .unwrap();
        assert_eq!(effect.params[0], 0.5);
        assert_eq!(effect.params[1], -6.0);
    }
}

