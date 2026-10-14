//! Genuine wet attenuation must survive dry mixes and lossy UI observation.

use super::*;
use vibez_core::effect::{DynamicsMeter, EffectType, GatePhase};
use vibez_core::id::EffectId;
use vibez_core::routing::{RoutingChannel, RoutingEffect};

fn readings(events: &mut Consumer<EngineEvent>) -> Vec<(TrackId, EffectId, u64, DynamicsMeter)> {
    std::iter::from_fn(|| events.pop().ok())
        .filter_map(|event| match event {
            EngineEvent::EffectDynamics {
                track_id,
                effect_id,
                generation,
                meter,
            } => Some((track_id, effect_id, generation, meter)),
            _ => None,
        })
        .collect()
}

type GateFixture = (
    AudioEngine,
    Producer<EngineCommand>,
    Consumer<EngineEvent>,
    TrackId,
    EffectId,
);

fn gate_fixture(graph: bool) -> GateFixture {
    use vibez_dsp::gate::param;

    let (engine, mut commands, events) = AudioEngine::new();
    let track = TrackId::new();
    let effect = EffectId::new();
    commands.push(EngineCommand::SetSampleRate(48000)).unwrap();
    commands.push(EngineCommand::add_track(track)).unwrap();
    commands.push(EngineCommand::insert_built_in_effect(track, effect, EffectType::Gate, 48000)).unwrap();
    for (param_index, value) in [
        (param::THRESHOLD, -18.0),
        (param::ATTACK, 0.1),
        (param::RELEASE, 5.0),
        (param::HOLD, 200.0),
        (param::HYSTERESIS, 6.0),
        (param::RANGE_ENABLED, 1.0),
        (param::RANGE, 60.0),
        (param::MIX, 0.0),
    ] {
        commands.push(EngineCommand::SetEffectParam { track_id: track, effect_id: effect, param_index, value }).unwrap();
    }
    if graph {
        let channels = [
            RoutingChannel { id: track, is_bus: false, sends: vec![], effects: vec![RoutingEffect {
                id: effect, inputs: vec![], assignments: vec![], inactive_inputs: vec![],
            }] },
            RoutingChannel { id: TrackId::MASTER, is_bus: true, sends: vec![], effects: vec![] },
        ];
        commands.push(EngineCommand::SetRouting(crate::routing::PreparedRouting::prepare(&channels, 128).unwrap())).unwrap();
    }
    commands.push(EngineCommand::SetEffectMeterChannel { track_id: Some(track), generation: 7 }).unwrap();
    (engine, commands, events, track, effect)
}

fn render_gate_window(engine: &mut AudioEngine, track: TrackId, amplitude: f32, output: &mut [f32; 1920]) {
    let input = [amplitude; 1920];
    engine.process_block(AudioProcessBlock::new(output, 2).with_live_input(track.raw(), &input));
}

#[test]
fn gate_windows_drop_under_full_ring_pressure_without_allocating_or_retrying_old_decisions() {
    for graph in [false, true] {
        let (mut engine, _commands, mut events, track, effect) = gate_fixture(graph);
        let mut output = [0.0; 1920];
        render_gate_window(&mut engine, track, 0.5, &mut output);
        let dry_scale = output[1918] / 0.5;
        let initial = readings(&mut events);
        assert_eq!(initial.len(), 1);
        assert_eq!((initial[0].0, initial[0].1, initial[0].2), (track, effect, 7));
        assert_eq!(initial[0].3.gate.unwrap().phase, GatePhase::Open);

        while engine.event_tx.push(EngineEvent::PlaybackPosition(0)).is_ok() {}
        let measured = crate::retirement::tests::allocations(|| {
            render_gate_window(&mut engine, track, 0.8, &mut output);
            for _ in 0..4 {
                render_gate_window(&mut engine, track, 0.0, &mut output);
            }
        });
        assert_eq!(measured, (0, 0));
        assert!(readings(&mut events).is_empty());

        let measured = crate::retirement::tests::allocations(|| {
            render_gate_window(&mut engine, track, 0.01, &mut output);
        });
        assert_eq!(measured, (0, 0));
        let fresh = readings(&mut events);
        assert_eq!(fresh.len(), 1, "dropped Gate windows must not queue a retry backlog");
        let meter = fresh[0].3;
        let gate = meter.gate.unwrap();
        assert_eq!((fresh[0].0, fresh[0].1, fresh[0].2), (track, effect, 7));
        assert_eq!(meter.detector_peak, [0.01; 2]);
        assert_eq!(gate.phase, GatePhase::Hold);
        assert_eq!(gate.phases_seen, GatePhase::Hold as u8);
        assert!((90.0..110.0).contains(&gate.hold_remaining_ms), "{gate:?}");
        assert!((gate.peak_envelope_db + 40.0).abs() < 0.002, "{gate:?}");
        assert_eq!((gate.open_threshold_db, gate.close_threshold_db), (-18.0, -24.0));
        assert!(output.iter().all(|sample| (*sample - 0.01 * dry_scale).abs() < 1e-7));

        let measured = crate::retirement::tests::allocations(|| {
            for _ in 0..7 {
                render_gate_window(&mut engine, track, 0.01, &mut output);
            }
        });
        assert_eq!(measured, (0, 0));
        let closed = readings(&mut events);
        assert_eq!(closed.len(), 7);
        let meter = closed.last().unwrap().3;
        let gate = meter.gate.unwrap();
        assert_eq!(gate.phase, GatePhase::Closed);
        assert_eq!(gate.phases_seen, GatePhase::Closed as u8);
        assert_eq!(gate.hold_remaining_ms, 0.0);
        assert_eq!(meter.detector_peak, [0.01; 2]);
        assert!(meter.current_reduction_db[0] > 40.0, "{meter:?}");
        assert!(output.iter().all(|sample| (*sample - 0.01 * dry_scale).abs() < 1e-7));
    }
}

#[test]
fn gate_windows_respect_generation_bypass_and_transport_reset_without_old_peaks_or_phases() {
    for graph in [false, true] {
        let (mut engine, mut commands, mut events, track, effect) = gate_fixture(graph);
        let mut output = [0.0; 1920];
        render_gate_window(&mut engine, track, 0.5, &mut output);
        readings(&mut events);
        commands.push(EngineCommand::SetEffectMeterChannel { track_id: None, generation: 8 }).unwrap();
        render_gate_window(&mut engine, track, 0.8, &mut output);
        render_gate_window(&mut engine, track, 0.01, &mut output);
        assert!(readings(&mut events).is_empty());

        commands.push(EngineCommand::SetEffectMeterChannel { track_id: Some(track), generation: 9 }).unwrap();
        render_gate_window(&mut engine, track, 0.001, &mut output);
        let fresh = readings(&mut events);
        assert_eq!(fresh.len(), 1);
        assert_eq!(fresh[0].2, 9);
        assert_eq!(fresh[0].3.detector_peak, [0.001; 2]);
        let gate = fresh[0].3.gate.unwrap();
        assert_eq!(gate.phase, GatePhase::Hold);
        assert_eq!(gate.phases_seen, GatePhase::Hold as u8);
        assert!(gate.peak_envelope_db < -35.0, "{gate:?}");

        commands.push(EngineCommand::SetEffectBypass { track_id: track, effect_id: effect, bypass: true }).unwrap();
        commands.push(EngineCommand::SetEffectMeterChannel { track_id: Some(track), generation: 10 }).unwrap();
        render_gate_window(&mut engine, track, 0.9, &mut output);
        let bypassed = readings(&mut events);
        assert_eq!(bypassed.len(), 1);
        assert_eq!(bypassed[0].2, 10);
        assert_eq!(bypassed[0].3, DynamicsMeter::default());

        commands.push(EngineCommand::SetEffectBypass { track_id: track, effect_id: effect, bypass: false }).unwrap();
        commands.push(EngineCommand::SetEffectMeterChannel { track_id: Some(track), generation: 11 }).unwrap();
        render_gate_window(&mut engine, track, 0.002, &mut output);
        let resumed = readings(&mut events);
        assert_eq!(resumed.len(), 1);
        assert_eq!(resumed[0].2, 11);
        assert_eq!(resumed[0].3.detector_peak, [0.002; 2]);
        let resumed_gate = resumed[0].3.gate.unwrap();
        // Prepared graphs reset stopped wet DSP on re-entry; direct slots retain it.
        let expected = if graph { GatePhase::Closed } else { GatePhase::Hold };
        assert_eq!(resumed_gate.phase, expected);
        assert_eq!(resumed_gate.phases_seen, expected as u8);
        if graph {
            assert_eq!(resumed_gate.hold_remaining_ms, 0.0);
        } else {
            assert!(resumed_gate.hold_remaining_ms > 0.0);
        }

        commands.push(EngineCommand::Play).unwrap();
        commands.push(EngineCommand::SetEffectMeterChannel { track_id: Some(track), generation: 12 }).unwrap();
        render_gate_window(&mut engine, track, 0.0, &mut output);
        let reset = readings(&mut events);
        assert_eq!(reset.len(), 1);
        assert_eq!(reset[0].2, 12);
        assert_eq!(reset[0].3.detector_peak, [0.0; 2]);
        let gate = reset[0].3.gate.unwrap();
        assert_eq!(gate.phase, GatePhase::Closed);
        assert_eq!(gate.phases_seen, GatePhase::Closed as u8);
        assert_eq!(gate.hold_remaining_ms, 0.0);
        assert!(gate.peak_envelope_db < -120.0, "{gate:?}");
        assert!(gate.envelope_db < -190.0, "{gate:?}");
        assert!(output.iter().all(|sample| *sample == 0.0));
    }
}
