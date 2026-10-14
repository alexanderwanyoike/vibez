//! Drive's declared delay preserves source alignment through production Bounce.

use super::*;
use vibez_dsp::drive::{param, DRIVE_PARAMS};

fn request(sample_rate: u32, bypass: bool, character: f32) -> BounceRequest {
    let mut track = bare_track("Drive impulses");
    let params = if character == 0.0 {
        vec![0.0, 0.5, 1.0, 1.0]
    } else {
        let mut values: Vec<_> = DRIVE_PARAMS
            .iter()
            .map(|descriptor| descriptor.default)
            .collect();
        values[param::AMOUNT] = 0.0;
        values[param::CHARACTER] = character;
        values
    };
    track.effects.push(EffectInfo {
        id: EffectId::new(),
        effect_type: EffectType::Drive,
        bypass,
        params,
        plugin: None,
        sidechains: vec![],
        inactive_sidechains: vec![],
    });
    let clip = ClipInfo {
        id: ClipId::new(),
        track_id: track.id,
        name: "Range-edge impulses".into(),
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
    for frame in [0, 199, 200, 2047] {
        samples[frame] = 0.25;
    }
    let audio = Arc::new(DecodedAudio {
        channels: vec![samples.clone(), samples],
        sample_rate,
    });
    BounceRequest {
        tracks: vec![track],
        master: None,
        buses: vec![],
        audio_clips: vec![clip.clone()],
        note_clips: vec![],
        clip_audio: HashMap::from([(clip.id, audio)]),
        sampler_audio: HashMap::new(),
        drum_pad_audio: HashMap::new(),
        mode: BounceMode::Master,
        range_samples: (0, 2048),
        bpm: 120.0,
        sample_rate,
        swing: vibez_core::perform::SwingAmount::STRAIGHT,
    }
}

#[test]
fn drive_bounce_retains_leading_and_trailing_impulses_at_every_rate_and_bypass_state() {
    for sample_rate in [44_100, 48_000, 96_000] {
        for character in [0.0, 1.0] {
            for bypass in [false, true] {
                for start in [0, 200] {
                    let mut req = request(sample_rate, bypass, character);
                    req.range_samples.0 = start;
                    let result =
                        render_offline_with_plugins(&req, &mut OfflinePlugins::default(), |_| {})
                            .unwrap();
                    assert!(result.warnings.is_empty());
                    assert_eq!(result.audio.sample_rate, sample_rate);
                    assert_eq!(result.audio.channels.len(), 2);
                    for channel in &result.audio.channels {
                        assert_eq!(channel.len(), (2048 - start) as usize);
                        for (index, sample) in channel.iter().enumerate() {
                            let expected = if [0, 199, 200, 2047].contains(&(index as u64 + start))
                            {
                                0.25 * std::f32::consts::FRAC_1_SQRT_2
                            } else {
                                0.0
                            };
                            assert!(
                                (*sample - expected).abs() < 1e-6,
                                "rate={sample_rate}, character={character}, bypass={bypass}, start={start}, frame={index}: {sample} != {expected}"
                            );
                        }
                    }
                }
            }
        }
    }
}

fn realtime(req: &BounceRequest) -> Vec<f32> {
    use vibez_core::routing::{NodeStage, RoutingChannel, RoutingEffect, RoutingNode, SourceTap};

    let (mut engine, mut commands, mut events) = AudioEngine::new();
    commands
        .push(Command::SetSampleRate(req.sample_rate))
        .unwrap();
    let mut channels = Vec::new();
    let mut reports = Vec::new();
    let mut warnings = Vec::new();
    for info in &req.tracks {
        let channel =
            super::super::prepare::channel(req, info, SourceTap::AfterFader, None, &mut warnings);
        for slot in &channel.effects {
            assert_eq!(slot.effect.latency_samples(), 64);
            reports.push((
                RoutingNode {
                    channel: info.id,
                    stage: NodeStage::Effect(slot.id),
                },
                64,
            ));
        }
        channels.push(RoutingChannel {
            id: info.id,
            is_bus: false,
            sends: vec![],
            effects: info
                .effects
                .iter()
                .map(|effect| RoutingEffect {
                    id: effect.id,
                    inputs: vec![],
                    assignments: vec![],
                    inactive_inputs: vec![],
                })
                .collect(),
        });
        commands.push(Command::AddTrack(Box::new(channel))).unwrap();
    }
    assert!(warnings.is_empty());
    channels.push(RoutingChannel {
        id: TrackId::MASTER,
        is_bus: true,
        sends: vec![],
        effects: vec![],
    });
    commands
        .push(Command::SetRouting(
            crate::routing::PreparedRouting::prepare_compensated(&channels, 128, &reports, &[], 1)
                .unwrap(),
        ))
        .unwrap();
    commands.push(Command::Play).unwrap();
    let frames = req.range_samples.1 as usize + 64;
    let mut output = Vec::with_capacity(frames * 2);
    let mut partition = [1, 31, 127, 64].into_iter().cycle();
    while output.len() < frames * 2 {
        let count = partition.next().unwrap().min(frames - output.len() / 2);
        let mut block = [0.0; 256];
        engine.process_block(AudioProcessBlock::new(&mut block[..count * 2], 2));
        output.extend_from_slice(&block[..count * 2]);
        while let Ok(event) = events.pop() {
            assert!(
                !matches!(
                    event,
                    crate::events::EngineEvent::CompensationInvalid { .. }
                        | crate::events::EngineEvent::CompensationFailed { .. }
                        | crate::events::EngineEvent::DeviceProcessingFailed { .. }
                ),
                "{event:?}"
            );
        }
    }
    output
}

#[test]
fn drive_realtime_parallel_paths_cancel_at_declared_latency_including_bypass() {
    for sample_rate in [44_100, 48_000, 96_000] {
        for character in [0.0, 1.0] {
            for bypass in [false, true] {
                let mut req = request(sample_rate, bypass, character);
                let audible = realtime(&req);
                for (frame, pair) in audible.as_chunks::<2>().0.iter().enumerate() {
                    let expected = if [64, 263, 264, 2111].contains(&frame) {
                        0.25 * std::f32::consts::FRAC_1_SQRT_2
                    } else {
                        0.0
                    };
                    assert!(pair.iter().all(|sample| (*sample - expected).abs() < 1e-6),
                        "rate={sample_rate}, character={character}, bypass={bypass}, frame={frame}: {pair:?}");
                }
                let dry = bare_track("Inverse dry reference");
                let mut clip = req.audio_clips[0].clone();
                let original = &req.clip_audio[&clip.id];
                let inverse = Arc::new(DecodedAudio {
                    channels: original
                        .channels
                        .iter()
                        .map(|channel| channel.iter().map(|sample| -*sample).collect())
                        .collect(),
                    sample_rate,
                });
                clip.id = ClipId::new();
                clip.track_id = dry.id;
                req.clip_audio.insert(clip.id, inverse);
                req.audio_clips.push(clip);
                req.tracks.push(dry);
                assert!(realtime(&req).iter().all(|sample| sample.abs() < 1e-6));
                let bounced =
                    render_offline_with_plugins(&req, &mut OfflinePlugins::default(), |_| {})
                        .unwrap();
                assert!(bounced.warnings.is_empty());
                assert!(bounced
                    .audio
                    .channels
                    .iter()
                    .flatten()
                    .all(|sample| sample.abs() < 1e-6));
            }
        }
    }
}
