use super::*;
use vibez_core::{
    effect::{EffectInfo, EffectType},
    id::{EffectId, TrackId},
    track::TrackInfo,
};

struct FailedRestart;
impl vibez_dsp::effect::AudioEffect for FailedRestart {
    fn reconfigure_on_main_thread(&mut self) -> Result<(), String> {
        Err("Rejected restart".into())
    }
    fn latency_samples(&self) -> u32 {
        0
    }
    fn effect_type(&self) -> EffectType {
        EffectType::Gain
    }
    fn param_descriptors(&self) -> &'static [vibez_core::effect::ParamDescriptor] {
        &[]
    }
    fn set_param(&mut self, _: usize, _: f32) -> bool {
        false
    }
    fn get_param(&self, _: usize) -> f32 {
        0.0
    }
    fn process(&mut self, _: &mut [f32], _: usize) {}
    fn reset(&mut self) {}
}

#[test]
fn failed_restart_cannot_keep_a_valid_latency_readout() {
    let mut app = super::test_support::app();
    let mut track = crate::state::ProjectTrack::new(TrackId::new(), "Track".into(), 0);
    let id = EffectId::new();
    track.effects.push(crate::state::UiEffect {
        id,
        effect_type: EffectType::Gain,
        bypass: false,
        params: vec![],
        descriptors: &[],
        plugin_name: Some("Failed device".into()),
        has_plugin_gui: false,
        plugin_ref: None,
        sidechains: vec![],
        inactive_sidechains: vec![],
        external_inputs: vec![],
        latency_samples: Some(521),
    });
    let track_id = track.id;
    Arc::make_mut(&mut app.state.project_tracks)
        .tracks
        .push(track);
    let (producer, mut consumer) = rtrb::RingBuffer::new(8);
    app.cmd_tx = crate::domains::EngineCommandQueue::new(producer);
    app.send_command(EngineCommand::AddPluginEffect {
        track_id,
        effect_id: id,
        effect: Box::new(FailedRestart),
        position: None,
    });
    let EngineCommand::AddPluginEffect { effect, .. } = consumer.pop().unwrap() else {
        panic!("device creation")
    };
    app.reconfigure_device_timing(
        vibez_engine::engine::reconfiguration::DeviceReconfiguration::Effect {
            handoff_id: 1,
            reserved_effects: Vec::new(),
            track_id,
            position: 0,
            slot: vibez_engine::mixer::EffectSlot {
                id,
                effect,
                bypass: false,
            },
        },
    );
    assert_eq!(
        app.state.project_tracks.tracks[0].effects[0].latency_samples,
        None
    );
    assert!(app
        .state
        .status_text
        .contains("Failed device: Rejected restart"));
}

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
