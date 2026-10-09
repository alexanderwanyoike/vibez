use super::*;
use vibez_core::{
    effect::{EffectInfo, EffectType},
    id::EffectId,
};

#[test]
fn new_and_legacy_projects_default_to_full_compensation() {
    assert!(!Project::default().reduced_latency_monitoring);
    let legacy =
        r#"{"name":"Legacy","bpm":120,"sample_rate":48000,"tracks":[],"clips":[],"note_clips":[]}"#;
    let project: Project = serde_json::from_str(legacy).unwrap();
    assert!(!project.reduced_latency_monitoring);
}

#[test]
fn legacy_json_and_current_container_roundtrip_monitoring_mode_and_host_bypass() {
    let directory = std::env::temp_dir().join(format!(
        "vibez-pdc-project-test-{}-{}",
        std::process::id(),
        EffectId::new().raw()
    ));
    std::fs::create_dir_all(&directory).unwrap();
    for enabled in [false, true] {
        let mut project = Project {
            reduced_latency_monitoring: enabled,
            ..Default::default()
        };
        let mut track = TrackInfo::new("Bass");
        let effect_id = EffectId::new();
        track.effects.push(EffectInfo {
            id: effect_id,
            effect_type: EffectType::Gain,
            bypass: true,
            params: vec![0.75],
            plugin: None,
            sidechains: vec![],
        });
        project.tracks.push(track);
        let json_path = directory.join(format!("mode-{enabled}.vibez"));
        project.save_to_file(&json_path).unwrap();
        let reopened = Project::load_from_file(&json_path).unwrap();
        assert_eq!(reopened.reduced_latency_monitoring, enabled);
        assert!(reopened.tracks[0].effects[0].bypass);
        assert_eq!(reopened.tracks[0].effects[0].id, effect_id);
        let path = directory.join(format!("mode-{enabled}.vzp"));
        project_format_v1::save_project_v1(&path, None, project).unwrap();
        let reopened = project_format_v1::ProjectContainer::open(&path)
            .unwrap()
            .load_document()
            .unwrap();
        assert_eq!(reopened.project.reduced_latency_monitoring, enabled);
        assert!(reopened.project.tracks[0].effects[0].bypass);
        let json = serde_json::to_value(&reopened.project).unwrap();
        assert_eq!(json["reduced_latency_monitoring"], enabled);
        assert!(json["tracks"][0]["effects"][0]
            .get("latency_samples")
            .is_none());
    }
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn switching_back_to_a_legacy_project_does_not_inherit_the_previous_mode() {
    let enabled: Project = serde_json::from_value(
        serde_json::to_value(Project {
            reduced_latency_monitoring: true,
            ..Default::default()
        })
        .unwrap(),
    )
    .unwrap();
    assert!(enabled.reduced_latency_monitoring);
    let mut legacy = serde_json::to_value(Project::default()).unwrap();
    legacy
        .as_object_mut()
        .unwrap()
        .remove("reduced_latency_monitoring");
    let opened: Project = serde_json::from_value(legacy).unwrap();
    assert!(!opened.reduced_latency_monitoring);
}
