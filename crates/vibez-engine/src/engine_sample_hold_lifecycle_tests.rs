//! Actual owner selection and lifecycle retain native packet identity and signal meaning.

use super::*;
use vibez_core::{
    effect::{EffectType, SampleHoldMeter},
    id::EffectId,
    routing::{NodeStage, RoutingChannel, RoutingEffect, RoutingNode},
};
use crate::metering::MeterValues;

#[path = "engine_sample_hold_automation_tests.rs"]
mod automation;

#[path = "engine_sample_hold_rate_tests.rs"]
mod rate;

#[derive(Clone, Copy, Debug)]
enum Owner { Track, Return, Master }

struct Fixture {
    engine: AudioEngine,
    commands: Producer<EngineCommand>,
    events: Consumer<EngineEvent>,
    source: TrackId,
    selected: TrackId,
    effect: EffectId,
    graph: bool,
    owner: Owner,
}

impl Fixture {
    fn new(graph: bool, owner: Owner) -> Self {
        let (engine, mut commands, events) = AudioEngine::new();
        let source = TrackId::new();
        let selected = match owner { Owner::Track => source, Owner::Return => TrackId::new(), Owner::Master => TrackId::MASTER };
        let effect = EffectId::new();
        commands.push(EngineCommand::SetSampleRate(48_000)).unwrap();
        commands.push(EngineCommand::add_track(source)).unwrap();
        if matches!(owner, Owner::Return) {
            commands.push(EngineCommand::add_bus(selected)).unwrap();
            commands.push(EngineCommand::SetSend { track_id: source, bus_id: selected, amount: 1.0 }).unwrap();
        }
        for id in [source, selected, TrackId::MASTER] {
            commands.push(EngineCommand::SetTrackGain(id, 1.0)).unwrap();
            commands.push(EngineCommand::SetTrackPan(id, 0.5)).unwrap();
        }
        commands.push(EngineCommand::insert_built_in_effect(selected, effect, EffectType::Bitcrush, 48_000)).unwrap();
        for (param_index, value) in [4.0, 3.0, 0.25, 0.0, 0.0, 0.0, 0.0, 0.0, 12_000.0].into_iter().enumerate() {
            commands.push(EngineCommand::SetEffectParam { track_id: selected, effect_id: effect, param_index, value }).unwrap();
        }
        let mut fixture = Self { engine, commands, events, source, selected, effect, graph, owner };
        if graph {
            fixture.publish_routing(Some(effect));
        }
        fixture
    }

    fn publish_routing(&mut self, effect: Option<EffectId>) {
        let chain = || effect.into_iter().map(|id| RoutingEffect {
            id, inputs: vec![], assignments: vec![], inactive_inputs: vec![],
        }).collect();
        let mut channels = vec![RoutingChannel { id: self.source, is_bus: false,
            sends: if matches!(self.owner, Owner::Return) { vec![self.selected] } else { vec![] },
            effects: if matches!(self.owner, Owner::Track) { chain() } else { vec![] } }];
        if matches!(self.owner, Owner::Return) {
            channels.push(RoutingChannel { id: self.selected, is_bus: true, sends: vec![], effects: chain() });
        }
        channels.push(RoutingChannel { id: TrackId::MASTER, is_bus: true, sends: vec![],
            effects: if matches!(self.owner, Owner::Master) { chain() } else { vec![] } });
        let reports: Vec<_> = effect.into_iter().map(|id| (RoutingNode {
            channel: self.selected, stage: NodeStage::Effect(id),
        }, 0)).collect();
        let plan = crate::routing::PreparedRouting::prepare_compensated(&channels, 128, &reports, &[], 1).unwrap();
        self.commands.push(EngineCommand::SetRouting(plan)).unwrap();
    }

    fn select(&mut self, selected: bool, generation: u64) {
        self.commands.push(EngineCommand::SetEffectMeterChannel {
            track_id: selected.then_some(self.selected), generation,
        }).unwrap();
    }

    fn render(&mut self, frames: usize, value: [f32; 2]) {
        let mut input = [0.0; 1920];
        for frame in input.chunks_exact_mut(2).take(frames) { frame.copy_from_slice(&value); }
        let mut output = [0.0; 1920];
        self.engine.process_block(AudioProcessBlock::new(&mut output[..frames * 2], 2)
            .with_live_input(self.source.raw(), &input[..frames * 2]));
        assert!(output[..frames * 2].iter().all(|sample| sample.is_finite()));
    }

    fn observe(&mut self) -> Observed {
        let mut observed = Observed::default();
        while let Ok(event) = self.events.pop() {
            match event {
                EngineEvent::EffectSampleHold { track_id, effect_id, generation, meter } => {
                    observed.scopes.push((track_id, effect_id, generation, meter));
                }
                EngineEvent::EffectMeter { track_id, effect_id, generation, input, output } => {
                    observed.levels.push((track_id, effect_id, generation, input, output));
                }
                EngineEvent::CompensationInvalid { .. } | EngineEvent::CompensationFailed { .. }
                    | EngineEvent::DeviceProcessingFailed { .. } => panic!("unexpected fixture failure: {event:?}"),
                _ => {}
            }
        }
        observed
    }
}

#[derive(Default)]
struct Observed {
    scopes: Vec<(TrackId, EffectId, u64, SampleHoldMeter)>,
    levels: Vec<(TrackId, EffectId, u64, MeterValues, MeterValues)>,
}

fn assert_static(fixture: &Fixture, observed: &Observed, generation: u64) {
    assert_eq!(observed.scopes.len(), 1, "graph={}, owner={:?}", fixture.graph, fixture.owner);
    assert_eq!(observed.levels.len(), 1);
    let (track, effect, actual_generation, meter) = observed.scopes[0];
    assert_eq!((track, effect, actual_generation), (fixture.selected, fixture.effect, generation));
    let factor = if matches!(fixture.owner, Owner::Track) { 1.0 } else { std::f32::consts::FRAC_1_SQRT_2 };
    let expected_input = [0.3 * factor, -0.19 * factor];
    let held = if matches!(fixture.owner, Owner::Track) { [0.25, -0.25] } else { [0.25, -0.125] };
    let expected_output = [expected_input[0] * 0.75 + held[0] * 0.25,
        expected_input[1] * 0.75 + held[1] * 0.25];
    assert_eq!((meter.count, meter.sample_rate, meter.latency_samples), (128, 48_000.0, 0));
    assert_eq!((meter.primary_interval_frames, meter.secondary_interval_frames), (3, 0));
    assert!(!meter.transition_active);
    for point in 0..128 {
        assert!((meter.input[point] - expected_input[0]).abs() < 1e-6);
        assert!((meter.output[point] - expected_output[0]).abs() < 1e-6);
        let acquired = meter.primary_acquisitions[point / 64] & (1_u64 << (point % 64)) != 0;
        assert_eq!(acquired, (meter.first_source_frame + point as u64).is_multiple_of(3));
    }
    assert_eq!(meter.secondary_acquisitions, [0; 2]);
    assert_eq!(meter.blending, [0; 2]);
    let (track, effect, actual_generation, input, output) = observed.levels[0];
    assert_eq!((track, effect, actual_generation), (fixture.selected, fixture.effect, generation));
    for (meter, expected) in [(input, expected_input), (output, expected_output)] {
        for (actual, expected) in [(meter.peak_l, expected[0].abs()), (meter.peak_r, expected[1].abs()),
            (meter.rms_l, expected[0].abs()), (meter.rms_r, expected[1].abs())] {
            assert!((actual - expected).abs() < 1e-6, "level actual={actual}, expected={expected}");
        }
    }
    assert!(fixture.engine.compensation_valid);
}

#[test]
fn native_scope_and_levels_follow_selected_track_return_and_master_in_both_render_paths() {
    for graph in [false, true] {
        for owner in [Owner::Track, Owner::Return, Owner::Master] {
            let mut fixture = Fixture::new(graph, owner);
            fixture.select(true, 11);
            fixture.render(960, [0.3, -0.19]);
            let observed = fixture.observe();
            assert_static(&fixture, &observed, 11);
        }
    }
}

#[test]
fn native_scope_lifecycle_drains_old_selection_and_bypass_then_restarts_from_real_silence() {
    for graph in [false, true] {
        for owner in [Owner::Track, Owner::Return, Owner::Master] {
            let mut fixture = Fixture::new(graph, owner);
            fixture.render(960, [0.8, -0.6]);
            let absent = fixture.observe();
            assert!(absent.scopes.is_empty() && absent.levels.is_empty());
            fixture.select(true, 11);
            fixture.render(960, [0.3, -0.19]);
            let selected = fixture.observe();
            assert_static(&fixture, &selected, 11);
            fixture.select(false, 12);
            fixture.render(960, [0.7, -0.8]);
            let absent = fixture.observe();
            assert!(absent.scopes.is_empty() && absent.levels.is_empty());
            fixture.select(true, 13);
            fixture.render(960, [0.3, -0.19]);
            let selected = fixture.observe();
            assert_static(&fixture, &selected, 13);
            fixture.commands.push(EngineCommand::SetEffectBypass {
                track_id: fixture.selected, effect_id: fixture.effect, bypass: true,
            }).unwrap();
            fixture.render(960, [0.9, -0.75]);
            let bypassed = fixture.observe();
            assert_eq!(bypassed.scopes.len(), 1);
            assert_eq!(bypassed.scopes[0], (fixture.selected, fixture.effect, 13, SampleHoldMeter::default()));
            fixture.commands.push(EngineCommand::SetEffectBypass {
                track_id: fixture.selected, effect_id: fixture.effect, bypass: false,
            }).unwrap();
            fixture.render(960, [0.0, 0.0]);
            let silence = fixture.observe();
            assert_eq!(silence.scopes.len(), 1);
            assert_eq!(silence.scopes[0].3.count, 128);
            assert!(silence.scopes[0].3.input.iter().chain(&silence.scopes[0].3.output).all(|&value| value == 0.0));
            fixture.commands.push(EngineCommand::SetEffectParam {
                track_id: fixture.selected, effect_id: fixture.effect, param_index: 0, value: 2.0,
            }).unwrap();
            fixture.render(64, [0.8, -0.6]);
            assert!(fixture.observe().scopes.is_empty());
            fixture.commands.push(EngineCommand::Play).unwrap();
            fixture.render(960, [0.0, 0.0]);
            let restarted = fixture.observe();
            assert_eq!(restarted.scopes.len(), 1);
            let (track, effect, generation, meter) = restarted.scopes[0];
            assert_eq!((track, effect, generation), (fixture.selected, fixture.effect, 13));
            assert_eq!((meter.count, meter.first_source_frame), (128, 832));
            assert!(meter.input.iter().chain(&meter.output).all(|&value| value == 0.0));
            assert_eq!(meter.blending, [0; 2]);
            assert_eq!(meter.secondary_acquisitions, [0; 2]);
            assert_eq!(meter.secondary_interval_frames, 0);
            assert!(!meter.transition_active);
            fixture.commands.push(EngineCommand::RemoveEffect(fixture.selected, fixture.effect)).unwrap();
            if graph { fixture.publish_routing(None); }
            fixture.render(960, [0.3, -0.19]);
            let removed = fixture.observe();
            assert!(removed.scopes.is_empty() && removed.levels.is_empty());
        }
    }
}

#[test]
fn sample_hold_event_equality_observes_every_field_and_stays_untimed() {
    let track = TrackId::new();
    let effect = EffectId::new();
    let meter = SampleHoldMeter { input: [0.3; 128], output: [0.2875; 128], count: 128,
        sample_rate: 48_000.0, first_source_frame: 832, latency_samples: 0,
        primary_interval_frames: 3, secondary_interval_frames: 0, transition_active: false,
        primary_acquisitions: [0x9249, 0x2492], secondary_acquisitions: [0; 2], blending: [0; 2] };
    let event = |track_id, effect_id, generation, meter| EngineEvent::EffectSampleHold {
        track_id, effect_id, generation, meter,
    };
    assert_eq!(event(track, effect, 11, meter), event(track, effect, 11, meter));
    for changed in [event(TrackId::new(), effect, 11, meter), event(track, EffectId::new(), 11, meter),
        event(track, effect, 12, meter)] {
        assert_ne!(event(track, effect, 11, meter), changed);
    }
    for point in 0..128 {
        for output in [false, true] {
            let mut changed = meter;
            if output { changed.output[point] = -0.5; } else { changed.input[point] = -0.25; }
            assert_ne!(event(track, effect, 11, meter), event(track, effect, 11, changed));
        }
    }
    for field in 0..13 {
        let mut changed = meter;
        match field {
            0 => changed.count = 127,
            1 => changed.sample_rate = 96_000.0,
            2 => changed.first_source_frame += 1,
            3 => changed.latency_samples += 1,
            4 => changed.primary_interval_frames += 1,
            5 => changed.secondary_interval_frames = 5,
            6 => changed.transition_active = true,
            7 | 8 => changed.primary_acquisitions[field - 7] ^= 1_u64 << 63,
            9 | 10 => changed.secondary_acquisitions[field - 9] ^= 1_u64 << 63,
            11 | 12 => changed.blending[field - 11] ^= 1_u64 << 63,
            _ => unreachable!(),
        }
        assert_ne!(event(track, effect, 11, meter), event(track, effect, 11, changed), "field{field}");
    }
    assert_eq!(event(track, effect, 11, meter).time_domain(), crate::events::TimeDomain::Untimed);
}

fn fill_actual_event_ring(fixture: &mut Fixture) -> usize {
    let capacity = fixture.engine.event_tx.slots();
    assert_eq!(capacity, vibez_core::constants::RING_BUFFER_CAPACITY);
    let mut accepted = 0;
    while fixture.engine.event_tx.push(EngineEvent::PlaybackPosition(0)).is_ok() {
        accepted += 1;
    }
    assert_eq!(accepted, capacity);
    assert_eq!(fixture.engine.event_tx.slots(), 0);
    accepted
}

#[test]
fn full_actual_event_ring_drops_scope_windows_without_callback_heap_traffic_or_replay() {
    for graph in [false, true] {
        let mut fixture = Fixture::new(graph, Owner::Track);
        fixture.select(true, 21);
        fixture.render(960, [0.3, -0.19]);
        let warm = fixture.observe();
        assert_static(&fixture, &warm, 21);
        let occupied = fill_actual_event_ring(&mut fixture);
        println!("graph={graph}, actual event ring accepted={occupied}");
        let traffic = crate::retirement::tests::allocations(|| {
            for value in [[0.8, -0.6], [0.0, 0.0], [-0.7, 0.9], [0.0, 0.0]] {
                fixture.render(960, value);
            }
        });
        assert_eq!(traffic, (0, 0));
        let dropped = fixture.observe();
        assert!(dropped.scopes.is_empty() && dropped.levels.is_empty());
        assert_eq!(fixture.engine.event_tx.slots(), occupied);
        let traffic = crate::retirement::tests::allocations(|| {
            fixture.render(960, [0.3, -0.19]);
        });
        assert_eq!(traffic, (0, 0));
        let fresh = fixture.observe();
        assert_static(&fixture, &fresh, 21);
        assert_eq!(fresh.scopes[0].3.first_source_frame, 5_632);
        let traffic = crate::retirement::tests::allocations(|| {
            fixture.render(1, [0.41, -0.27]);
        });
        assert_eq!(traffic, (0, 0));
        let subdeadline = fixture.observe();
        assert!(subdeadline.scopes.is_empty() && subdeadline.levels.is_empty(),
            "free ring space cannot replay a previously dropped window");
    }
}

#[test]
fn full_actual_command_and_event_rings_keep_all_nine_latest_scalar_targets_without_replay() {
    let values = [
        [4.75, 5.49, 0.37, 6.0, 1.0, 1.0, 0.75, 1.0, 700.0],
        [12.25, 9.1, 0.65, 3.0, 2.0, 0.0, 0.25, 0.0, 4_000.0],
    ];
    for graph in [false, true] {
        let mut fixture = Fixture::new(graph, Owner::Track);
        fixture.select(true, 31);
        fixture.render(960, [0.3, -0.19]);
        assert_eq!(fixture.observe().scopes.len(), 1);
        let capacity = fixture.commands.slots();
        assert_eq!(capacity, vibez_core::constants::RING_BUFFER_CAPACITY);
        let mut expected = [4.0, 3.0, 0.25, 0.0, 0.0, 0.0, 0.0, 0.0, 12_000.0];
        let mut accepted = 0;
        let mut per_parameter = [0; 9];
        let rejected = loop {
            let index = accepted % expected.len();
            let value = values[(accepted / expected.len()) % 2][index];
            let command = EngineCommand::SetEffectParam { track_id: fixture.selected,
                effect_id: fixture.effect, param_index: index, value };
            match fixture.commands.push(command) {
                Ok(()) => {
                    expected[index] = value;
                    per_parameter[index] += 1;
                    accepted += 1;
                }
                Err(rtrb::PushError::Full(command)) => break command,
            }
        };
        assert_eq!(accepted, capacity);
        assert_eq!(fixture.commands.slots(), 0);
        for (index, &count) in per_parameter.iter().enumerate() {
            assert_eq!(count, accepted / 9 + usize::from(index < accepted % 9));
            assert!(count > 0);
        }
        let EngineCommand::SetEffectParam { param_index: rejected_index, value: rejected_value, .. } = rejected
            else { panic!("scalar command must be returned to its caller when full"); };
        assert_ne!(rejected_value, expected[rejected_index]);
        let occupied = fill_actual_event_ring(&mut fixture);
        println!("graph={graph}, actual command ring accepted={accepted}, per_parameter={per_parameter:?}, actual event ring accepted={occupied}");
        let traffic = crate::retirement::tests::allocations(|| {
            fixture.render(960, [0.8, -0.6]);
            fixture.render(960, [-0.4, 0.7]);
        });
        assert_eq!(traffic, (0, 0));
        assert_eq!(fixture.commands.slots(), capacity, "the accepted scalar batch was consumed");
        {
            let effect = &fixture.engine.tracks[0].effects[0].effect;
            for (index, value) in expected.into_iter().enumerate() {
                assert_eq!(effect.get_param(index), value, "accepted target{index}");
            }
            assert_ne!(effect.get_param(rejected_index), rejected_value);
        }
        let dropped = fixture.observe();
        assert!(dropped.scopes.is_empty() && dropped.levels.is_empty());
        fixture.commands.push(EngineCommand::SetEffectParam { track_id: fixture.selected,
            effect_id: fixture.effect, param_index: 2, value: 0.0 }).unwrap();
        let traffic = crate::retirement::tests::allocations(|| {
            fixture.render(960, [0.423, -0.219]);
        });
        assert_eq!(traffic, (0, 0));
        let fresh = fixture.observe();
        assert_eq!(fresh.scopes.len(), 1);
        assert_eq!(fresh.levels.len(), 1);
        let (track, effect, generation, meter) = fresh.scopes[0];
        assert_eq!((track, effect, generation), (fixture.selected, fixture.effect, 31));
        assert_eq!((meter.count, meter.first_source_frame), (128, 3_712));
        assert!(!meter.transition_active);
        assert!(meter.input.iter().all(|&value| value == 0.423));
        assert!(meter.output.iter().all(|&value| value == 0.423),
            "a newly accepted dry target wins after all queued ramps settle");
        expected[2] = 0.0;
        for (index, value) in expected.into_iter().enumerate() {
            assert_eq!(fixture.engine.tracks[0].effects[0].effect.get_param(index), value);
        }
        let traffic = crate::retirement::tests::allocations(|| {
            fixture.render(1, [-0.123, 0.456]);
        });
        assert_eq!(traffic, (0, 0));
        let subdeadline = fixture.observe();
        assert!(subdeadline.scopes.is_empty() && subdeadline.levels.is_empty());
        assert_eq!(fixture.engine.tracks[0].effects[0].effect.get_param(2), 0.0,
            "an old accepted or rejected scalar target cannot revive after the drain");
    }
}
