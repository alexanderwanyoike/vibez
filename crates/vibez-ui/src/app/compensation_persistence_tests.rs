use super::*;
use vibez_core::{
    effect::{EffectInfo, EffectType},
    id::EffectId,
    track::TrackInfo,
};

#[tokio::test]
async fn project_save_and_hydration_services_preserve_monitoring_and_bypass() {
    let directory = tempfile::tempdir().unwrap();
    for extension in ["vibez", "vzp"] {
        let mut project = vibez_project::Project {
            reduced_latency_monitoring: true,
            ..Default::default()
        };
        let mut track = TrackInfo::new("Track");
        let effect = EffectId::new();
        track.effects.push(EffectInfo {
            inactive_sidechains: vec![],
            id: effect,
            effect_type: EffectType::Gain,
            bypass: true,
            params: vec![0.75],
            plugin: None,
            sidechains: vec![],
        });
        project.tracks.push(track);
        let path = directory.path().join(format!("monitoring.{extension}"));
        let saved = save_project_async(path.clone(), None, project)
            .await
            .unwrap();
        assert!(saved.project.reduced_latency_monitoring);
        let loaded = load_project_async(path, None).await.unwrap();
        assert!(loaded.project.reduced_latency_monitoring);
        assert_eq!(loaded.project.tracks[0].effects[0].id, effect);
        assert!(loaded.project.tracks[0].effects[0].bypass);
        assert!(loaded.unresolved_clips.is_empty());
    }
    let legacy = directory.path().join("legacy.vibez");
    std::fs::write(
        &legacy,
        r#"{"name":"Legacy","bpm":120,"sample_rate":48000,"tracks":[],"clips":[],"note_clips":[]}"#,
    )
    .unwrap();
    let incoming = load_project_async(legacy, None).await.unwrap();
    assert!(!incoming.project.reduced_latency_monitoring);
}
