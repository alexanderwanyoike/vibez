//! Verifies legacy and modern Drive projects restore declared native delay on every channel owner.

use super::*;
use vibez_core::effect::EffectType;

#[test]
fn loaded_drive_declares_real_latency_and_preserves_legacy_or_explicit_modern_character() {
    for modern in [false, true] {
        let params = if modern {
            serde_json::json!([0.35, 0.5, 1, 1, 1, 2, 0, 80, 0, 12000])
        } else {
            serde_json::json!([0.35, 0.5, 1, 1])
        };
        let effect =
            |id| serde_json::json!({"id":id,"effect_type":"Drive","bypass":false,"params":params});
        let channel = |id, name, effect_id| serde_json::json!({"id":id,"name":name,"color_index":3,"gain":1,"pan":0,"mute":false,"solo":false,"effects":[effect(effect_id)]});
        let project: vibez_project::Project = serde_json::from_value(serde_json::json!({
            "name":"Drive latency", "bpm":120,"sample_rate":48000,
            "tracks":[channel(100,"Track",101)],"buses":[channel(200,"Return",201)],
            "master":channel(0,"Master",301),"clips":[]
        }))
        .unwrap();
        let mut app = super::test_support::app();
        let path = std::path::PathBuf::from("drive-latency.vibez");
        app.state.project.recent_project_paths = vec![path.clone()];
        let _ = app.update_and_refresh_clips(Message::ProjectLoaded {
            path: path.clone(),
            result: Box::new(Ok(ProjectLoadResult {
                path,
                project,
                clips: vec![],
                unresolved_clips: vec![],
                sampler_samples: vec![],
                drum_rack_pad_samples: vec![],
                warnings: vec![],
            })),
        });
        for channel in app
            .state
            .project_tracks
            .tracks
            .iter()
            .chain(&app.state.project_tracks.buses)
            .chain(std::iter::once(&app.state.project_tracks.master))
        {
            let drive = channel
                .effects
                .iter()
                .find(|effect| effect.effect_type == EffectType::Drive)
                .unwrap();
            assert_eq!(drive.latency_samples, Some(64), "{}", channel.name);
            assert_eq!(drive.params[4], if modern { 1.0 } else { 0.0 });
            assert_eq!(&drive.params[..4], &[0.35, 0.5, 1.0, 1.0]);
            assert_eq!(drive.params.len(), vibez_dsp::drive::param::COUNT);
        }
    }
}
