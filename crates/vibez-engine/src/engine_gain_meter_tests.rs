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
    commands
        .push(EngineCommand::AddTrack(track_id, "Metered".into()))
        .unwrap();
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
    commands
        .push(EngineCommand::SetRouting(routing))
        .unwrap();
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
    for use_graph in [false, true] {
        for monitor_master in [false, true] {
            let (mut engine, mut commands, mut events) = AudioEngine::new();
            let source_id = TrackId::new();
            let return_id = TrackId::new();
            let return_effect = EffectId::new();
            let master_effect = EffectId::new();
            commands.push(EngineCommand::SetSampleRate(48_000)).unwrap();
            commands
                .push(EngineCommand::AddTrack(source_id, "Source".into()))
                .unwrap();
            commands
                .push(EngineCommand::AddBus(return_id, "Return".into()))
                .unwrap();
            commands
                .push(EngineCommand::SetSend {
                    track_id: source_id,
                    bus_id: return_id,
                    amount: 0.25,
                })
                .unwrap();
            for (track_id, effect_id) in [
                (return_id, return_effect),
                (TrackId::MASTER, master_effect),
            ] {
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
            if use_graph {
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
                commands
                    .push(EngineCommand::SetRouting(routing))
                    .unwrap();
            }
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
                    AudioProcessBlock::new(&mut output, 2)
                        .with_live_input(source_id.raw(), &input),
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
                    "graph={use_graph}, master={monitor_master}: expected {expected}, got {actual}"
                );
            }
        }
    }
}

#[test]
fn gain_meter_backpressure_drops_old_windows_without_callback_allocation() {
    for use_graph in [false, true] {
        let (mut engine, _commands, mut events, track_id, effect_id) = gain_meter_fixture(use_graph);
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
                    AudioProcessBlock::new(&mut output, 2)
                        .with_live_input(track_id.raw(), &loud_input),
                );
            }),
            (0, 0),
            "active meters must not allocate or deallocate, graph={use_graph}"
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
                    AudioProcessBlock::new(&mut output, 2)
                        .with_live_input(track_id.raw(), &loud_input),
                );
            }),
            (0, 0),
            "meter backpressure must not allocate or deallocate, graph={use_graph}"
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
                        "a dropped loud window must not leak into new readings: expected {expected}, got {actual}, graph={use_graph}"
                    );
                }
            }
        }
        assert_eq!(readings, 1);
    }
}

fn gain_meter_fixture(
    use_graph: bool,
) -> (
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
    commands
        .push(EngineCommand::AddTrack(track_id, "Metered".into()))
        .unwrap();
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
    if use_graph {
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
    }
    commands
        .push(EngineCommand::SetEffectMeterChannel {
            track_id: Some(track_id),
            generation: 1,
        })
        .unwrap();
    (engine, commands, events, track_id, effect_id)
}
