//! Strict offline rendering rejects actual native processing failures.

#[allow(dead_code)]
mod support;

#[test]
fn a_loadable_processing_failure_is_not_successful_silent_audio_and_returns_its_owner() {
    use vibez_core::{
        effect::{EffectInfo, EffectType, PluginDeviceInfo},
        track::TrackInfo,
    };
    use vibez_engine::render::{
        render_offline_with_plugins, BounceMode, BounceRequest, OfflinePlugins,
    };
    use vibez_plugin_host::{
        vst3_host::{instance::Vst3PluginInstance, scanner::scan_vst3},
        wrappers::effect::PluginEffectWrapper,
    };
    let fixture = support::Fixture::new();
    let classes = scan_vst3(&fixture.vst3).unwrap();
    let plugin =
        Vst3PluginInstance::load(&fixture.vst3, &classes[5].id.uid, false, 48000.0, 512).unwrap();
    let mut track = TrackInfo::new("Bass");
    let effect = vibez_core::id::EffectId::new();
    track.effects.push(EffectInfo {
        id: effect,
        effect_type: EffectType::Gain,
        params: vec![],
        bypass: false,
        sidechains: vec![],
        inactive_sidechains: vec![],
        plugin: Some(PluginDeviceInfo {
            format: "vst3".into(),
            uid: classes[5].id.uid.clone(),
            path: fixture.vst3.clone(),
            name: "Processing error".into(),
            state_b64: None,
        }),
    });
    let request = BounceRequest {
        tracks: vec![track],
        master: None,
        buses: vec![],
        audio_clips: vec![],
        note_clips: vec![],
        clip_audio: Default::default(),
        sampler_audio: Default::default(),
        drum_pad_audio: Default::default(),
        mode: BounceMode::Master,
        range_samples: (0, 512),
        bpm: 120.0,
        sample_rate: 48000,
        swing: Default::default(),
    };
    let mut plugins = OfflinePlugins::default();
    plugins
        .effects
        .insert(effect, Box::new(PluginEffectWrapper::new(Box::new(plugin))));
    let error = render_offline_with_plugins(&request, &mut plugins, |_| {})
        .err()
        .expect("failed native DSP must reject the render");
    assert!(error.contains("Processing error"));
    assert!(error.contains("Bass"));
    assert!(error.contains("VST3 process returned failure"));
    assert!(
        plugins.effects.contains_key(&effect),
        "failed renderer must return the isolated owner for main-thread teardown"
    );
}
