//! Gain panel readings identify each insert and measure its actual stereo signal.

use super::*;
use vibez_core::effect::EffectType;
use vibez_core::id::EffectId;
use vibez_core::routing::{RoutingChannel, RoutingEffect};

#[test]
fn monitored_gain_chain_reports_each_inserts_stereo_input_and_output() {
    let (mut engine, mut commands, mut events) = AudioEngine::new();
    let track_id = TrackId::new();
    let effects = [EffectId::new(), EffectId::new()];
    commands.push(EngineCommand::SetSampleRate(48_000)).unwrap();
    commands.push(EngineCommand::add_track(track_id)).unwrap();
    for effect_id in effects {
        commands
            .push(EngineCommand::insert_built_in_effect(
                track_id,
                effect_id,
                EffectType::Gain,
                48_000,
            ))
            .unwrap();
        commands
            .push(EngineCommand::SetEffectParam {
                track_id,
                effect_id,
                param_index: 0,
                value: 0.5,
            })
            .unwrap();
    }
    commands
        .push(EngineCommand::SetTrackGain(track_id, 0.125))
        .unwrap();
    commands
        .push(EngineCommand::SetTrackGain(TrackId::MASTER, 0.125))
        .unwrap();
    let routing = crate::routing::PreparedRouting::prepare(
        &[
            RoutingChannel {
                id: track_id,
                is_bus: false,
                effects: effects
                    .iter()
                    .map(|&id| RoutingEffect {
                        id,
                        inputs: vec![],
                        assignments: vec![],
                        inactive_inputs: vec![],
                    })
                    .collect(),
                sends: vec![],
            },
            RoutingChannel {
                id: TrackId::MASTER,
                is_bus: true,
                effects: vec![],
                sends: vec![],
            },
        ],
        128,
    )
    .unwrap();
    commands.push(EngineCommand::SetRouting(routing)).unwrap();
    commands
        .push(EngineCommand::SetEffectMeterChannel {
            track_id: Some(track_id),
            generation: 7,
        })
        .unwrap();

    let input: Vec<f32> = [0.8, -0.1, 0.0, 0.3].repeat(480);
    let mut output = vec![0.0; input.len()];
    let mut readings = [None; 2];
    for _ in 0..12 {
        engine.process_block(
            AudioProcessBlock::new(&mut output, 2).with_live_input(track_id.raw(), &input),
        );
        while let Ok(event) = events.pop() {
            if let EngineEvent::EffectMeter {
                track_id: observed_track,
                effect_id,
                generation,
                input,
                output,
            } = event
            {
                assert_eq!(observed_track, track_id);
                assert_eq!(generation, 7);
                let index = effects.iter().position(|&id| id == effect_id).unwrap();
                readings[index] = Some((input, output));
            }
        }
    }

    for (index, reading) in readings.into_iter().enumerate() {
        let (input, output) = reading.expect("each visible Gain needs its own real meter reading");
        let incoming_gain = 0.5_f32.powi(index as i32);
        for (actual, expected) in [
            (input.peak_l, 0.8 * incoming_gain),
            (input.peak_r, 0.3 * incoming_gain),
            (input.rms_l, 0.32_f32.sqrt() * incoming_gain),
            (input.rms_r, 0.05_f32.sqrt() * incoming_gain),
            (output.peak_l, 0.4 * incoming_gain),
            (output.peak_r, 0.15 * incoming_gain),
            (output.rms_l, 0.32_f32.sqrt() * incoming_gain * 0.5),
            (output.rms_r, 0.05_f32.sqrt() * incoming_gain * 0.5),
        ] {
            assert!(
                (actual - expected).abs() < 1e-5,
                "insert {index}: expected {expected}, got {actual}"
            );
        }
    }
}

#[test]
fn return_and_master_gain_meters_observe_insert_levels_in_both_render_paths() {
    for monitor_master in [false, true] {
        let (mut engine, mut commands, mut events) = AudioEngine::new();
        let source_id = TrackId::new();
        let return_id = TrackId::new();
        let return_effect = EffectId::new();
        let master_effect = EffectId::new();
        commands.push(EngineCommand::SetSampleRate(48_000)).unwrap();
        commands.push(EngineCommand::add_track(source_id)).unwrap();
        commands.push(EngineCommand::add_bus(return_id)).unwrap();
        commands
            .push(EngineCommand::SetSend {
                track_id: source_id,
                bus_id: return_id,
                amount: 0.25,
            })
            .unwrap();
        for (track_id, effect_id) in [(return_id, return_effect), (TrackId::MASTER, master_effect)]
        {
            commands
                .push(EngineCommand::insert_built_in_effect(
                    track_id,
                    effect_id,
                    EffectType::Gain,
                    48_000,
                ))
                .unwrap();
            commands
                .push(EngineCommand::SetEffectParam {
                    track_id,
                    effect_id,
                    param_index: 0,
                    value: 0.5,
                })
                .unwrap();
        }
        for (track_id, gain) in [
            (source_id, 0.5),
            (return_id, 0.25),
            (TrackId::MASTER, 0.125),
        ] {
            commands
                .push(EngineCommand::SetTrackGain(track_id, gain))
                .unwrap();
        }
        let routing = crate::routing::PreparedRouting::prepare(
            &[
                RoutingChannel {
                    id: source_id,
                    is_bus: false,
                    effects: vec![],
                    sends: vec![return_id],
                },
                RoutingChannel {
                    id: return_id,
                    is_bus: true,
                    effects: vec![RoutingEffect {
                        id: return_effect,
                        inputs: vec![],
                        assignments: vec![],
                        inactive_inputs: vec![],
                    }],
                    sends: vec![],
                },
                RoutingChannel {
                    id: TrackId::MASTER,
                    is_bus: true,
                    effects: vec![RoutingEffect {
                        id: master_effect,
                        inputs: vec![],
                        assignments: vec![],
                        inactive_inputs: vec![],
                    }],
                    sends: vec![],
                },
            ],
            128,
        )
        .unwrap();
        commands.push(EngineCommand::SetRouting(routing)).unwrap();
        let (monitored_channel, monitored_effect) = if monitor_master {
            (TrackId::MASTER, master_effect)
        } else {
            (return_id, return_effect)
        };
        commands
            .push(EngineCommand::SetEffectMeterChannel {
                track_id: Some(monitored_channel),
                generation: 19,
            })
            .unwrap();
        let input: Vec<f32> = [0.8, -0.1, 0.0, 0.3].repeat(480);
        let mut output = vec![0.0; input.len()];
        let mut reading = None;
        for _ in 0..12 {
            engine.process_block(
                AudioProcessBlock::new(&mut output, 2).with_live_input(source_id.raw(), &input),
            );
            while let Ok(event) = events.pop() {
                if let EngineEvent::EffectMeter {
                    track_id,
                    effect_id,
                    generation,
                    input,
                    output,
                } = event
                {
                    assert_eq!(track_id, monitored_channel);
                    assert_eq!(effect_id, monitored_effect);
                    assert_eq!(generation, 19);
                    reading = Some((input, output));
                }
            }
        }
        let (input, output) = reading.expect("selected return and master Gains need readings");
        let source_gain = 0.5 * std::f32::consts::FRAC_1_SQRT_2;
        let incoming_gain = source_gain
            * if monitor_master {
                1.0 + 0.25 * 0.5 * 0.25
            } else {
                0.25
            };
        for (actual, expected) in [
            (input.peak_l, 0.8 * incoming_gain),
            (input.peak_r, 0.3 * incoming_gain),
            (input.rms_l, 0.32_f32.sqrt() * incoming_gain),
            (input.rms_r, 0.05_f32.sqrt() * incoming_gain),
            (output.peak_l, 0.4 * incoming_gain),
            (output.peak_r, 0.15 * incoming_gain),
            (output.rms_l, 0.32_f32.sqrt() * incoming_gain * 0.5),
            (output.rms_r, 0.05_f32.sqrt() * incoming_gain * 0.5),
        ] {
            assert!(
                (actual - expected).abs() < 1e-5,
                "master={monitor_master}: expected {expected}, got {actual}"
            );
        }
    }
}

#[test]
fn gain_meter_backpressure_drops_old_windows_without_callback_allocation() {
    let (mut engine, _commands, mut events, track_id, effect_id) = gain_meter_fixture();
    let loud_input = [0.8, -0.4].repeat(960);
    let quiet_input = [0.2, -0.1].repeat(960);
    let mut output = vec![0.0; loud_input.len()];
    engine.process_block(
        AudioProcessBlock::new(&mut output, 2).with_live_input(track_id.raw(), &loud_input),
    );
    while events.pop().is_ok() {}

    assert_eq!(
        crate::retirement::tests::allocations(|| {
            engine.process_block(
                AudioProcessBlock::new(&mut output, 2).with_live_input(track_id.raw(), &loud_input),
            );
        }),
        (0, 0),
        "active meters must not allocate or deallocate"
    );
    let mut saw_meter = false;
    while let Ok(event) = events.pop() {
        saw_meter |= matches!(event, EngineEvent::EffectMeter { .. });
    }
    assert!(saw_meter);
    let audible_output = output.clone();
    while engine
        .event_tx
        .push(EngineEvent::PlaybackPosition(0))
        .is_ok()
    {}
    assert_eq!(
        crate::retirement::tests::allocations(|| {
            engine.process_block(
                AudioProcessBlock::new(&mut output, 2).with_live_input(track_id.raw(), &loud_input),
            );
        }),
        (0, 0),
        "meter backpressure must not allocate or deallocate"
    );
    assert_eq!(output, audible_output);
    while let Ok(event) = events.pop() {
        assert!(!matches!(event, EngineEvent::EffectMeter { .. }));
    }

    engine.process_block(
        AudioProcessBlock::new(&mut output, 2).with_live_input(track_id.raw(), &quiet_input),
    );
    let mut readings = 0;
    while let Ok(event) = events.pop() {
        if let EngineEvent::EffectMeter {
            track_id: observed_track,
            effect_id: observed_effect,
            input,
            output,
            ..
        } = event
        {
            readings += 1;
            assert_eq!(observed_track, track_id);
            assert_eq!(observed_effect, effect_id);
            for (actual, expected) in [
                (input.peak_l, 0.2),
                (input.peak_r, 0.1),
                (input.rms_l, 0.2),
                (input.rms_r, 0.1),
                (output.peak_l, 0.1),
                (output.peak_r, 0.05),
                (output.rms_l, 0.1),
                (output.rms_r, 0.05),
            ] {
                assert!(
                    (actual - expected).abs() < 1e-5,
                    "a dropped loud window must not leak into new readings: expected {expected}, got {actual}"
                );
            }
        }
    }
    assert_eq!(readings, 1);
}

#[test]
fn gain_meter_reports_bypass_and_audible_reenable_fade() {
    let (mut engine, mut commands, mut events, track_id, effect_id) = gain_meter_fixture();
    let input = [0.8, -0.4].repeat(960);
    let mut output = vec![0.0; input.len()];
    engine.process_block(
        AudioProcessBlock::new(&mut output, 2).with_live_input(track_id.raw(), &input),
    );
    while events.pop().is_ok() {}

    for bypass in [true, false] {
        commands
            .push(EngineCommand::SetEffectBypass {
                track_id,
                effect_id,
                bypass,
            })
            .unwrap();
        engine.process_block(
            AudioProcessBlock::new(&mut output, 2).with_live_input(track_id.raw(), &input),
        );
        let mut reading = None;
        while let Ok(event) = events.pop() {
            if let EngineEvent::EffectMeter { input, output, .. } = event {
                assert!(reading.is_none());
                reading = Some((input, output));
            }
        }
        let (input_levels, output_levels) = reading.expect("bypassed Gains still need readings");
        assert!((input_levels.peak_l - 0.8).abs() < 1e-5);
        assert!((input_levels.peak_r - 0.4).abs() < 1e-5);
        assert!((input_levels.rms_l - 0.8).abs() < 1e-5);
        assert!((input_levels.rms_r - 0.4).abs() < 1e-5);

        // The insert precedes equal-power channel pan. Its meter must
        // include the graph's dry/wet recovery blend heard downstream.
        let mut heard_peak = [0.0_f32; 2];
        let mut heard_squares = [0.0_f64; 2];
        for frame in output.as_chunks::<2>().0 {
            for channel in 0..2 {
                let sample = frame[channel] / std::f32::consts::FRAC_1_SQRT_2;
                heard_peak[channel] = heard_peak[channel].max(sample.abs());
                heard_squares[channel] += f64::from(sample).powi(2);
            }
        }
        for (actual, expected) in [
            (output_levels.peak_l, heard_peak[0]),
            (output_levels.peak_r, heard_peak[1]),
            (
                output_levels.rms_l,
                (heard_squares[0] / 960.0).sqrt() as f32,
            ),
            (
                output_levels.rms_r,
                (heard_squares[1] / 960.0).sqrt() as f32,
            ),
        ] {
            assert!(
                (actual - expected).abs() < 1e-5,
                "bypass={bypass}: meter {actual}, heard {expected}"
            );
        }
        if bypass {
            assert_eq!(input_levels, output_levels);
        } else {
            assert!((output_levels.peak_l - 0.8).abs() < 1e-5);
            assert!(output_levels.rms_l > 0.4 && output_levels.rms_l < 0.8);
        }
    }
}

#[test]
fn legacy_gain_automation_keeps_linear_mapping_with_neutral_new_controls() {
    use vibez_core::automation::{AutomationLane, AutomationPoint, AutomationTarget};

    let (mut engine, mut commands, _events, track_id, effect_id) = gain_meter_fixture();
    commands
        .push(EngineCommand::SetEffectParam {
            track_id,
            effect_id,
            param_index: 0,
            value: 1.5,
        })
        .unwrap();
    let target = AutomationTarget::EffectParam {
        effect_id,
        param_index: 0,
    };
    let mut lane = AutomationLane::new(target);
    lane.insert_point(AutomationPoint {
        beat: 0.0,
        value: 0.25,
        curve: 0.0,
    });
    commands
        .push(EngineCommand::SetAutomationLane {
            track_id,
            insertion: crate::clip_insertion::PreparedAutomationLaneInsertion::new(lane),
        })
        .unwrap();
    engine.process(&mut [], 2);
    engine
        .routing
        .as_mut()
        .configure_automation(&[(track_id, target)])
        .unwrap();

    let input = [0.8, -0.4].repeat(960);
    let mut output = vec![0.0; input.len()];
    engine.process_block(
        AudioProcessBlock::new(&mut output, 2).with_live_input(track_id.raw(), &input),
    );
    for frame in output.as_chunks::<2>().0 {
        for (actual, expected) in frame
            .iter()
            .copied()
            .zip([0.4, -0.2].map(|sample| sample * std::f32::consts::FRAC_1_SQRT_2))
        {
            assert!(
                (actual - expected).abs() < 1e-5,
                "legacy normalized 0.25 must render half gain: expected {expected}, got {actual}"
            );
        }
    }
    let effect = &engine.tracks[0].effects[0].effect;
    assert_eq!(effect.get_param(0), 0.5);
    for index in 1..5 {
        assert_eq!(effect.get_param(index), 0.0);
    }
}

fn gain_meter_fixture() -> (
    AudioEngine,
    Producer<EngineCommand>,
    Consumer<EngineEvent>,
    TrackId,
    EffectId,
) {
    let (engine, mut commands, events) = AudioEngine::new();
    let track_id = TrackId::new();
    let effect_id = EffectId::new();
    commands.push(EngineCommand::SetSampleRate(48_000)).unwrap();
    commands.push(EngineCommand::add_track(track_id)).unwrap();
    commands
        .push(EngineCommand::insert_built_in_effect(
            track_id,
            effect_id,
            EffectType::Gain,
            48_000,
        ))
        .unwrap();
    commands
        .push(EngineCommand::SetEffectParam {
            track_id,
            effect_id,
            param_index: 0,
            value: 0.5,
        })
        .unwrap();
    commands
        .push(EngineCommand::SetRouting(
            crate::routing::PreparedRouting::prepare(
                &[
                    RoutingChannel {
                        id: track_id,
                        is_bus: false,
                        effects: vec![RoutingEffect {
                            id: effect_id,
                            inputs: vec![],
                            assignments: vec![],
                            inactive_inputs: vec![],
                        }],
                        sends: vec![],
                    },
                    RoutingChannel {
                        id: TrackId::MASTER,
                        is_bus: true,
                        effects: vec![],
                        sends: vec![],
                    },
                ],
                128,
            )
            .unwrap(),
        ))
        .unwrap();
    commands
        .push(EngineCommand::SetEffectMeterChannel {
            track_id: Some(track_id),
            generation: 1,
        })
        .unwrap();
    (engine, commands, events, track_id, effect_id)
}
