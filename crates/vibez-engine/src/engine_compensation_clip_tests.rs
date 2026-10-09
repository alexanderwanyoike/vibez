use super::*;
use crate::playback_source::{PreparedClipPlayback, PreparedPlaybackSource};
use vibez_core::automation::{AutomationLane, AutomationPoint, AutomationTarget};
use vibez_core::effect::{EffectType, ParamDescriptor};
use vibez_core::id::{ClipId, EffectId};
use vibez_core::perform::MusicalBoundary;
use vibez_core::routing::*;
use vibez_dsp::compensation_delay::CompensationDelay;
use vibez_dsp::effect::AudioEffect;

struct Delay(CompensationDelay, u32);
impl AudioEffect for Delay {
    fn latency_samples(&self) -> u32 {
        self.1
    }
    fn effect_type(&self) -> EffectType {
        EffectType::Gain
    }
    fn param_descriptors(&self) -> &'static [ParamDescriptor] {
        &[]
    }
    fn set_param(&mut self, _: usize, _: f32) -> bool {
        false
    }
    fn get_param(&self, _: usize) -> f32 {
        0.0
    }
    fn process(&mut self, samples: &mut [f32], channels: usize) {
        self.0.process_layout(samples, channels);
    }
    fn reset(&mut self) {
        self.0.clear();
    }
}
struct PositionProbe(u64);
impl AudioEffect for PositionProbe {
    fn set_audio_context(&mut self, context: vibez_core::audio_context::DeviceAudioContext) {
        self.0 = context.musical_sample;
    }
    fn effect_type(&self) -> EffectType {
        EffectType::Gain
    }
    fn param_descriptors(&self) -> &'static [ParamDescriptor] {
        &[]
    }
    fn set_param(&mut self, _: usize, _: f32) -> bool {
        false
    }
    fn get_param(&self, _: usize) -> f32 {
        0.0
    }
    fn process(&mut self, samples: &mut [f32], channels: usize) {
        for (frame, pair) in samples.chunks_exact_mut(channels).enumerate() {
            for sample in pair {
                *sample *= (self.0 + frame as u64 + 1) as f32;
            }
        }
    }
    fn reset(&mut self) {}
}
fn channel(id: TrackId, effects: &[EffectId], bus: bool) -> RoutingChannel {
    RoutingChannel {
        id,
        is_bus: bus,
        sends: vec![],
        effects: effects
            .iter()
            .map(|&id| RoutingEffect {
                inactive_inputs: vec![],
                id,
                inputs: vec![],
                assignments: vec![],
            })
            .collect(),
    }
}
fn lane(target: AutomationTarget, points: &[(u64, f32)], spb: f64) -> AutomationLane {
    let mut lane = AutomationLane::new(target);
    for &(sample, value) in points {
        lane.insert_point(AutomationPoint {
            beat: sample as f64 / spb,
            value,
            curve: 0.0,
        });
    }
    lane
}
fn clip(
    track: TrackId,
    request: u64,
    value: f32,
    lanes: Vec<AutomationLane>,
) -> Box<PreparedClipPlayback> {
    let id = ClipId::new();
    Box::new(PreparedClipPlayback {
        track_id: track,
        clip_id: Some(id),
        request_id: request,
        length_samples: 16,
        looping: true,
        source: Box::new(PreparedPlaybackSource::new(
            vec![EngineClip {
                id,
                audio: Arc::new(DecodedAudio {
                    channels: vec![vec![value; 16]; 2],
                    sample_rate: 128,
                }),
                position: 0,
                source_offset: 0,
                start_marker: 0,
                duration: 16,
                loop_enabled: false,
                loop_start: 0,
                loop_end: 0,
                linear_gain: 1.0,
                fades: Default::default(),
                playback_direction: Default::default(),
                warp_markers: Default::default(),
            }],
            vec![],
            lanes,
        )),
    })
}

#[test]
fn wider_hardware_keeps_compensated_live_and_capture_frames_on_the_first_pair() {
    for channels in [2, 6] {
        let (mut engine, mut commands, mut events) = AudioEngine::new();
        let track = TrackId::new();
        let effect = EffectId::new();
        commands
            .push(EngineCommand::AddTrack(track, "Live".into()))
            .unwrap();
        commands
            .push(EngineCommand::AddPluginEffect {
                track_id: track,
                effect_id: effect,
                effect: Box::new(Delay(CompensationDelay::prepare(137, 2, 274).unwrap(), 137)),
                position: None,
            })
            .unwrap();
        let model = [
            channel(track, &[effect], false),
            channel(TrackId::MASTER, &[], true),
        ];
        let reports = [(
            RoutingNode {
                channel: track,
                stage: NodeStage::Effect(effect),
            },
            137,
        )];
        commands
            .push(EngineCommand::SetRouting(
                crate::routing::PreparedRouting::prepare_compensated(&model, 64, &reports, &[], 1)
                    .unwrap(),
            ))
            .unwrap();
        commands.push(EngineCommand::Play).unwrap();
        engine.process(&mut [], channels);
        while events.pop().is_ok() {}
        let mut position = 0;
        for frames in [1, 7, 31, 64].into_iter().cycle().take(12) {
            let mut input = vec![0.0; frames * channels];
            let mut output = vec![0.0; frames * channels];
            let mut capture = vec![0.0; frames * channels];
            if position == 0 {
                input[0] = 0.25;
                input[1] = 0.5;
                input[2..channels].fill(99.0);
            }
            assert_eq!(
                crate::retirement::tests::allocations(|| engine.process_block(
                    AudioProcessBlock::new(&mut output, channels)
                        .with_live_input(track.raw(), &input)
                        .with_track_output_capture(track.raw(), &mut capture)
                )),
                (0, 0)
            );
            for (frame, (output, capture)) in output
                .chunks_exact(channels)
                .zip(capture.chunks_exact(channels))
                .enumerate()
            {
                let (left, right) = equal_power_pan(0.5);
                let expected = if position + frame == 137 {
                    [0.25 * left, 0.5 * right]
                } else {
                    [0.0; 2]
                };
                assert_eq!(
                    &output[..2],
                    &expected,
                    "hardware layout {channels}, frame {}",
                    position + frame
                );
                assert_eq!(&capture[..2], &expected);
                assert!(output[2..]
                    .iter()
                    .chain(&capture[2..])
                    .all(|sample| *sample == 0.0));
            }
            position += frames;
            while events.pop().is_ok() {}
        }
    }
}

#[test]
fn independent_clip_wraps_retain_effect_fader_send_and_device_audio_context() {
    let (mut engine, mut commands, mut events) = AudioEngine::new();
    let track = TrackId::new();
    let slow = TrackId::new();
    let bus = TrackId::new();
    let effects = [EffectId::new(), EffectId::new(), EffectId::new()];
    let slow_fx = EffectId::new();
    commands.push(EngineCommand::SetSampleRate(128)).unwrap();
    commands.push(EngineCommand::SetBpm(60.0)).unwrap();
    for id in [track, slow] {
        commands
            .push(EngineCommand::AddTrack(id, "Clip".into()))
            .unwrap();
    }
    commands
        .push(EngineCommand::AddBus(bus, "Return".into()))
        .unwrap();
    commands
        .push(EngineCommand::SetSend {
            track_id: track,
            bus_id: bus,
            amount: 0.5,
        })
        .unwrap();
    for (id, report) in [(effects[0], 137), (slow_fx, 521)] {
        commands
            .push(EngineCommand::AddPluginEffect {
                track_id: if id == slow_fx { slow } else { track },
                effect_id: id,
                effect: Box::new(Delay(
                    CompensationDelay::prepare(report, 2, 1042).unwrap(),
                    report,
                )),
                position: None,
            })
            .unwrap();
    }
    commands
        .push(EngineCommand::AddEffect {
            track_id: track,
            effect_id: effects[1],
            effect_type: EffectType::Gain,
            position: None,
        })
        .unwrap();
    commands
        .push(EngineCommand::AddPluginEffect {
            track_id: track,
            effect_id: effects[2],
            effect: Box::new(PositionProbe(0)),
            position: None,
        })
        .unwrap();
    let fx_target = AutomationTarget::EffectParam {
        effect_id: effects[1],
        param_index: 0,
    };
    let mut source = channel(track, &effects, false);
    source.sends.push((bus, 1.0));
    let channels = [
        source,
        channel(slow, &[slow_fx], false),
        channel(bus, &[], true),
        channel(TrackId::MASTER, &[], true),
    ];
    let reports = [
        (
            RoutingNode {
                channel: track,
                stage: NodeStage::Effect(effects[0]),
            },
            137,
        ),
        (
            RoutingNode {
                channel: slow,
                stage: NodeStage::Effect(slow_fx),
            },
            521,
        ),
    ];
    let mut routing =
        crate::routing::PreparedRouting::prepare_compensated(&channels, 64, &reports, &[], 1)
            .unwrap();
    routing
        .configure_automation(&[
            (track, fx_target),
            (track, AutomationTarget::TrackGain),
            (track, AutomationTarget::TrackPan),
            (track, AutomationTarget::Send { bus_id: bus }),
        ])
        .unwrap();
    commands.push(EngineCommand::SetRouting(routing)).unwrap();
    let automation = vec![
        lane(fx_target, &[(0, 0.25), (15, 0.75)], 128.0),
        lane(
            AutomationTarget::TrackGain,
            &[(0, 0.5), (7, 0.5), (8, 0.25), (15, 0.25)],
            128.0,
        ),
        lane(AutomationTarget::TrackPan, &[(0, 0.0), (15, 1.0)], 128.0),
        lane(
            AutomationTarget::Send { bus_id: bus },
            &[(0, 0.5), (7, 0.5), (8, 1.0), (15, 1.0)],
            128.0,
        ),
    ];
    commands
        .push(EngineCommand::QueueClips {
            clips: vec![clip(track, 1, 1.0, automation), clip(slow, 2, 0.0, vec![])],
            quantization: MusicalBoundary::Immediate,
        })
        .unwrap();
    let mut rendered = Vec::new();
    for frames in [1, 7, 31, 64].into_iter().cycle().take(40) {
        let mut block = vec![0.0; frames * 2];
        engine.process(&mut block, 2);
        rendered.extend(block);
        while events.pop().is_ok() {}
    }
    let launch = rendered.len() / 2;
    let replacement = vec![
        lane(fx_target, &[(0, 0.5), (15, 0.5)], 128.0),
        lane(AutomationTarget::TrackGain, &[(0, 0.25), (15, 0.25)], 128.0),
        lane(AutomationTarget::TrackPan, &[(0, 1.0), (15, 0.0)], 128.0),
        lane(
            AutomationTarget::Send { bus_id: bus },
            &[(0, 0.25), (15, 0.25)],
            128.0,
        ),
    ];
    commands
        .push(EngineCommand::QueueClips {
            clips: vec![clip(track, 3, 2.0, replacement)],
            quantization: MusicalBoundary::Immediate,
        })
        .unwrap();
    for frames in [1, 7, 31, 64].into_iter().cycle().take(32) {
        let mut block = vec![0.0; frames * 2];
        engine.process(&mut block, 2);
        rendered.extend(block);
        while events.pop().is_ok() {}
    }
    for (physical, values) in rendered.chunks_exact(2).enumerate().skip(521) {
        let changed = physical >= launch + 521;
        let phase = if changed {
            (physical - launch - 521) % 16
        } else {
            (physical - 521) % 16
        };
        let gain = if changed {
            1.0
        } else {
            (0.5 + phase as f32 / 15.0) * if phase < 8 { 1.0 } else { 0.5 }
        };
        let send = if changed {
            0.25
        } else {
            if phase < 8 {
                0.5
            } else {
                1.0
            }
        };
        let expected = (phase + 1) as f32 * gain * (1.0 + send);
        let pan = if changed {
            1.0 - phase as f32 / 15.0
        } else {
            phase as f32 / 15.0
        };
        let (left, right) = equal_power_pan(pan);
        assert!(
            (values[0] - expected * left).abs() < 2e-5,
            "frame {physical}: {values:?}, expected {}",
            expected * left
        );
        assert!(
            (values[1] - expected * right).abs() < 2e-5,
            "frame {physical}: values {values:?}, expected right {}, phase {phase}, changed {changed}", expected * right
        );
    }
}

#[test]
fn late_bar_launch_waits_for_the_next_achievable_boundary_and_reports_it_when_heard() {
    let (mut engine, mut commands, mut events) = AudioEngine::new();
    let track = TrackId::new();
    let slow = TrackId::new();
    let effect = EffectId::new();
    commands.push(EngineCommand::SetSampleRate(128)).unwrap();
    commands.push(EngineCommand::SetBpm(60.0)).unwrap();
    for id in [track, slow] {
        commands
            .push(EngineCommand::AddTrack(id, "Clip".into()))
            .unwrap();
    }
    commands
        .push(EngineCommand::AddPluginEffect {
            track_id: slow,
            effect_id: effect,
            effect: Box::new(Delay(
                CompensationDelay::prepare(521, 2, 1042).unwrap(),
                521,
            )),
            position: None,
        })
        .unwrap();
    let channels = [
        channel(track, &[], false),
        channel(slow, &[effect], false),
        channel(TrackId::MASTER, &[], true),
    ];
    let reports = [(
        RoutingNode {
            channel: slow,
            stage: NodeStage::Effect(effect),
        },
        521,
    )];
    commands
        .push(EngineCommand::SetRouting(
            crate::routing::PreparedRouting::prepare_compensated(&channels, 64, &reports, &[], 1)
                .unwrap(),
        ))
        .unwrap();
    commands
        .push(EngineCommand::QueueClips {
            clips: vec![clip(track, 1, 0.0, vec![])],
            quantization: MusicalBoundary::Immediate,
        })
        .unwrap();
    for _ in 0..15 {
        engine.process(&mut [0.0; 128], 2);
        while events.pop().is_ok() {}
    }
    engine.process(&mut [0.0; 122], 2);
    while events.pop().is_ok() {}
    assert_eq!(engine.performance_position, 1021);
    assert_eq!(
        engine.presentation_context(engine.mix_latency()).perform,
        500
    );
    commands
        .push(EngineCommand::QueueClips {
            clips: vec![clip(track, 2, 1.0, vec![])],
            quantization: MusicalBoundary::OneBar,
        })
        .unwrap();
    let mut rendered = 1021usize;
    let mut onset = None;
    let mut transition = None;
    for frames in [1, 7, 31, 64].into_iter().cycle().take(30) {
        let mut output = vec![0.0; frames * 2];
        engine.process(&mut output, 2);
        if onset.is_none() {
            onset = output
                .chunks_exact(2)
                .position(|frame| frame[0] > 0.1)
                .map(|offset| rendered + offset);
        }
        rendered += frames;
        while let Ok(event) = events.pop() {
            if let EngineEvent::ClipTransitioned {
                request_id: 2,
                effective_at_samples,
                ..
            } = event
            {
                transition = Some((effective_at_samples, rendered));
            }
        }
    }
    assert_eq!(onset, Some(1024 + 521));
    assert_eq!(transition.map(|(effective, _)| effective), Some(1024));
    assert!(transition.unwrap().1 >= 1024 + 521);
}
