//! Graph fixtures must expose the same live-input mix and callback ownership as production routing.

use super::*;
use graph_test_support::{graph_engine, routing_channel};
use vibez_core::routing::RoutingChannel;

#[test]
fn graph_fixture_publishes_a_master_plan_before_the_first_callback() {
    let (mut engine, _commands, _events) = graph_engine(&[routing_channel(TrackId::MASTER)]);
    let routing = engine.routing.as_ref();
    assert_eq!(routing.channels.len(), 1);
    assert_eq!(routing.graph.nodes.last().unwrap().channel, TrackId::MASTER);
    let mut output = [1.0; 32];
    assert_eq!(
        crate::retirement::tests::allocations(|| engine.process(&mut output, 2)),
        (0, 0)
    );
    assert_eq!(output, [0.0; 32]);
}

#[test]
fn graph_fixture_renders_live_input_through_send_return_and_master() {
    let track = TrackId::new();
    let bus = TrackId::new();
    let channels = [
        RoutingChannel {
            sends: vec![bus],
            ..routing_channel(track)
        },
        RoutingChannel {
            is_bus: true,
            ..routing_channel(bus)
        },
        routing_channel(TrackId::MASTER),
    ];
    let (mut engine, mut commands, mut events) = graph_engine(&channels);
    commands.push(EngineCommand::add_track(track)).unwrap();
    commands.push(EngineCommand::add_bus(bus)).unwrap();
    commands
        .push(EngineCommand::SetSend {
            track_id: track,
            bus_id: bus,
            amount: 1.0,
        })
        .unwrap();
    commands
        .push(EngineCommand::SetTrackGain(bus, 0.5))
        .unwrap();
    commands
        .push(EngineCommand::SetTrackGain(TrackId::MASTER, 0.8))
        .unwrap();
    let input = [0.25, -0.5, 0.25, -0.5];
    let mut output = [0.0; 4];
    engine
        .process_block(AudioProcessBlock::new(&mut output, 2).with_live_input(track.raw(), &input));
    let centre = std::f32::consts::FRAC_1_SQRT_2;
    for (actual, input) in output.iter().zip(input) {
        assert!((*actual - input * centre * 1.5 * 0.8).abs() < 1e-6);
    }
    while events.pop().is_ok() {}
    assert_eq!(
        crate::retirement::tests::allocations(|| engine.process_block(
            AudioProcessBlock::new(&mut output, 2).with_live_input(track.raw(), &input)
        )),
        (0, 0)
    );
}
