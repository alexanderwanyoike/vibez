//! Verifies native Delay automation and live/Bounce history through the real compensated graph.

use crate::{
    commands::EngineCommand,
    engine::{AudioEngine, AudioProcessBlock},
    render::{render_offline_with_plugins, BounceMode, BounceRequest, OfflinePlugins},
    routing::PreparedRouting,
    test_support::DelayProbe,
};
use std::{collections::HashMap, sync::Arc};
use vibez_core::{
    audio_buffer::DecodedAudio,
    automation::{AutomationLane, AutomationPoint, AutomationTarget},
    effect::{EffectInfo, EffectType, PluginDeviceInfo},
    id::{ClipId, EffectId, TrackId},
    routing::{NodeStage, RoutingChannel, RoutingEffect, RoutingNode},
    track::{ClipInfo, TrackInfo},
};
use vibez_dsp::{delay::param, factory::create_effect_with_params};

const LATENCY: u32 = 37;

fn delay(id: EffectId, sync: bool, mix: f32) -> EffectInfo {
    let effect = vibez_dsp::factory::create_effect(EffectType::Delay, 1000.0);
    let mut params: Vec<_> = effect
        .param_descriptors()
        .iter()
        .map(|descriptor| descriptor.default)
        .collect();
    for (index, value) in [
        (param::SYNC, f32::from(u8::from(sync))),
        (param::LEFT_DIVISION, 3.0),
        (param::LEFT_MODIFIER, 1.0),
        (param::ROUTING, 1.0),
        (param::MIX, mix),
    ] {
        params[index] = value;
    }
    EffectInfo {
        id,
        effect_type: EffectType::Delay,
        bypass: false,
        params,
        plugin: None,
        sidechains: vec![],
        inactive_sidechains: vec![],
    }
}

fn neighbor(id: EffectId) -> EffectInfo {
    EffectInfo {
        id,
        effect_type: EffectType::Gain,
        bypass: false,
        params: vec![],
        plugin: Some(PluginDeviceInfo {
            format: "clap".into(),
            uid: "test.delay-neighbor".into(),
            path: "/test/delay-neighbor.clap".into(),
            name: "Latency neighbor".into(),
            state_b64: None,
        }),
        sidechains: vec![],
        inactive_sidechains: vec![],
    }
}

fn live(
    track: &TrackInfo,
    rate: u32,
    controls: &[(TrackId, AutomationTarget)],
) -> (
    AudioEngine,
    rtrb::Producer<EngineCommand>,
    rtrb::Consumer<crate::events::EngineEvent>,
) {
    let (engine, mut commands, events) = AudioEngine::new();
    commands.push(EngineCommand::SetSampleRate(rate)).unwrap();
    commands.push(EngineCommand::SetBpm(120.0)).unwrap();
    commands.push(EngineCommand::add_track(track.id)).unwrap();
    for info in &track.effects {
        let effect: Box<dyn vibez_dsp::effect::AudioEffect> = if info.plugin.is_some() {
            Box::new(DelayProbe::new(LATENCY))
        } else {
            create_effect_with_params(info.effect_type, rate as f32, &info.params)
        };
        commands
            .push(EngineCommand::InsertEffect {
                track_id: track.id,
                effect_id: info.id,
                effect,
                position: None,
            })
            .unwrap();
    }
    let channels = [
        RoutingChannel {
            id: track.id,
            is_bus: false,
            sends: vec![],
            effects: track
                .effects
                .iter()
                .map(|effect| RoutingEffect {
                    id: effect.id,
                    inputs: vec![],
                    assignments: vec![],
                    inactive_inputs: vec![],
                })
                .collect(),
        },
        RoutingChannel {
            id: TrackId::MASTER,
            is_bus: true,
            sends: vec![],
            effects: vec![],
        },
    ];
    let reports: Vec<_> = track
        .effects
        .iter()
        .map(|effect| {
            (
                RoutingNode {
                    channel: track.id,
                    stage: NodeStage::Effect(effect.id),
                },
                if effect.plugin.is_some() { LATENCY } else { 0 },
            )
        })
        .collect();
    let mut routing =
        PreparedRouting::prepare_compensated(&channels, 512, &reports, &[track.id], 1).unwrap();
    assert_eq!(
        routing.compensation.output_latency,
        if track.effects.iter().any(|effect| effect.plugin.is_some()) {
            LATENCY
        } else {
            0
        }
    );
    routing.configure_automation(controls).unwrap();
    commands.push(EngineCommand::SetRouting(routing)).unwrap();
    commands.push(EngineCommand::Play).unwrap();
    (engine, commands, events)
}

fn pulse(frame: usize, channel: usize) -> f32 {
    match frame {
        0 => [0.25, -0.125][channel],
        7 => [-0.125, 0.0625][channel],
        _ => 0.0,
    }
}

fn request(track: TrackInfo, rate: u32, end: usize) -> BounceRequest {
    let clip_id = ClipId::new();
    let clip = ClipInfo {
        id: clip_id,
        track_id: track.id,
        name: "Stereo pulses".into(),
        position: 0,
        source_offset: 0,
        start_marker: None,
        duration: 8,
        source: None,
        file_path: None,
        loop_enabled: false,
        loop_start: 0,
        loop_end: 8,
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
    BounceRequest {
        tracks: vec![track],
        master: None,
        buses: vec![],
        audio_clips: vec![clip],
        note_clips: vec![],
        clip_audio: HashMap::from([(
            clip_id,
            Arc::new(DecodedAudio {
                channels: (0..2)
                    .map(|channel| (0..8).map(|frame| pulse(frame, channel)).collect())
                    .collect(),
                sample_rate: rate,
            }),
        )]),
        sampler_audio: HashMap::new(),
        drum_pad_audio: HashMap::new(),
        mode: BounceMode::Master,
        range_samples: (0, end as u64),
        bpm: 120.0,
        sample_rate: rate,
        swing: vibez_core::perform::SwingAmount::STRAIGHT,
    }
}

fn bounce(request: &BounceRequest, neighbor: EffectId) -> DecodedAudio {
    let mut plugins = OfflinePlugins::default();
    plugins
        .effects
        .insert(neighbor, Box::new(DelayProbe::new(LATENCY)));
    let result = render_offline_with_plugins(request, &mut plugins, |_| {}).unwrap();
    assert!(result.warnings.is_empty());
    result.audio
}

#[test]
fn live_and_bounce_share_an_explicit_tail_window_and_preserve_history_beside_positive_latency() {
    for rate in [1000, 48000] {
        for neighbor_first in [false, true] {
            let distance = 3 * rate as usize / 8;
            let end = 4 * distance + 40;
            let (delay_id, neighbor_id) = (EffectId::new(), EffectId::new());
            let mut track = TrackInfo::new("Delay continuity");
            track.effects = vec![delay(delay_id, true, 0.4), neighbor(neighbor_id)];
            if neighbor_first {
                track.effects.reverse();
            }
            let ids: Vec<_> = track.effects.iter().map(|effect| effect.id).collect();
            let track_id = track.id;
            let mut request = request(track.clone(), rate, end);
            let full = bounce(&request, neighbor_id);
            assert_eq!(full.num_frames(), end);
            let (mut engine, _commands, mut events) = live(&track, rate, &[]);
            let mut heard = Vec::new();
            let mut position = 0;
            let mut part = 0;
            let mut delay_address = None;
            while position < end + LATENCY as usize {
                let frames = [1, 7, 31, 128, 512][part % 5].min(end + LATENCY as usize - position);
                let mut input = [0.0; 1024];
                let mut output = [0.0; 1024];
                for frame in 0..frames {
                    for channel in 0..2 {
                        input[2 * frame + channel] = pulse(position + frame, channel);
                    }
                }
                engine.process_block(
                    AudioProcessBlock::new(&mut output[..2 * frames], 2)
                        .with_live_input(track_id.raw(), &input[..2 * frames]),
                );
                heard.extend_from_slice(&output[..2 * frames]);
                let slots = &engine.tracks()[0].effects;
                assert_eq!(slots.iter().map(|slot| slot.id).collect::<Vec<_>>(), ids);
                let slot = slots.iter().find(|slot| slot.id == delay_id).unwrap();
                let address = std::ptr::from_ref(slot.effect.as_ref()).cast::<()>();
                assert_eq!(*delay_address.get_or_insert(address), address);
                assert_eq!(slot.effect.latency_samples(), 0);
                assert_eq!(slot.effect.activation_sample_rate(), Some(rate));
                while events.pop().is_ok() {}
                position += frames;
                part += 1;
            }
            for frame in 0..end {
                for channel in 0..2 {
                    let mut expected = 0.6 * pulse(frame, channel);
                    for repeat in 1..=4 {
                        if let Some(source) = frame.checked_sub(repeat * distance) {
                            let source_channel = if repeat % 2 == 0 {
                                1 - channel
                            } else {
                                channel
                            };
                            expected += 0.4
                                * 0.5_f32.powi(repeat as i32 - 1)
                                * pulse(source, source_channel);
                        }
                    }
                    expected *= std::f32::consts::FRAC_1_SQRT_2;
                    let actual = heard[2 * (frame + LATENCY as usize) + channel];
                    assert!((actual - expected).abs() <= 2e-7, "live rate={rate}, neighbor_first={neighbor_first}, frame={frame}, channel={channel}: {actual} != {expected}");
                    assert!((full.channels[channel][frame] - actual).abs() <= 2e-7,
                    "Bounce/live rate={rate}, neighbor_first={neighbor_first}, frame={frame}, channel={channel}");
                }
            }
            assert!(full.channels[0][4 * distance].abs() > 0.0);
            request.range_samples.0 = (distance + 3) as u64;
            let cropped = bounce(&request, neighbor_id);
            for channel in 0..2 {
                assert_eq!(
                    cropped.channels[channel],
                    full.channels[channel][distance + 3..]
                );
            }
            println!("DELAY_LIVE_BOUNCE rate={rate} neighbor_first={neighbor_first} latency={LATENCY} distance={distance} explicit_frames={end} crop_start={}", distance + 3);
        }
    }
}

#[test]
fn engine_automation_is_exact_while_manual_mix_uses_the_ten_ms_trajectory() {
    for rate in [1000, 48000] {
        for automated in [false, true] {
            let mut track = TrackInfo::new("Mix ownership");
            let effect_id = EffectId::new();
            let mut info = delay(effect_id, false, 0.0);
            info.params[param::FEEDBACK] = 0.0;
            track.effects.push(info);
            let target = AutomationTarget::EffectParam {
                effect_id,
                param_index: param::MIX,
            };
            let (mut engine, mut commands, mut events) = live(&track, rate, &[(track.id, target)]);
            let input = [0.25, -0.125];
            let mut first = [0.0; 2];
            engine.process_block(
                AudioProcessBlock::new(&mut first, 2).with_live_input(track.id.raw(), &input),
            );
            let address =
                std::ptr::from_ref(engine.tracks()[0].effects[0].effect.as_ref()).cast::<()>();
            if automated {
                let mut lane = AutomationLane::new(target);
                lane.insert_point(AutomationPoint {
                    beat: 0.0,
                    value: 1.0,
                    curve: 0.0,
                });
                commands
                    .push(EngineCommand::SetAutomationLane {
                        track_id: track.id,
                        lane,
                    })
                    .unwrap();
            } else {
                commands
                    .push(EngineCommand::SetEffectParam {
                        track_id: track.id,
                        effect_id,
                        param_index: param::MIX,
                        value: 1.0,
                    })
                    .unwrap();
            }
            let count = (rate as usize).div_ceil(100);
            let mut heard = Vec::new();
            let mut position = 0;
            let mut part = 0;
            while position < count {
                let frames = [1, 7, 31, 128][part % 4].min(count - position);
                let mut input = [0.0; 256];
                for pair in input[..2 * frames].as_chunks_mut::<2>().0.iter_mut() {
                    pair.copy_from_slice(&[0.25, -0.125]);
                }
                let mut output = [0.0; 256];
                engine.process_block(
                    AudioProcessBlock::new(&mut output[..2 * frames], 2)
                        .with_live_input(track.id.raw(), &input[..2 * frames]),
                );
                heard.extend_from_slice(&output[..2 * frames]);
                while events.pop().is_ok() {}
                position += frames;
                part += 1;
            }
            for (frame, pair) in heard.as_chunks::<2>().0.iter().enumerate() {
                for channel in 0..2 {
                    let mix = if automated {
                        1.0
                    } else {
                        (frame + 1) as f32 / count as f32
                    };
                    let expected =
                        [0.25, -0.125][channel] * (1.0 - mix) * std::f32::consts::FRAC_1_SQRT_2;
                    assert!((pair[channel] - expected).abs() <= 2e-7,
                    "rate={rate}, automated={automated}, frame={frame}, channel={channel}: {} != {expected}", pair[channel]);
                }
            }
            let slots = &engine.tracks()[0].effects;
            assert_eq!(slots.len(), 1);
            assert_eq!(slots[0].id, effect_id);
            assert_eq!(
                std::ptr::from_ref(slots[0].effect.as_ref()).cast::<()>(),
                address
            );
            assert_eq!(slots[0].effect.latency_samples(), 0);
            let state = slots[0].effect.delay_snapshot().unwrap();
            assert_eq!(state.mix, 1.0);
            assert_eq!(state.feedback, 0.0);
            assert_eq!(state.lines[0].target_samples, rate as f64 / 2.0);
            assert!(!state.reference_moving);
            println!(
                "DELAY_ENGINE_OWNERSHIP rate={rate} automated={automated} ramp_frames={count}"
            );
        }
    }
}
