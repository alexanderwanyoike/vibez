#[allow(dead_code)]
mod support;

use std::{collections::HashMap, sync::Arc};
use vibez_core::{
    audio_buffer::DecodedAudio,
    effect::{EffectInfo, EffectType, PluginDeviceInfo},
    id::{ClipId, EffectId},
    midi::{MidiNote, NoteClipInfo, TrackKind},
    routing::{SidechainAssignment, SourceTap},
    track::{ClipInfo, TrackInfo},
};
use vibez_engine::render::{
    render_offline_with_plugins, BounceMode, BounceRequest, OfflinePlugins,
};
use vibez_plugin_host::{PluginEffectWrapper, PluginInstrumentWrapper};

fn render(
    fixture: &support::Fixture,
    source_format: &str,
    receiver_format: &str,
    range: (u64, u64),
    report: u32,
) -> Vec<f32> {
    let mut receiver = fixture.load(receiver_format, 512);
    let mut source = fixture.load_instrument(source_format, 512);
    for (plugin, actual, reported, probe) in [
        (&mut receiver, 521, report, 1u32),
        (&mut source, 137, 137, 0),
    ] {
        let state: Vec<_> = [actual, reported, probe]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect();
        assert!(plugin.load_state(&state));
        plugin.reconfigure_on_main_thread().unwrap();
    }
    let input = receiver.external_inputs()[0].clone();
    let mut main = TrackInfo::new("Main");
    let mut detector = TrackInfo::new("Detector");
    detector.kind = TrackKind::Midi;
    detector.mute = true;
    let identity = |format: &str| PluginDeviceInfo {
        format: format.into(),
        uid: "fixture".into(),
        name: "fixture".into(),
        path: fixture.root.clone(),
        state_b64: None,
    };
    detector.plugin_instrument = Some(identity(source_format));
    let effect = EffectId::new();
    main.effects.push(EffectInfo {
        inactive_sidechains: vec![],
        id: effect,
        effect_type: EffectType::Gain,
        bypass: false,
        params: vec![],
        plugin: Some(identity(receiver_format)),
        sidechains: vec![SidechainAssignment {
            input_id: input.id,
            input_name: input.name,
            source: detector.id,
            source_name: detector.name.clone(),
            tap: SourceTap::AfterEffects,
        }],
    });
    let clip = ClipInfo {
        id: ClipId::new(),
        track_id: main.id,
        name: "Pulses".into(),
        position: 0,
        source_offset: 0,
        start_marker: None,
        duration: 2048,
        source: None,
        file_path: None,
        loop_enabled: false,
        loop_start: 0,
        loop_end: 0,
        gain_db: Default::default(),
        fades: Default::default(),
        playback_direction: Default::default(),
        transient_markers: Default::default(),
        warp_markers: Default::default(),
        transpose: Default::default(),
        original_bpm: None,
        warped: false,
        warped_to_bpm: None,
    };
    let mut samples = vec![0.0; 2048];
    for frame in [0, 1000, 1099, 2047] {
        samples[frame] = 0.1;
    }
    let audio = Arc::new(DecodedAudio {
        channels: vec![samples.clone(), samples],
        sample_rate: 48000,
    });
    let notes = NoteClipInfo {
        id: ClipId::new(),
        track_id: detector.id,
        name: "Earlier detector context".into(),
        position_beats: 0.0,
        duration_beats: 1.0,
        notes: vec![MidiNote {
            pitch: 60,
            velocity: 100,
            start_beat: 997.0 / 24000.0,
            duration_beats: 10.0 / 24000.0,
        }],
        start_marker_beats: None,
        loop_enabled: false,
        loop_start_beats: 0.0,
        loop_end_beats: 0.0,
        groove_grid: Default::default(),
    };
    let detector_id = detector.id;
    let req = BounceRequest {
        tracks: vec![main.clone(), detector],
        master: None,
        buses: vec![],
        audio_clips: vec![clip.clone()],
        note_clips: vec![notes],
        clip_audio: [(clip.id, audio)].into_iter().collect(),
        sampler_audio: HashMap::new(),
        drum_pad_audio: HashMap::new(),
        mode: BounceMode::Track(main.id),
        range_samples: range,
        bpm: 120.0,
        sample_rate: 48000,
        swing: Default::default(),
    };
    let mut plugins = OfflinePlugins::default();
    plugins
        .effects
        .insert(effect, Box::new(PluginEffectWrapper::new(receiver)));
    plugins
        .instruments
        .insert(detector_id, Box::new(PluginInstrumentWrapper::new(source)));
    let (plugins, output) = std::thread::spawn(move || {
        let output = render_offline_with_plugins(&req, &mut plugins, |_| {}).unwrap();
        (plugins, output)
    })
    .join()
    .unwrap();
    assert!(plugins.effects.contains_key(&effect));
    assert!(plugins.instruments.contains_key(&detector_id));
    assert!(output.warnings.is_empty());
    assert_eq!(output.audio.num_frames() as u64, range.1 - range.0);
    output.audio.channels[0].clone()
}

#[test]
fn delayed_loaded_formats_trim_only_compensation_and_retain_exact_crop_and_detector_context() {
    let fixture = support::Fixture::new();
    for source in ["clap", "vst3"] {
        for receiver in ["clap", "vst3"] {
            let full = render(&fixture, source, receiver, (0, 2048), 521);
            for (frame, &actual) in full.iter().enumerate() {
                let expected = (if [0, 1000, 1099, 2047].contains(&frame) {
                    0.1
                } else {
                    0.0
                }) + if (997..1007).contains(&frame) {
                    1.5
                } else {
                    0.0
                };
                assert!(
                    (actual - expected * std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-6,
                    "{source}->{receiver} frame{frame}: {actual}"
                );
            }
            let partial = render(&fixture, source, receiver, (1000, 1100), 521);
            assert_eq!(
                partial,
                full[1000..1100],
                "{source}->{receiver} zero-origin crop"
            );
            let wrong = render(&fixture, source, receiver, (1000, 1100), 522);
            assert_ne!(
                wrong, partial,
                "wrong declared delay must fail the sample oracle"
            );
        }
    }
}
