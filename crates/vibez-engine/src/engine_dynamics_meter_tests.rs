//! Genuine wet attenuation must survive dry mixes and lossy UI observation.

use super::*;
use vibez_core::effect::{DynamicsMeter, EffectType};
use vibez_core::id::EffectId;
use vibez_core::routing::{RoutingChannel, RoutingEffect};

type Fixture = (
    AudioEngine,
    Producer<EngineCommand>,
    Consumer<EngineEvent>,
    TrackId,
    [EffectId; 2],
);

fn fixture(master: bool, bus: bool) -> Fixture {
    let (engine, mut commands, events) = AudioEngine::new();
    let source = TrackId::new();
    let selected = if master {
        TrackId::MASTER
    } else if bus {
        TrackId::new()
    } else {
        source
    };
    let effects = [EffectId::new(), EffectId::new()];
    commands.push(EngineCommand::SetSampleRate(48000)).unwrap();
    commands.push(EngineCommand::add_track(source)).unwrap();
    if bus {
        commands.push(EngineCommand::add_bus(selected)).unwrap();
        commands
            .push(EngineCommand::SetSend {
                track_id: source,
                bus_id: selected,
                amount: 1.0,
            })
            .unwrap();
    }
    for (index, effect_id) in effects.into_iter().enumerate() {
        commands
            .push(EngineCommand::insert_built_in_effect(
                selected,
                effect_id,
                EffectType::Compressor,
                48000,
            ))
            .unwrap();
        for (param_index, value) in [
            (0, if index == 0 { -18.0 } else { -12.0 }),
            (1, 4.0),
            (2, 0.1),
            (4, 24.0),
            (5, 0.0),
        ] {
            commands
                .push(EngineCommand::SetEffectParam {
                    track_id: selected,
                    effect_id,
                    param_index,
                    value,
                })
                .unwrap();
        }
    }
    let chain = || {
        effects
            .into_iter()
            .map(|id| RoutingEffect {
                id,
                inputs: vec![],
                assignments: vec![],
                inactive_inputs: vec![],
            })
            .collect()
    };
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
    commands
        .push(EngineCommand::SetRouting(
            crate::routing::PreparedRouting::prepare(&channels, 128).unwrap(),
        ))
        .unwrap();
    commands
        .push(EngineCommand::SetEffectMeterChannel {
            track_id: Some(selected),
            generation: 7,
        })
        .unwrap();
    (engine, commands, events, source, effects)
}

fn render(engine: &mut AudioEngine, track: TrackId, amplitude: f32) {
    let input = [amplitude, amplitude * 0.25].repeat(960);
    let mut output = [0.0; 1920];
    engine
        .process_block(AudioProcessBlock::new(&mut output, 2).with_live_input(track.raw(), &input));
}

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

#[test]
fn dynamics_are_real_wet_reduction_for_multiple_inserts_even_when_mix_is_dry() {
    let (mut engine, _commands, mut events, track, effects) = fixture(false, false);
    for _ in 0..5 {
        render(&mut engine, track, 0.5);
    }
    let windows = readings(&mut events);
    assert_eq!(windows.len(), 10);
    for (effect, threshold) in effects.into_iter().zip([-18.0, -12.0]) {
        let (_, _, generation, meter) = windows
            .iter()
            .rev()
            .find(|value| value.1 == effect)
            .unwrap();
        assert_eq!(*generation, 7);
        let expected = (20.0 * 0.5_f32.log10() - threshold) * 0.75;
        assert!(
            (meter.current_reduction_db[0] - expected).abs() < 0.002,
            "{meter:?}"
        );
        assert_eq!(meter.current_reduction_db[0], meter.current_reduction_db[1]);
        assert!(meter.peak_reduction_db[0] >= meter.current_reduction_db[0]);
        assert_eq!(meter.detector_peak, [0.5, 0.125]);
    }
}

#[test]
fn dynamics_clear_on_bypass_and_resume_only_under_the_new_generation() {
    let (mut engine, mut commands, mut events, track, effects) = fixture(false, false);
    render(&mut engine, track, 0.5);
    readings(&mut events);
    commands
        .push(EngineCommand::SetEffectBypass {
            track_id: track,
            effect_id: effects[0],
            bypass: true,
        })
        .unwrap();
    commands
        .push(EngineCommand::SetEffectMeterChannel {
            track_id: Some(track),
            generation: 8,
        })
        .unwrap();
    render(&mut engine, track, 0.5);
    let windows = readings(&mut events);
    assert!(windows.iter().all(|window| window.2 == 8));
    assert_eq!(
        windows
            .iter()
            .find(|window| window.1 == effects[0])
            .unwrap()
            .3,
        DynamicsMeter::default()
    );
    commands
        .push(EngineCommand::SetEffectBypass {
            track_id: track,
            effect_id: effects[0],
            bypass: false,
        })
        .unwrap();
    render(&mut engine, track, 0.5);
    assert!(
        readings(&mut events)
            .iter()
            .find(|window| window.1 == effects[0])
            .unwrap()
            .3
            .current_reduction_db[0]
            > 1.0
    );
}

#[test]
fn detector_windows_drain_and_resubscription_does_not_report_old_peaks() {
    let (mut engine, mut commands, mut events, track, _effects) = fixture(false, false);
    render(&mut engine, track, 0.8);
    readings(&mut events);
    commands
        .push(EngineCommand::SetEffectMeterChannel {
            track_id: None,
            generation: 8,
        })
        .unwrap();
    render(&mut engine, track, 1.0);
    commands
        .push(EngineCommand::SetEffectMeterChannel {
            track_id: Some(track),
            generation: 9,
        })
        .unwrap();
    render(&mut engine, track, 0.1);
    let windows = readings(&mut events);
    assert_eq!(windows.len(), 2);
    assert!(windows
        .iter()
        .all(|window| window.2 == 9 && window.3.detector_peak == [0.1, 0.025]));
}

#[test]
fn return_and_master_dynamics_are_selected_by_channel_identity() {
    for (master, bus) in [(false, true), (true, false)] {
        let (mut engine, _commands, mut events, source, _effects) = fixture(master, bus);
        render(&mut engine, source, 0.8);
        let windows = readings(&mut events);
        assert_eq!(windows.len(), 2);
        assert!(
            windows
                .iter()
                .all(|window| window.0.is_master() == master
                    && window.3.current_reduction_db[0] > 1.0)
        );
    }
}

#[test]
fn dynamics_publication_uses_the_existing_fifty_hertz_window_and_is_lossy_without_allocating() {
    let (mut engine, _commands, mut events, track, _effects) = fixture(false, false);
    let input = [0.5; 960];
    let mut output = [0.0; 960];
    engine
        .process_block(AudioProcessBlock::new(&mut output, 2).with_live_input(track.raw(), &input));
    assert!(readings(&mut events).is_empty());
    engine
        .process_block(AudioProcessBlock::new(&mut output, 2).with_live_input(track.raw(), &input));
    assert_eq!(readings(&mut events).len(), 2);
    while engine
        .event_tx
        .push(EngineEvent::PlaybackPosition(0))
        .is_ok()
    {}
    assert_eq!(
        crate::retirement::tests::allocations(|| {
            for _ in 0..4 {
                engine.process_block(
                    AudioProcessBlock::new(&mut output, 2).with_live_input(track.raw(), &input),
                );
            }
        }),
        (0, 0)
    );
    while events.pop().is_ok() {}
    engine
        .process_block(AudioProcessBlock::new(&mut output, 2).with_live_input(track.raw(), &input));
    engine
        .process_block(AudioProcessBlock::new(&mut output, 2).with_live_input(track.raw(), &input));
    let windows = readings(&mut events);
    assert_eq!(
        windows.len(),
        2,
        "dropped windows must not queue a retry backlog"
    );
}
