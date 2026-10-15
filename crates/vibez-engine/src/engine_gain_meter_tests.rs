//! Selected native insert readings measure stereo signal independently of channel faders.

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
    for use_graph in [false, true] {
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
            for (track_id, effect_id) in
                [(return_id, return_effect), (TrackId::MASTER, master_effect)]
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
                commands.push(EngineCommand::SetRouting(routing)).unwrap();
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
                    "graph={use_graph}, master={monitor_master}: expected {expected}, got {actual}"
                );
            }
        }
    }
}

#[test]
fn gain_meter_backpressure_drops_old_windows_without_callback_allocation() {
    for use_graph in [false, true] {
        let (mut engine, _commands, mut events, track_id, effect_id) =
            gain_meter_fixture(use_graph);
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

#[test]
fn gain_meter_reports_bypass_and_audible_reenable_fade() {
    for use_graph in [false, true] {
        let (mut engine, mut commands, mut events, track_id, effect_id) =
            gain_meter_fixture(use_graph);
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
            let (input_levels, output_levels) =
                reading.expect("bypassed Gains still need readings");
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
                    "graph={use_graph}, bypass={bypass}: meter {actual}, heard {expected}"
                );
            }
            if bypass {
                assert_eq!(input_levels, output_levels);
            } else if use_graph {
                assert!((output_levels.peak_l - 0.8).abs() < 1e-5);
                assert!(output_levels.rms_l > 0.4 && output_levels.rms_l < 0.8);
            } else {
                assert!((output_levels.peak_l - 0.4).abs() < 1e-5);
                assert!((output_levels.rms_l - 0.4).abs() < 1e-5);
            }
        }
    }
}

#[test]
fn skipped_gain_processing_counts_as_silence_and_clears_old_peaks() {
    let (mut engine, mut commands, mut events, track_id, effect_id) = gain_meter_fixture(false);
    let input = [0.8, -0.4].repeat(960);
    let mut output = vec![0.0; input.len()];
    engine.process_block(
        AudioProcessBlock::new(&mut output[..960], 2)
            .with_live_input(track_id.raw(), &input[..960]),
    );
    while let Ok(event) = events.pop() {
        assert!(
            !matches!(event, EngineEvent::EffectMeter { .. }),
            "a half-window must not publish early"
        );
    }

    // Stopped, muted channels skip their effect chain in the fallback path.
    // Their elapsed audio time still belongs to the meter's RMS window.
    commands
        .push(EngineCommand::SetTrackMute(track_id, true))
        .unwrap();
    engine.process_block(
        AudioProcessBlock::new(&mut output[..960], 2)
            .with_live_input(track_id.raw(), &input[..960]),
    );
    assert!(output[..960].iter().all(|sample| *sample == 0.0));
    let mut reading = None;
    while let Ok(event) = events.pop() {
        if let EngineEvent::EffectMeter { input, output, .. } = event {
            assert!(reading.is_none());
            reading = Some((input, output));
        }
    }
    let (input_levels, output_levels) =
        reading.expect("a skipped chain still publishes its window");
    let half_window = std::f32::consts::FRAC_1_SQRT_2;
    for (actual, expected) in [
        (input_levels.peak_l, 0.8),
        (input_levels.peak_r, 0.4),
        (input_levels.rms_l, 0.8 * half_window),
        (input_levels.rms_r, 0.4 * half_window),
        (output_levels.peak_l, 0.4),
        (output_levels.peak_r, 0.2),
        (output_levels.rms_l, 0.4 * half_window),
        (output_levels.rms_r, 0.2 * half_window),
    ] {
        assert!(
            (actual - expected).abs() < 1e-5,
            "skipped frames must contribute silence: expected {expected}, got {actual}"
        );
    }

    engine.process_block(
        AudioProcessBlock::new(&mut output, 2).with_live_input(track_id.raw(), &input),
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
            assert_eq!(input, metering::MeterValues::SILENT);
            assert_eq!(output, metering::MeterValues::SILENT);
        }
    }
    assert_eq!(
        readings, 1,
        "a fully skipped window clears the visible meter"
    );
}

#[test]
fn legacy_gain_automation_keeps_linear_mapping_with_neutral_new_controls() {
    use vibez_core::automation::{AutomationLane, AutomationPoint, AutomationTarget};

    for use_graph in [false, true] {
        let (mut engine, mut commands, _events, track_id, effect_id) =
            gain_meter_fixture(use_graph);
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
            .push(EngineCommand::SetAutomationLane { track_id, lane })
            .unwrap();
        engine.process(&mut [], 2);
        if let Some(routing) = engine.routing.as_mut() {
            routing.configure_automation(&[(track_id, target)]).unwrap();
        }

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
                    "legacy normalized 0.25 must render half gain: expected {expected}, got {actual}, graph={use_graph}"
                );
            }
        }
        let effect = &engine.tracks[0].effects[0].effect;
        assert_eq!(effect.get_param(0), 0.5);
        for index in 1..5 {
            assert_eq!(effect.get_param(index), 0.0);
        }
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

#[test]
fn native_delay_meter_reports_real_insert_levels_before_channel_faders() {
    use vibez_dsp::delay::param;

    for use_graph in [false, true] {
        let (mut engine, mut commands, mut events) = AudioEngine::new();
        let track = TrackId::new();
        let effect = EffectId::new();
        commands.push(EngineCommand::SetSampleRate(48_000)).unwrap();
        commands.push(EngineCommand::add_track(track)).unwrap();
        commands
            .push(EngineCommand::insert_built_in_effect(
                track,
                effect,
                EffectType::Delay,
                48_000,
            ))
            .unwrap();
        for (param_index, value) in [
            (param::TIME_MS, 3.0),
            (param::LINK, 0.0),
            (param::RIGHT_TIME_MS, 5.0),
            (param::ROUTING, 1.0),
            (param::FEEDBACK, 0.5),
            (param::MIX, 0.25),
        ] {
            commands
                .push(EngineCommand::SetEffectParam {
                    track_id: track,
                    effect_id: effect,
                    param_index,
                    value,
                })
                .unwrap();
        }
        commands
            .push(EngineCommand::SetTrackGain(track, 0.125))
            .unwrap();
        commands
            .push(EngineCommand::SetTrackGain(TrackId::MASTER, 0.25))
            .unwrap();
        if use_graph {
            let routing = crate::routing::PreparedRouting::prepare(
                &[
                    RoutingChannel {
                        id: track,
                        is_bus: false,
                        effects: vec![RoutingEffect {
                            id: effect,
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
            .unwrap();
            commands.push(EngineCommand::SetRouting(routing)).unwrap();
        }
        let silence = [0.0_f32; 1920];
        let mut output = [0.0_f32; 1920];
        engine.process_block(
            AudioProcessBlock::new(&mut output, 2).with_live_input(track.raw(), &silence),
        );
        while events.pop().is_ok() {}
        commands
            .push(EngineCommand::SetEffectMeterChannel {
                track_id: Some(track),
                generation: 73,
            })
            .unwrap();

        let mut input = [0.0_f32; 1920];
        input[0] = 0.5;
        input[1] = -0.25;
        let mut expected_insert = [0.0_f32; 1920];
        expected_insert[0] = 0.375;
        expected_insert[1] = -0.1875;
        // Crossed feedback traverses the destination's 144 or 240 sample history.
        for (frame, channel, sample) in [
            (144, 0, 0.125),
            (240, 1, -0.0625),
            (384, 0, -0.03125),
            (384, 1, 0.0625),
            (528, 0, 0.03125),
            (624, 1, -0.015625),
            (768, 0, -0.0078125),
            (768, 1, 0.015625),
            (912, 0, 0.0078125),
        ] {
            expected_insert[2 * frame + channel] = sample;
        }
        let mut reading = None;
        let mut delay_state = None;
        for half in 0..2 {
            let range = half * 960..(half + 1) * 960;
            assert_eq!(
                crate::retirement::tests::allocations(|| {
                    engine.process_block(
                        AudioProcessBlock::new(&mut output[range.clone()], 2)
                            .with_live_input(track.raw(), &input[range.clone()]),
                    );
                }),
                (0, 0),
                "Delay meter callback heap activity, graph={use_graph}, half={half}",
            );
            while let Ok(event) = events.pop() {
                if let EngineEvent::EffectDelayState {
                    track_id,
                    effect_id,
                    generation,
                    snapshot,
                    ..
                } = &event
                {
                    assert_eq!(event.time_domain(), crate::events::TimeDomain::Untimed);
                    assert_eq!(half, 1);
                    assert_eq!((*track_id, *effect_id, *generation), (track, effect, 73));
                    assert!(delay_state
                        .replace(snapshot.expect("active Delay snapshot"))
                        .is_none());
                }
                if let EngineEvent::EffectMeter {
                    track_id,
                    effect_id,
                    generation,
                    input,
                    output,
                } = event
                {
                    assert_eq!(half, 1, "a 50Hz window must include all 960 frames");
                    assert_eq!((track_id, effect_id, generation), (track, effect, 73));
                    assert!(reading.replace((input, output)).is_none());
                }
            }
        }
        let post_fader_gain = 0.125 * 0.25 * std::f32::consts::FRAC_1_SQRT_2;
        for (sample, (&actual, &expected)) in output.iter().zip(&expected_insert).enumerate() {
            assert!(
                (actual - expected * post_fader_gain).abs() <= 2e-7,
                "graph={use_graph}, sample={sample}: heard={actual}, insert={expected}"
            );
        }
        let (input_levels, output_levels) =
            reading.expect("the selected native Delay must publish its real insert meter");
        let state = delay_state.expect("the live insert must report applied Delay state");
        assert_eq!(state.sample_rate, 48000);
        assert_eq!(state.processed_frames, 1920);
        assert_eq!(state.lines[0].old_samples, 144.0);
        assert_eq!(state.lines[1].old_samples, 240.0);
        assert_eq!(state.feedback, 0.5);
        assert_eq!(state.mix, 0.25);
        assert_eq!(state.routing, 1.0);
        assert!(!state.reference_moving);
        let mut squares = [0.0_f64; 2];
        for frame in expected_insert.as_chunks::<2>().0.iter() {
            for channel in 0..2 {
                squares[channel] += f64::from(frame[channel]).powi(2);
            }
        }
        for (actual, expected) in [
            (input_levels.peak_l, 0.5),
            (input_levels.peak_r, 0.25),
            (input_levels.rms_l, 0.5 / 960.0_f32.sqrt()),
            (input_levels.rms_r, 0.25 / 960.0_f32.sqrt()),
            (output_levels.peak_l, 0.375),
            (output_levels.peak_r, 0.1875),
            (output_levels.rms_l, (squares[0] / 960.0).sqrt() as f32),
            (output_levels.rms_r, (squares[1] / 960.0).sqrt() as f32),
        ] {
            assert!(
                (actual - expected).abs() <= 1e-6,
                "graph={use_graph}: meter={actual}, independent expected={expected}"
            );
        }
    }
}
