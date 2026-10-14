//! Neutral native processing preserves source alignment and finite dry headroom through PDC/Bounce.

use super::*;
use vibez_core::effect::SampleHoldMeter;
use vibez_core::routing::{NodeStage, RoutingChannel, RoutingEffect, RoutingNode, SourceTap};

const FRAMES: usize = 2048;
const MASTER_GAIN: f32 = 0.25;
const INIT: [f32; 9] = [16.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 12_000.0];

fn request(sample_rate: u32, bypass: bool) -> BounceRequest {
    let mut track = bare_track("Bitcrush neutral witness");
    track.gain = 1.0;
    track.pan = 0.5;
    let params = if bypass {
        [2.0, 7.0, 1.0, 12.0, 1.0, 1.0, 0.75, 1.0, 700.0]
    } else {
        INIT
    };
    track.effects.push(EffectInfo {
        id: EffectId::new(),
        effect_type: EffectType::Bitcrush,
        bypass,
        params: params.to_vec(),
        plugin: None,
        sidechains: vec![],
        inactive_sidechains: vec![],
    });
    let clip = ClipInfo {
        id: ClipId::new(),
        track_id: track.id,
        name: "Neutral range edges".into(),
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
    let mut samples = vec![vec![0.0; FRAMES]; 2];
    for (frame, pair) in [
        (0, [2.5, -1.75]),
        (199, [-0.625, 0.375]),
        (200, [1.75, -2.5]),
        (511, [0.3, -0.19]),
        (512, [-0.4375, 0.8125]),
        (1023, [0.17, -0.27]),
        (2047, [-0.875, 0.65]),
    ] {
        samples[0][frame] = pair[0];
        samples[1][frame] = pair[1];
    }
    let (left, right) = samples.split_at_mut(1);
    for (frame, (left, right)) in left[0].iter_mut().zip(&mut right[0]).enumerate().take(672).skip(640) {
        *left = (frame - 640) as f32 / 32.0 - 0.5;
        *right = 0.25 - *left * 0.5;
    }
    let audio = Arc::new(DecodedAudio {
        channels: samples,
        sample_rate,
    });
    let mut master = bare_track("Master witness attenuation");
    master.id = TrackId::MASTER;
    master.gain = MASTER_GAIN;
    master.pan = 0.5;
    BounceRequest {
        tracks: vec![track],
        master: Some(master),
        buses: vec![],
        audio_clips: vec![clip.clone()],
        note_clips: vec![],
        clip_audio: HashMap::from([(clip.id, audio)]),
        sampler_audio: HashMap::new(),
        drum_pad_audio: HashMap::new(),
        mode: BounceMode::Master,
        range_samples: (0, FRAMES as u64),
        bpm: 120.0,
        sample_rate,
        swing: vibez_core::perform::SwingAmount::STRAIGHT,
    }
}

struct Realtime {
    master: Vec<f32>,
    positive: Vec<f32>,
    scopes: Vec<SampleHoldMeter>,
}

fn realtime(request: &BounceRequest, positive: TrackId) -> Realtime {
    realtime_with_scope(request, positive, false)
}

fn realtime_with_scope(request: &BounceRequest, positive: TrackId, observe: bool) -> Realtime {
    let (mut engine, mut commands, mut events) = AudioEngine::new();
    commands
        .push(Command::SetSampleRate(request.sample_rate))
        .unwrap();
    let master = request.master.as_ref().unwrap();
    commands
        .push(Command::SetTrackGain(TrackId::MASTER, master.gain))
        .unwrap();
    commands
        .push(Command::SetTrackPan(TrackId::MASTER, master.pan))
        .unwrap();
    let mut channels = Vec::new();
    let mut reports = Vec::new();
    let mut warnings = Vec::new();
    for info in &request.tracks {
        let channel = super::super::prepare::channel(
            request,
            info,
            SourceTap::AfterFader,
            None,
            &mut warnings,
        );
        for slot in &channel.effects {
            assert_eq!(slot.effect.latency_samples(), 0);
            assert_eq!(
                slot.effect.activation_sample_rate(),
                Some(request.sample_rate)
            );
            let saved = info
                .effects
                .iter()
                .find(|effect| effect.id == slot.id)
                .unwrap();
            for (index, &value) in saved.params.iter().enumerate() {
                assert_eq!(slot.effect.get_param(index), value);
            }
            reports.push((
                RoutingNode {
                    channel: info.id,
                    stage: NodeStage::Effect(slot.id),
                },
                0,
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
    let plan =
        crate::routing::PreparedRouting::prepare_compensated(&channels, 128, &reports, &[], 1)
            .unwrap();
    assert_eq!(plan.compensation.output_latency, 0);
    commands.push(Command::SetRouting(plan)).unwrap();
    let effect = request
        .tracks
        .iter()
        .find(|track| track.id == positive)
        .unwrap()
        .effects[0]
        .id;
    if observe {
        commands
            .push(Command::SetEffectMeterChannel {
                track_id: Some(positive),
                generation: 61,
            })
            .unwrap();
    }
    commands.push(Command::Play).unwrap();
    let mut rendered = Realtime {
        master: Vec::with_capacity((FRAMES + 128) * 2),
        positive: Vec::with_capacity((FRAMES + 128) * 2),
        scopes: Vec::new(),
    };
    let mut position = 0;
    for frames in [1, 31, 127, 64, 3].into_iter().cycle() {
        let frames = frames.min(FRAMES + 128 - position);
        let mut output = [0.0; 256];
        let mut capture = [0.0; 256];
        engine.process_block(
            AudioProcessBlock::new(&mut output[..frames * 2], 2)
                .with_track_output_capture(positive.raw(), &mut capture[..frames * 2]),
        );
        rendered.master.extend_from_slice(&output[..frames * 2]);
        rendered.positive.extend_from_slice(&capture[..frames * 2]);
        assert!(engine.offline_compensation_failure().is_none());
        while let Ok(event) = events.pop() {
            match event {
                crate::events::EngineEvent::EffectSampleHold {
                    track_id,
                    effect_id,
                    generation,
                    meter,
                } => {
                    assert!(observe);
                    assert_eq!((track_id, effect_id, generation), (positive, effect, 61));
                    rendered.scopes.push(meter);
                }
                crate::events::EngineEvent::CompensationInvalid { .. }
                | crate::events::EngineEvent::CompensationFailed { .. }
                | crate::events::EngineEvent::DeviceProcessingFailed { .. } => panic!("{event:?}"),
                _ => {}
            }
        }
        position += frames;
        if position == FRAMES + 128 {
            break;
        }
    }
    rendered
}

fn assert_audio(audio: &[Vec<f32>], source: &DecodedAudio, start: usize, gain: f32) {
    assert_eq!(audio.len(), 2);
    for (channel, actual) in audio.iter().enumerate() {
        assert_eq!(actual.len(), FRAMES - start);
        for (point, &value) in actual.iter().enumerate() {
            let expected =
                source.channels[channel][start + point] * std::f32::consts::FRAC_1_SQRT_2 * gain;
            assert!(
                value.is_finite() && (value - expected).abs() < 1e-6,
                "source frame={}, channel={channel}, actual={value}, expected={expected}",
                start + point
            );
        }
    }
}

#[test]
fn init_and_host_bypass_keep_zero_delay_headroom_range_edges_and_parallel_cancellation() {
    for sample_rate in [44_100, 48_000, 96_000] {
        for bypass in [false, true] {
            for reversed in [false, true] {
                let mut request = request(sample_rate, bypass);
                let positive = request.tracks[0].id;
                let source = Arc::clone(&request.clip_audio[&request.audio_clips[0].id]);
                let witness = realtime(&request, positive);
                assert_eq!(witness.master.len(), (FRAMES + 128) * 2);
                for frame in 0..FRAMES + 128 {
                    for channel in 0..2 {
                        let expected = if frame < FRAMES {
                            source.channels[channel][frame] * std::f32::consts::FRAC_1_SQRT_2
                        } else {
                            0.0
                        };
                        let captured = witness.positive[frame * 2 + channel];
                        let master = witness.master[frame * 2 + channel];
                        assert!(captured.is_finite() && (captured - expected).abs() < 1e-6,
                            "Fs={sample_rate}, bypass={bypass}, frame={frame}, channel={channel}, captured={captured}, expected={expected}");
                        assert!(
                            master.is_finite() && (master - expected * MASTER_GAIN).abs() < 1e-6
                        );
                    }
                }
                assert!(
                    witness.positive.iter().any(|sample| sample.abs() > 1.0),
                    "finite dry headroom must reach the real track capture"
                );
                assert!(witness.master.iter().any(|sample| sample.abs() > 0.1));
                for start in [0, 200] {
                    request.range_samples.0 = start;
                    let bounced = render_offline_with_plugins(
                        &request,
                        &mut OfflinePlugins::default(),
                        |_| {},
                    )
                    .unwrap();
                    assert!(bounced.warnings.is_empty());
                    assert_eq!(bounced.audio.sample_rate, sample_rate);
                    assert_audio(
                        &bounced.audio.channels,
                        &source,
                        start as usize,
                        MASTER_GAIN,
                    );
                }
                request.mode = BounceMode::Track(positive);
                request.range_samples.0 = 0;
                let stem =
                    render_offline_with_plugins(&request, &mut OfflinePlugins::default(), |_| {})
                        .unwrap();
                assert!(stem.warnings.is_empty());
                assert_audio(&stem.audio.channels, &source, 0, 1.0);
                assert!(stem
                    .audio
                    .channels
                    .iter()
                    .flatten()
                    .any(|sample| sample.abs() > 1.0));
                let mut dry = bare_track("Inverse dry reference");
                dry.gain = 1.0;
                dry.pan = 0.5;
                let mut clip = request.audio_clips[0].clone();
                clip.id = ClipId::new();
                clip.track_id = dry.id;
                let inverse = Arc::new(DecodedAudio {
                    channels: source
                        .channels
                        .iter()
                        .map(|channel| channel.iter().map(|sample| -*sample).collect())
                        .collect(),
                    sample_rate,
                });
                request.clip_audio.insert(clip.id, inverse);
                request.audio_clips.push(clip);
                request.tracks.push(dry);
                if reversed {
                    request.tracks.reverse();
                }
                request.mode = BounceMode::Master;
                request.range_samples.0 = 0;
                let canceled = realtime(&request, positive);
                assert!(canceled.positive.iter().any(|sample| sample.abs() > 1.0));
                assert!(canceled.master.iter().all(|sample| sample.is_finite() && sample.abs() < 1e-6),
                    "real PDC0 cancellation: Fs={sample_rate}, bypass={bypass}, reversed={reversed}");
                for start in [0, 200] {
                    request.range_samples.0 = start;
                    let canceled = render_offline_with_plugins(
                        &request,
                        &mut OfflinePlugins::default(),
                        |_| {},
                    )
                    .unwrap();
                    assert!(canceled.warnings.is_empty());
                    assert_eq!(canceled.audio.num_frames(), FRAMES - start as usize);
                    assert!(canceled.audio.channels.iter().flatten().all(|sample| sample.is_finite() && sample.abs() < 1e-6),
                        "production Bounce cancellation: Fs={sample_rate}, bypass={bypass}, reversed={reversed}, start={start}");
                }
            }
        }
    }
}

#[test]
fn jitter_drive_and_common_filter_history_match_full_and_warmed_bounce_at_a_nonacquired_crop() {
    const START: usize = 1025;
    const PARAMS: [f32; 9] = [8.0, 7.0, 0.65, 6.0, 0.0, 1.0, 0.75, 1.0, 700.0];
    let identity: EffectId = serde_json::from_str("17").unwrap();
    for sample_rate in [44_100, 48_000, 96_000] {
        let mut request = request(sample_rate, false);
        let positive = request.tracks[0].id;
        request.tracks[0].effects[0].id = identity;
        request.tracks[0].effects[0].params = PARAMS.to_vec();
        let mut samples = vec![vec![0.0; FRAMES]; 2];
        let (left, right) = samples.split_at_mut(1);
        for (frame, (left, right)) in left[0].iter_mut().zip(&mut right[0]).enumerate() {
            let time = frame as f64;
            *left = (0.22 * (time * 0.037).sin()
                + 0.13 * (time * 0.013).cos()
                + if frame % 113 == 0 { 0.5 } else { 0.0 }) as f32;
            *right = (0.19 * (time * 0.021).cos() - 0.15 * (time * 0.043).sin()) as f32;
        }
        let source = Arc::new(DecodedAudio {
            channels: samples,
            sample_rate,
        });
        request
            .clip_audio
            .insert(request.audio_clips[0].id, Arc::clone(&source));
        let actual = realtime_with_scope(&request, positive, true);
        let full =
            render_offline_with_plugins(&request, &mut OfflinePlugins::default(), |_| {}).unwrap();
        assert!(full.warnings.is_empty());
        assert_eq!(full.audio.num_frames(), FRAMES);
        assert_eq!(full.audio.sample_rate, sample_rate);
        request.range_samples.0 = START as u64;
        let crop =
            render_offline_with_plugins(&request, &mut OfflinePlugins::default(), |_| {}).unwrap();
        assert!(crop.warnings.is_empty());
        assert_eq!(crop.audio.num_frames(), FRAMES - START);
        assert_eq!(crop.audio.sample_rate, sample_rate);
        for channel in 0..2 {
            for frame in 0..FRAMES {
                let realtime = actual.master[frame * 2 + channel];
                let full = full.audio.channels[channel][frame];
                assert!(
                    realtime.is_finite() && full.is_finite() && (realtime - full).abs() < 1e-6,
                    "Fs={sample_rate}, channel={channel}, common-origin frame={frame}"
                );
            }
            for point in 0..FRAMES - START {
                let value = crop.audio.channels[channel][point];
                let warmed = full.audio.channels[channel][START + point];
                assert!(
                    value.is_finite() && (value - warmed).abs() < 1e-6,
                    "Fs={sample_rate}, channel={channel}, warmed crop point={point}"
                );
            }
        }
        assert!(!actual.scopes.is_empty());
        for meter in &actual.scopes {
            assert_eq!(meter.sample_rate, sample_rate as f32);
            assert_eq!(meter.latency_samples, 0);
            assert!(!meter.transition_active);
            assert_eq!(meter.secondary_acquisitions, [0; 2]);
            assert_eq!(meter.blending, [0; 2]);
            for point in 0..usize::from(meter.count) {
                let frame = meter.first_source_frame as usize + point;
                assert_eq!(meter.input[point], source.channels[0][frame]);
                assert!(
                    (meter.output[point] * std::f32::consts::FRAC_1_SQRT_2
                        - actual.positive[frame * 2])
                        .abs()
                        < 1e-6
                );
            }
        }
        // Only the 48 kHz/50 Hz window covers1025; other host rates have legitimate observation gaps.
        if sample_rate == 48_000 {
            let meter = actual
                .scopes
                .iter()
                .find(|meter| {
                    meter.first_source_frame <= START as u64
                        && START as u64 - meter.first_source_frame < u64::from(meter.count)
                })
                .expect("real selected packet covering the nonacquisition crop boundary");
            let point = START - meter.first_source_frame as usize;
            assert_eq!(
                meter.primary_acquisitions[point / 64] & (1_u64 << (point % 64)),
                0
            );
            // Frozen tag0 identity17/N7/Jitter.75 acquires at1022 and1029, not1025.
            for acquisition in [1022, 1029] {
                let point = acquisition - meter.first_source_frame as usize;
                assert!(point < usize::from(meter.count));
                assert_ne!(
                    meter.primary_acquisitions[point / 64] & (1_u64 << (point % 64)),
                    0
                );
            }
        }
        let mut fresh = vibez_dsp::factory::create_effect_for_instance(
            EffectType::Bitcrush,
            sample_rate as f32,
            &PARAMS,
            identity,
        );
        let mut late: Vec<_> = (START..FRAMES)
            .flat_map(|frame| [source.channels[0][frame], source.channels[1][frame]])
            .collect();
        fresh.process(&mut late[..2], 2);
        let first = fresh.take_sample_hold_meter().unwrap();
        assert_eq!(
            (first.count, first.first_source_frame, first.latency_samples),
            (1, 0, 0)
        );
        assert_eq!(first.primary_acquisitions, [1, 0]);
        fresh.process(&mut late[2..], 2);
        let mut largest_difference = 0.0_f32;
        let mut first_window_squared = 0.0_f64;
        for (point, pair) in late.as_chunks::<2>().0.iter().enumerate() {
            for (channel, &sample) in pair.iter().enumerate() {
                let cold = sample * std::f32::consts::FRAC_1_SQRT_2 * MASTER_GAIN;
                assert!(cold.is_finite());
                let difference = cold - crop.audio.channels[channel][point];
                largest_difference = largest_difference.max(difference.abs());
                if point < 64 {
                    first_window_squared += f64::from(difference).powi(2);
                }
            }
        }
        let first_window_rms = (first_window_squared / 128.0).sqrt();
        println!("Fs={sample_rate}, fresh-late max difference={largest_difference}, first64 stereo RMS difference={first_window_rms}");
        assert!(largest_difference > 1e-3 && first_window_rms > 1e-4,
            "a freshly constructed late processor cannot substitute for warmed clock/hold/filter history");
    }
}
