//! Characterizes three-value Delay restoration through real legacy and container project files.

use super::*;
use vibez_core::{
    effect::{EffectInfo, EffectType},
    id::EffectId,
    track::TrackInfo,
};
use vibez_dsp::factory::create_effect_with_params;

#[tokio::test]
async fn three_value_delay_files_restore_stored_units_identity_and_original_sound() {
    let directory = tempfile::tempdir().unwrap();
    let parameters = [125.0_f32, 0.625, 0.37];
    let mut observations = Vec::new();
    for extension in ["vibez", "vzp"] {
        let mut project = vibez_project::Project {
            sample_rate: 48_000,
            ..Default::default()
        };
        let mut track = TrackInfo::new("Legacy Delay");
        let track_id = track.id;
        let effect_id = EffectId::new();
        track.effects.push(EffectInfo {
            id: effect_id,
            effect_type: EffectType::Delay,
            bypass: false,
            params: parameters.to_vec(),
            plugin: None,
            sidechains: vec![],
            inactive_sidechains: vec![],
        });
        project.tracks.push(track);
        let path = directory.path().join(format!("legacy-delay.{extension}"));
        save_project_async(path.clone(), None, project)
            .await
            .unwrap();
        let saved_bytes = std::fs::metadata(&path).unwrap().len();
        let loaded = load_project_async(path, None).await.unwrap();
        let stored = &loaded.project.tracks[0].effects[0];
        assert_eq!(stored.id, effect_id);
        assert_eq!(stored.effect_type, EffectType::Delay);
        assert_eq!(stored.params.len(), 3);
        assert_eq!(stored.params, parameters);
        let mut app = super::test_support::app();
        app.state.transport.sample_rate = 48_000;
        app.send_command(EngineCommand::SetSampleRate(48_000));
        app.rebuild_from_loaded_project(loaded);
        let effect = app
            .state
            .find_track(track_id)
            .unwrap()
            .effects
            .iter()
            .find(|effect| effect.id == effect_id)
            .unwrap();
        assert_eq!(effect.effect_type, EffectType::Delay);
        assert!(!effect.bypass);
        assert!(effect.plugin_ref.is_none());
        assert!(effect.plugin_name.is_none());
        // Loaded legacy values remain exact; descriptor defaults now append fourteen targets.
        assert_eq!(&effect.params[..parameters.len()], &parameters);
        assert_eq!(effect.descriptors[0].unit, "ms");
        assert_eq!(effect.latency_samples, Some(0));
        let mut processor = create_effect_with_params(EffectType::Delay, 48_000.0, &effect.params);
        assert_eq!(processor.activation_sample_rate(), Some(48_000));
        assert_eq!(processor.latency_samples(), 0);
        for (index, value) in parameters.into_iter().enumerate() {
            assert_eq!(processor.get_param(index).to_bits(), value.to_bits());
        }
        let mut actual = vec![0.0; 12_001 * 2];
        actual[..2].copy_from_slice(&[0.75, -0.25]);
        processor.process(&mut actual, 2);
        let mut expected = vec![0.0; actual.len()];
        for (channel, amplitude) in [0.75, -0.25].into_iter().enumerate() {
            expected[channel] = amplitude * (1.0_f32 - parameters[2]);
            expected[6000 * 2 + channel] = amplitude * parameters[2];
            expected[12_000 * 2 + channel] = (amplitude * parameters[1]) * parameters[2];
        }
        assert_eq!(actual, expected, "format={extension}");
        observations.push(serde_json::json!({
            "format": extension,
            "saved_bytes": saved_bytes,
            "track_id": track_id.raw(),
            "effect_id": effect_id.raw(),
            "loaded_parameter_bits": effect.params.iter().map(|value| value.to_bits()).collect::<Vec<_>>(),
            "parameter_scope": "Original three stored values preserved; loaded vector includes appended defaults",
            "activation_rate": processor.activation_sample_rate(),
            "latency_samples": processor.latency_samples(),
            "dry_bits": [actual[0].to_bits(), actual[1].to_bits()],
            "first_repeat_frame": 6000,
            "first_repeat_bits": [actual[12_000].to_bits(), actual[12_001].to_bits()],
            "second_repeat_frame": 12_000,
            "second_repeat_bits": [actual[24_000].to_bits(), actual[24_001].to_bits()],
        }));
    }
    println!(
        "DELAY_LEGACY_RECORD {}",
        serde_json::json!({
            "case": "real_three_value_project_restore",
            "execution_head": std::env::var("VIBEZ_VERIFICATION_HEAD")
                .unwrap_or_else(|_| "UNLABELLED".into()),
            "original_delay_blob": "ec654ec086255e377d38b6095adceb28bf15696a",
            "observed": observations,
        })
    );
}
