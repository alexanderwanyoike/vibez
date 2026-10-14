//! Neutral native processing preserves source alignment and finite dry headroom through PDC/Bounce.

use super::*;
use vibez_core::routing::{NodeStage, RoutingChannel, RoutingEffect, RoutingNode, SourceTap};

const FRAMES: usize = 2048;
const MASTER_GAIN: f32 = 0.25;
const INIT: [f32; 9] = [16.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 12_000.0];

fn request(sample_rate: u32, bypass: bool) -> BounceRequest {
    let mut track = bare_track("Bitcrush neutral witness");
    track.gain = 1.0;
    track.pan = 0.5;
    let params = if bypass { [2.0, 7.0, 1.0, 12.0, 1.0, 1.0, 0.75, 1.0, 700.0] } else { INIT };
    track.effects.push(EffectInfo { id: EffectId::new(), effect_type: EffectType::Bitcrush,
        bypass, params: params.to_vec(), plugin: None, sidechains: vec![], inactive_sidechains: vec![] });
    let clip = ClipInfo { id: ClipId::new(), track_id: track.id, name: "Neutral range edges".into(),
        position: 0, source_offset: 0, start_marker: None, duration: FRAMES as u64,
        source: None, file_path: None, loop_enabled: false, loop_start: 0, loop_end: 0,
        gain_db: Default::default(), fades: Default::default(), playback_direction: Default::default(),
        transient_markers: Default::default(), warp_markers: Default::default(), transpose: Default::default(),
        original_bpm: None, warped: false, warped_to_bpm: None };
    let mut samples = vec![vec![0.0; FRAMES]; 2];
    for (frame, pair) in [(0, [2.5, -1.75]), (199, [-0.625, 0.375]), (200, [1.75, -2.5]),
        (511, [0.3, -0.19]), (512, [-0.4375, 0.8125]), (1023, [0.17, -0.27]), (2047, [-0.875, 0.65])] {
        samples[0][frame] = pair[0];
        samples[1][frame] = pair[1];
    }
    for frame in 640..672 {
        samples[0][frame] = (frame - 640) as f32 / 32.0 - 0.5;
        samples[1][frame] = 0.25 - samples[0][frame] * 0.5;
    }
    let audio = Arc::new(DecodedAudio { channels: samples, sample_rate });
    let mut master = bare_track("Master witness attenuation");
    master.id = TrackId::MASTER;
    master.gain = MASTER_GAIN;
    master.pan = 0.5;
    BounceRequest { tracks: vec![track], master: Some(master), buses: vec![],
        audio_clips: vec![clip.clone()], note_clips: vec![], clip_audio: HashMap::from([(clip.id, audio)]),
        sampler_audio: HashMap::new(), drum_pad_audio: HashMap::new(), mode: BounceMode::Master,
        range_samples: (0, FRAMES as u64), bpm: 120.0, sample_rate,
        swing: vibez_core::perform::SwingAmount::STRAIGHT }
}

struct Realtime {
    master: Vec<f32>,
    positive: Vec<f32>,
}

fn realtime(request: &BounceRequest, positive: TrackId) -> Realtime {
    let (mut engine, mut commands, mut events) = AudioEngine::new();
    commands.push(Command::SetSampleRate(request.sample_rate)).unwrap();
    let master = request.master.as_ref().unwrap();
    commands.push(Command::SetTrackGain(TrackId::MASTER, master.gain)).unwrap();
    commands.push(Command::SetTrackPan(TrackId::MASTER, master.pan)).unwrap();
    let mut channels = Vec::new();
    let mut reports = Vec::new();
    let mut warnings = Vec::new();
    for info in &request.tracks {
        let channel = super::super::prepare::channel(request, info, SourceTap::AfterFader, None, &mut warnings);
        for slot in &channel.effects {
            assert_eq!(slot.effect.latency_samples(), 0);
            assert_eq!(slot.effect.activation_sample_rate(), Some(request.sample_rate));
            let saved = info.effects.iter().find(|effect| effect.id == slot.id).unwrap();
            for (index, &value) in saved.params.iter().enumerate() {
                assert_eq!(slot.effect.get_param(index), value);
            }
            reports.push((RoutingNode { channel: info.id, stage: NodeStage::Effect(slot.id) }, 0));
        }
        channels.push(RoutingChannel { id: info.id, is_bus: false, sends: vec![],
            effects: info.effects.iter().map(|effect| RoutingEffect { id: effect.id,
                inputs: vec![], assignments: vec![], inactive_inputs: vec![] }).collect() });
        commands.push(Command::AddTrack(Box::new(channel))).unwrap();
    }
    assert!(warnings.is_empty());
    channels.push(RoutingChannel { id: TrackId::MASTER, is_bus: true, sends: vec![], effects: vec![] });
    let plan = crate::routing::PreparedRouting::prepare_compensated(&channels, 128, &reports, &[], 1).unwrap();
    assert_eq!(plan.compensation.output_latency, 0);
    commands.push(Command::SetRouting(plan)).unwrap();
    commands.push(Command::Play).unwrap();
    let mut rendered = Realtime { master: Vec::with_capacity((FRAMES + 128) * 2),
        positive: Vec::with_capacity((FRAMES + 128) * 2) };
    let mut position = 0;
    for frames in [1, 31, 127, 64, 3].into_iter().cycle() {
        let frames = frames.min(FRAMES + 128 - position);
        let mut output = [0.0; 256];
        let mut capture = [0.0; 256];
        engine.process_block(AudioProcessBlock::new(&mut output[..frames * 2], 2)
            .with_track_output_capture(positive.raw(), &mut capture[..frames * 2]));
        rendered.master.extend_from_slice(&output[..frames * 2]);
        rendered.positive.extend_from_slice(&capture[..frames * 2]);
        assert!(engine.offline_compensation_failure().is_none());
        while let Ok(event) = events.pop() {
            assert!(!matches!(event, crate::events::EngineEvent::CompensationInvalid { .. }
                | crate::events::EngineEvent::CompensationFailed { .. }
                | crate::events::EngineEvent::DeviceProcessingFailed { .. }), "{event:?}");
        }
        position += frames;
        if position == FRAMES + 128 { break; }
    }
    rendered
}

fn assert_audio(audio: &[Vec<f32>], source: &DecodedAudio, start: usize, gain: f32) {
    assert_eq!(audio.len(), 2);
    for (channel, actual) in audio.iter().enumerate() {
        assert_eq!(actual.len(), FRAMES - start);
        for (point, &value) in actual.iter().enumerate() {
            let expected = source.channels[channel][start + point] * std::f32::consts::FRAC_1_SQRT_2 * gain;
            assert!(value.is_finite() && (value - expected).abs() < 1e-6,
                "source frame={}, channel={channel}, actual={value}, expected={expected}", start + point);
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
                        } else { 0.0 };
                        let captured = witness.positive[frame * 2 + channel];
                        let master = witness.master[frame * 2 + channel];
                        assert!(captured.is_finite() && (captured - expected).abs() < 1e-6,
                            "Fs={sample_rate}, bypass={bypass}, frame={frame}, channel={channel}, captured={captured}, expected={expected}");
                        assert!(master.is_finite() && (master - expected * MASTER_GAIN).abs() < 1e-6);
                    }
                }
                assert!(witness.positive.iter().any(|sample| sample.abs() > 1.0), "finite dry headroom must reach the real track capture");
                assert!(witness.master.iter().any(|sample| sample.abs() > 0.1));
                for start in [0, 200] {
                    request.range_samples.0 = start;
                    let bounced = render_offline_with_plugins(&request, &mut OfflinePlugins::default(), |_| {}).unwrap();
                    assert!(bounced.warnings.is_empty());
                    assert_eq!(bounced.audio.sample_rate, sample_rate);
                    assert_audio(&bounced.audio.channels, &source, start as usize, MASTER_GAIN);
                }
                request.mode = BounceMode::Track(positive);
                request.range_samples.0 = 0;
                let stem = render_offline_with_plugins(&request, &mut OfflinePlugins::default(), |_| {}).unwrap();
                assert!(stem.warnings.is_empty());
                assert_audio(&stem.audio.channels, &source, 0, 1.0);
                assert!(stem.audio.channels.iter().flatten().any(|sample| sample.abs() > 1.0));
                let mut dry = bare_track("Inverse dry reference");
                dry.gain = 1.0;
                dry.pan = 0.5;
                let mut clip = request.audio_clips[0].clone();
                clip.id = ClipId::new();
                clip.track_id = dry.id;
                let inverse = Arc::new(DecodedAudio { channels: source.channels.iter()
                    .map(|channel| channel.iter().map(|sample| -*sample).collect()).collect(), sample_rate });
                request.clip_audio.insert(clip.id, inverse);
                request.audio_clips.push(clip);
                request.tracks.push(dry);
                if reversed { request.tracks.reverse(); }
                request.mode = BounceMode::Master;
                request.range_samples.0 = 0;
                let canceled = realtime(&request, positive);
                assert!(canceled.positive.iter().any(|sample| sample.abs() > 1.0));
                assert!(canceled.master.iter().all(|sample| sample.is_finite() && sample.abs() < 1e-6),
                    "real PDC0 cancellation: Fs={sample_rate}, bypass={bypass}, reversed={reversed}");
                for start in [0, 200] {
                    request.range_samples.0 = start;
                    let canceled = render_offline_with_plugins(&request, &mut OfflinePlugins::default(), |_| {}).unwrap();
                    assert!(canceled.warnings.is_empty());
                    assert_eq!(canceled.audio.num_frames(), FRAMES - start as usize);
                    assert!(canceled.audio.channels.iter().flatten().all(|sample| sample.is_finite() && sample.abs() < 1e-6),
                        "production Bounce cancellation: Fs={sample_rate}, bypass={bypass}, reversed={reversed}, start={start}");
                }
            }
        }
    }
}
