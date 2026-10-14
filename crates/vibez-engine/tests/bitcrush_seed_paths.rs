//! Production engine and Bounce construction must retain persisted random identity and origin.

use std::{collections::HashMap, sync::Arc};
use vibez_core::{
    audio_buffer::DecodedAudio,
    effect::{EffectInfo, EffectType},
    id::{ClipId, EffectId},
    midi::TrackKind,
    perform::SwingAmount,
    routing::{NodeStage, RoutingChannel, RoutingEffect, RoutingNode},
    track::{ClipInfo, TrackInfo},
};
use vibez_engine::{
    commands::EngineCommand,
    engine::{AudioEngine, AudioProcessBlock},
    mixer::{EngineClip, EngineTrack},
    render::{render_offline_with_plugins, BounceMode, BounceRequest, OfflinePlugins},
    routing::PreparedRouting,
};

const FRAMES: usize = 553;
const PARAMS: [f32; 6] = [4.0, 1.0, 1.0, 0.0, 0.0, 1.0];

fn fixture(effect_id: EffectId) -> BounceRequest {
    let mut track = TrackInfo::new("Seeded Bitcrush");
    track.kind = TrackKind::Audio;
    track.gain = 1.0;
    track.pan = 0.5;
    track.effects.push(EffectInfo {
        id: effect_id,
        effect_type: EffectType::Bitcrush,
        bypass: false,
        params: PARAMS.to_vec(),
        plugin: None,
        sidechains: vec![],
        inactive_sidechains: vec![],
    });
    let track: TrackInfo = serde_json::from_str(&serde_json::to_string(&track).unwrap()).unwrap();
    let clip = ClipInfo {
        id: ClipId::new(),
        track_id: track.id,
        name: "Quarter LSB and silence".into(),
        position: 0,
        source_offset: 0,
        start_marker: None,
        duration: FRAMES as u64,
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
    let audio = Arc::new(DecodedAudio {
        channels: vec![vec![0.03125; FRAMES], vec![0.0; FRAMES]],
        sample_rate: 48_000,
    });
    BounceRequest {
        mode: BounceMode::Track(track.id),
        tracks: vec![track],
        master: None,
        buses: vec![],
        clip_audio: HashMap::from([(clip.id, audio)]),
        audio_clips: vec![clip],
        note_clips: vec![],
        sampler_audio: HashMap::new(),
        drum_pad_audio: HashMap::new(),
        range_samples: (0, FRAMES as u64),
        bpm: 120.0,
        sample_rate: 48_000,
        swing: SwingAmount::STRAIGHT,
    }
}

fn realtime(request: &BounceRequest) -> Vec<f32> {
    let info = &request.tracks[0];
    let clip = &request.audio_clips[0];
    let mut track = EngineTrack::new(info.id);
    track.gain = info.gain;
    track.pan = info.pan;
    track.playback_source.clips.push(EngineClip {
        id: clip.id,
        audio: Arc::clone(&request.clip_audio[&clip.id]),
        position: clip.position,
        source_offset: 0,
        start_marker: 0,
        duration: FRAMES as u64,
        loop_enabled: false,
        loop_start: 0,
        loop_end: FRAMES as u64,
        linear_gain: 1.0,
        fades: Default::default(),
        playback_direction: Default::default(),
        warp_markers: Default::default(),
    });
    let effect_id = info.effects[0].id;
    let (mut engine, mut commands, mut events) = AudioEngine::new();
    commands
        .push(EngineCommand::SetSampleRate(request.sample_rate))
        .unwrap();
    commands
        .push(EngineCommand::AddTrack(Box::new(track)))
        .unwrap();
    commands
        .push(EngineCommand::insert_built_in_effect(
            info.id,
            effect_id,
            EffectType::Bitcrush,
            request.sample_rate,
        ))
        .unwrap();
    for (param_index, value) in PARAMS.into_iter().enumerate() {
        commands
            .push(EngineCommand::SetEffectParam {
                track_id: info.id,
                effect_id,
                param_index,
                value,
            })
            .unwrap();
    }
    let channels = [
        RoutingChannel {
            id: info.id,
            is_bus: false,
            sends: vec![],
            effects: vec![RoutingEffect {
                id: effect_id,
                inputs: vec![],
                assignments: vec![],
                inactive_inputs: vec![],
            }],
        },
        RoutingChannel {
            id: vibez_core::id::TrackId::MASTER,
            is_bus: true,
            effects: vec![],
            sends: vec![],
        },
    ];
    let reports = [(
        RoutingNode {
            channel: info.id,
            stage: NodeStage::Effect(effect_id),
        },
        0,
    )];
    commands
        .push(EngineCommand::SetRouting(
            PreparedRouting::prepare_compensated(&channels, 512, &reports, &[], 1).unwrap(),
        ))
        .unwrap();
    commands.push(EngineCommand::Play).unwrap();
    let mut rendered = Vec::new();
    let mut position = 0;
    for frames in [1, 7, 31, 113, 5, 17, 127].into_iter().cycle() {
        let frames = frames.min(FRAMES - position);
        let mut output = vec![0.0; frames * 2];
        let mut capture = vec![0.0; frames * 2];
        engine.process_block(
            AudioProcessBlock::new(&mut output, 2)
                .with_track_output_capture(info.id.raw(), &mut capture),
        );
        rendered.extend(capture);
        while events.pop().is_ok() {}
        position += frames;
        if position == FRAMES {
            break;
        }
    }
    rendered
}

fn interleaved(audio: &DecodedAudio) -> Vec<f32> {
    (0..audio.num_frames())
        .flat_map(|frame| [audio.channels[0][frame], audio.channels[1][frame]])
        .collect()
}

#[test]
fn command_and_bounce_owners_keep_persisted_streams_from_origin_through_a_warmed_crop() {
    // Independent sparse windows distinguish origin0 from the later warmed random phase.
    type StereoLsbWindow = [[i8; 2]; 32];
    let cases: [(&str, StereoLsbWindow, StereoLsbWindow); 2] = [
        (
            "17",
            [
                [1, 0],
                [0, 0],
                [1, 0],
                [-1, 0],
                [0, 0],
                [1, 0],
                [0, 0],
                [0, 0],
                [0, 0],
                [0, 0],
                [0, 0],
                [1, 0],
                [0, 0],
                [0, 1],
                [0, 0],
                [0, 0],
                [-1, 1],
                [0, 0],
                [0, 1],
                [0, 0],
                [1, 0],
                [0, 0],
                [1, 0],
                [0, 0],
                [1, 0],
                [0, 0],
                [0, 1],
                [0, 1],
                [0, 0],
                [1, 1],
                [0, 0],
                [1, 0],
            ],
            [
                [1, 1],
                [1, 0],
                [1, -1],
                [0, 0],
                [0, 0],
                [0, 0],
                [0, 0],
                [0, 0],
                [0, 0],
                [1, -1],
                [0, 0],
                [0, 0],
                [0, 0],
                [0, 0],
                [0, 0],
                [0, -1],
                [1, 0],
                [0, 0],
                [1, 1],
                [0, 0],
                [0, 0],
                [0, 0],
                [0, 0],
                [0, -1],
                [0, -1],
                [1, -1],
                [0, -1],
                [0, 0],
                [0, 1],
                [0, -1],
                [1, 0],
                [0, 0],
            ],
        ),
        (
            "9999",
            [
                [0, 0],
                [0, 0],
                [1, -1],
                [0, -1],
                [1, 0],
                [1, 1],
                [0, 0],
                [0, 1],
                [1, -1],
                [0, 0],
                [0, 0],
                [0, 0],
                [1, -1],
                [0, 0],
                [1, 0],
                [0, 0],
                [0, 0],
                [1, 0],
                [0, 1],
                [1, -1],
                [0, 0],
                [1, 0],
                [0, 0],
                [0, 0],
                [1, 1],
                [0, 0],
                [0, 0],
                [0, 0],
                [0, 0],
                [1, 0],
                [1, 1],
                [1, 0],
            ],
            [
                [0, 1],
                [-1, 0],
                [0, 0],
                [0, -1],
                [0, 0],
                [0, 0],
                [1, 0],
                [0, 0],
                [1, -1],
                [1, 0],
                [0, 0],
                [0, 0],
                [0, -1],
                [1, 0],
                [0, 0],
                [0, 0],
                [1, 0],
                [0, 0],
                [0, -1],
                [-1, 0],
                [0, 1],
                [0, -1],
                [0, 0],
                [0, 0],
                [1, -1],
                [0, 0],
                [0, 0],
                [0, 1],
                [0, -1],
                [0, 0],
                [0, 0],
                [1, 0],
            ],
        ),
    ];
    let mut failures = Vec::new();
    for (stored_id, prefix_codes, crop_codes) in cases {
        let effect_id: EffectId = serde_json::from_str(stored_id).unwrap();
        let mut request = fixture(effect_id);
        let live = realtime(&request);
        let full =
            render_offline_with_plugins(&request, &mut OfflinePlugins::default(), |_| {}).unwrap();
        assert!(full.warnings.is_empty());
        let full = interleaved(&full.audio);
        request.range_samples = (521, FRAMES as u64);
        let crop =
            render_offline_with_plugins(&request, &mut OfflinePlugins::default(), |_| {}).unwrap();
        assert!(crop.warnings.is_empty());
        let crop = interleaved(&crop.audio);
        let expected = |codes: [[i8; 2]; 32]| {
            codes
                .into_iter()
                .flatten()
                .map(|code| f32::from(code) * 0.125 * std::f32::consts::FRAC_1_SQRT_2)
                .collect::<Vec<_>>()
        };
        let prefix = expected(prefix_codes);
        let warmed = expected(crop_codes);
        assert_eq!(live.len(), FRAMES * 2);
        assert_eq!(full.len(), FRAMES * 2);
        assert_eq!(crop.len(), 64);
        for (home, actual, golden) in [
            ("command origin", &live[..64], prefix.as_slice()),
            ("Bounce origin", &full[..64], prefix.as_slice()),
            ("command warmed", &live[1042..], warmed.as_slice()),
            ("Bounce full warmed", &full[1042..], warmed.as_slice()),
            ("Bounce cropped warmed", crop.as_slice(), warmed.as_slice()),
        ] {
            if actual != golden {
                let first = actual
                    .iter()
                    .zip(golden)
                    .position(|(actual, expected)| actual != expected)
                    .unwrap();
                failures.push(format!(
                    "ID{stored_id} {home}: sample{first} actual{} expected{}",
                    actual[first], golden[first]
                ));
            }
        }
        if full != live {
            failures.push(format!(
                "ID{stored_id}: full Bounce differs from real engine origin"
            ));
        }
        if crop != live[1042..] {
            failures.push(format!(
                "ID{stored_id}: cropped Bounce differs from real engine warmed range"
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
