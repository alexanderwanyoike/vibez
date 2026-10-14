//! Selected Drive snapshots retain real aligned samples through lossy observation.

use super::*;
use vibez_core::{effect::EffectType, id::EffectId};

#[test]
fn waveshaper_event_equality_compares_identity_generation_and_every_snapshot_field() {
    let track = TrackId::new();
    let effect = EffectId::new();
    let meter = vibez_core::effect::WaveShaperMeter {
        input: [0.25; 128],
        output: [0.5; 128],
        count: 128,
        sample_rate: 48_000.0,
        first_output_sample: 64,
        latency_samples: 64,
        transfer: [0.75; 65],
    };
    let event = |track_id, effect_id, generation, meter| EngineEvent::EffectWaveshaper {
        track_id,
        effect_id,
        generation,
        meter,
    };
    assert_eq!(event(track, effect, 7, meter), event(track, effect, 7, meter));
    for changed in [
        event(TrackId::new(), effect, 7, meter),
        event(track, EffectId::new(), 7, meter),
        event(track, effect, 8, meter),
    ] {
        assert_ne!(event(track, effect, 7, meter), changed);
    }
    for field in 0..7 {
        let mut changed = meter;
        match field {
            0 => changed.input[127] = -0.25,
            1 => changed.output[127] = -0.5,
            2 => changed.count = 127,
            3 => changed.sample_rate = 44_100.0,
            4 => changed.first_output_sample += 1,
            5 => changed.latency_samples += 1,
            6 => changed.transfer[64] = -0.75,
            _ => unreachable!(),
        }
        assert_ne!(event(track, effect, 7, meter), event(track, effect, 7, changed));
    }
    assert_eq!(event(track, effect, 7, meter).time_domain(), crate::events::TimeDomain::Untimed);
}

type Fixture = (
    AudioEngine,
    Producer<EngineCommand>,
    Consumer<EngineEvent>,
    TrackId,
    EffectId,
);

fn fixture() -> Fixture {
    let (engine, mut commands, events) = AudioEngine::new();
    let track_id = TrackId::new();
    let effect_id = EffectId::new();
    commands.push(EngineCommand::SetSampleRate(48_000)).unwrap();
    commands.push(EngineCommand::add_track(track_id)).unwrap();
    commands
        .push(EngineCommand::insert_built_in_effect(
            track_id,
            effect_id,
            EffectType::Drive,
            48_000,
        ))
        .unwrap();
    commands
        .push(EngineCommand::SetEffectParam {
            track_id,
            effect_id,
            param_index: 0,
            value: 0.0,
        })
        .unwrap();
    commands
        .push(EngineCommand::SetEffectMeterChannel {
            track_id: Some(track_id),
            generation: 7,
        })
        .unwrap();
    (engine, commands, events, track_id, effect_id)
}

fn render_window(engine: &mut AudioEngine, track: TrackId, amplitude: f32) -> [f32; 1920] {
    let input = [amplitude; 1920];
    let mut output = [0.0; 1920];
    engine.process_block(
        AudioProcessBlock::new(&mut output, 2).with_live_input(track.raw(), &input),
    );
    output
}

fn snapshots(
    events: &mut Consumer<EngineEvent>,
) -> Vec<(TrackId, EffectId, u64, vibez_core::effect::WaveShaperMeter)> {
    std::iter::from_fn(|| events.pop().ok())
        .filter_map(|event| match event {
            EngineEvent::EffectWaveshaper {
                track_id,
                effect_id,
                generation,
                meter,
            } => Some((track_id, effect_id, generation, meter)),
            _ => None,
        })
        .collect()
}

#[test]
fn drive_snapshots_follow_selected_track_return_and_master_in_both_render_paths() {
    use vibez_core::routing::{NodeStage, RoutingChannel, RoutingEffect, RoutingNode};

    for graph in [false, true] {
        for (master, bus) in [(false, false), (false, true), (true, false)] {
            let (mut engine, mut commands, mut events) = AudioEngine::new();
            let source = TrackId::new();
            let selected = if master { TrackId::MASTER } else if bus { TrackId::new() } else { source };
            let effect = EffectId::new();
            commands.push(EngineCommand::SetSampleRate(48_000)).unwrap();
            commands.push(EngineCommand::add_track(source)).unwrap();
            if bus {
                commands.push(EngineCommand::add_bus(selected)).unwrap();
                commands.push(EngineCommand::SetSend {
                    track_id: source,
                    bus_id: selected,
                    amount: 1.0,
                }).unwrap();
            }
            commands.push(EngineCommand::insert_built_in_effect(
                selected, effect, EffectType::Drive, 48_000,
            )).unwrap();
            commands.push(EngineCommand::SetEffectParam {
                track_id: selected,
                effect_id: effect,
                param_index: 0,
                value: 0.0,
            }).unwrap();
            if graph {
                let chain = || vec![RoutingEffect {
                    id: effect,
                    inputs: vec![],
                    assignments: vec![],
                    inactive_inputs: vec![],
                }];
                let mut channels = vec![RoutingChannel {
                    id: source,
                    is_bus: false,
                    effects: if !master && !bus { chain() } else { vec![] },
                    sends: if bus { vec![selected] } else { vec![] },
                }];
                if bus {
                    channels.push(RoutingChannel {
                        id: selected,
                        is_bus: true,
                        effects: chain(),
                        sends: vec![],
                    });
                }
                channels.push(RoutingChannel {
                    id: TrackId::MASTER,
                    is_bus: true,
                    effects: if master { chain() } else { vec![] },
                    sends: vec![],
                });
                let reports = [(RoutingNode { channel: selected, stage: NodeStage::Effect(effect) }, 64)];
                commands.push(EngineCommand::SetRouting(
                    crate::routing::PreparedRouting::prepare_compensated(
                        &channels, 128, &reports, &[], 1,
                    ).unwrap(),
                )).unwrap();
            }
            commands.push(EngineCommand::SetEffectMeterChannel {
                track_id: Some(selected),
                generation: 11,
            }).unwrap();
            render_window(&mut engine, source, 0.25);
            let windows = snapshots(&mut events);
            assert_eq!(windows.len(), 1, "graph={graph}, master={master}, bus={bus}");
            assert_eq!((windows[0].0, windows[0].1, windows[0].2), (selected, effect, 11));
            let meter = windows[0].3;
            assert_eq!((meter.count, meter.latency_samples), (128, 64));
            assert_eq!(meter.sample_rate, 48_000.0);
            let expected = 0.25 * if selected == source { 1.0 } else { std::f32::consts::FRAC_1_SQRT_2 };
            assert!(meter.input.iter().all(|sample| (*sample - expected).abs() < 1e-6));
            assert!(meter.output.iter().all(|sample| (*sample - expected).abs() < 1e-6));
            assert!(engine.compensation_valid);
        }
    }
}

#[test]
fn drive_full_ring_drops_snapshots_without_callback_allocation_or_retry_backlog() {
    let (mut engine, _commands, mut events, track, effect) = fixture();
    render_window(&mut engine, track, 0.5);
    assert_eq!(snapshots(&mut events).len(), 1);
    let mut occupied = 0;
    while engine.event_tx.push(EngineEvent::PlaybackPosition(0)).is_ok() {
        occupied += 1;
    }
    assert_eq!(occupied, 1024, "telemetry must keep the bounded event ring");
    let measured = crate::retirement::tests::allocations(|| {
        render_window(&mut engine, track, 0.8);
        for _ in 0..4 {
            render_window(&mut engine, track, 0.0);
        }
    });
    assert_eq!(measured, (0, 0));
    assert!(snapshots(&mut events).is_empty());

    let measured = crate::retirement::tests::allocations(|| {
        render_window(&mut engine, track, 0.03);
    });
    assert_eq!(measured, (0, 0));
    let fresh = snapshots(&mut events);
    assert_eq!(fresh.len(), 1, "dropped windows cannot queue a retry backlog");
    assert_eq!((fresh[0].0, fresh[0].1, fresh[0].2), (track, effect, 7));
    assert_eq!(fresh[0].3.count, 128);
    assert_eq!(fresh[0].3.latency_samples, 64);
    assert!(fresh[0].3.input.iter().all(|sample| (*sample - 0.03).abs() < 1e-6));
    assert!(fresh[0].3.output.iter().all(|sample| (*sample - 0.03).abs() < 1e-6));
}

#[test]
fn drive_snapshots_clear_on_bypass_and_resume_with_fresh_selection_generation() {
    let (mut engine, mut commands, mut events, track, effect) = fixture();
    render_window(&mut engine, track, 0.5);
    assert_eq!(snapshots(&mut events)[0].3.count, 128);
    commands
        .push(EngineCommand::SetEffectMeterChannel {
            track_id: None,
            generation: 8,
        })
        .unwrap();
    render_window(&mut engine, track, 0.9);
    assert!(snapshots(&mut events).is_empty());
    commands
        .push(EngineCommand::SetEffectMeterChannel {
            track_id: Some(track),
            generation: 9,
        })
        .unwrap();
    render_window(&mut engine, track, 0.03);
    let fresh = snapshots(&mut events);
    assert_eq!(fresh.len(), 1);
    assert_eq!((fresh[0].0, fresh[0].1, fresh[0].2), (track, effect, 9));
    assert_eq!(fresh[0].3.count, 128);
    assert!(fresh[0].3.input.iter().all(|sample| (*sample - 0.03).abs() < 1e-6));
    assert!(fresh[0].3.output.iter().all(|sample| (*sample - 0.03).abs() < 1e-6));

    commands
        .push(EngineCommand::SetEffectBypass {
            track_id: track,
            effect_id: effect,
            bypass: true,
        })
        .unwrap();
    render_window(&mut engine, track, 0.8);
    let bypassed = snapshots(&mut events);
    assert_eq!(bypassed.len(), 1);
    assert_eq!(bypassed[0].2, 9);
    assert_eq!(bypassed[0].3.count, 0);

    commands
        .push(EngineCommand::SetEffectBypass {
            track_id: track,
            effect_id: effect,
            bypass: false,
        })
        .unwrap();
    render_window(&mut engine, track, 0.0);
    let silence = snapshots(&mut events);
    assert_eq!(silence.len(), 1);
    assert_eq!(silence[0].3.count, 128, "real silence is available data");
    assert!(silence[0].3.input.iter().all(|sample| *sample == 0.0));
    assert!(silence[0].3.output.iter().all(|sample| *sample == 0.0));

    commands.push(EngineCommand::Play).unwrap();
    render_window(&mut engine, track, 0.0);
    let reset = snapshots(&mut events);
    assert_eq!(reset.len(), 1);
    assert_eq!(reset[0].3.first_output_sample, 832);
    assert!(reset[0].3.input.iter().all(|sample| *sample == 0.0));
    assert!(reset[0].3.output.iter().all(|sample| *sample == 0.0));
}

#[test]
fn drive_waveform_publishes_real_chronological_pairs_at_fifty_hertz() {
    let (mut engine, _commands, mut events, track_id, effect_id) = fixture();
    let input: [f32; 1920] = std::array::from_fn(|index| {
        if index % 2 == 0 {
            0.01 + index as f32 * 0.000_005
        } else {
            -0.7
        }
    });
    let mut output = [0.0; 1920];
    engine.process_block(
        AudioProcessBlock::new(&mut output[..960], 2)
            .with_live_input(track_id.raw(), &input[..960]),
    );
    assert!(std::iter::from_fn(|| events.pop().ok())
        .all(|event| !matches!(event, EngineEvent::EffectWaveshaper { .. })));
    engine.process_block(
        AudioProcessBlock::new(&mut output[960..], 2)
            .with_live_input(track_id.raw(), &input[960..]),
    );
    let windows: Vec<_> = std::iter::from_fn(|| events.pop().ok())
        .filter_map(|event| match event {
            EngineEvent::EffectWaveshaper {
                track_id: observed_track,
                effect_id: observed_effect,
                generation,
                meter,
            } => {
                assert_eq!((observed_track, observed_effect, generation), (track_id, effect_id, 7));
                Some(meter)
            }
            _ => None,
        })
        .collect();
    assert_eq!(windows.len(), 1, "the selected Drive needs a genuine window");
    let meter = windows[0];
    assert_eq!(meter.count, 128);
    assert_eq!(meter.sample_rate, 48_000.0);
    assert_eq!(meter.latency_samples, 64);
    assert_eq!(meter.first_output_sample, 832);
    for index in 0..usize::from(meter.count) {
        let source_frame = meter.first_output_sample as usize + index - 64;
        let expected = input[source_frame * 2];
        assert!((meter.input[index] - expected).abs() < 1e-6);
        assert!((meter.output[index] - expected).abs() < 1e-6);
    }
}
