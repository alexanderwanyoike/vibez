//! Startup and plan publication preserve callback ownership and initial audio.

use super::*;
use graph_test_support::{routing_channel, routing_plan};

#[test]
fn first_project_plan_is_silent_in_the_event_protocol_and_has_no_initial_morph() {
    for playing_before_plan in [false, true] {
        let (mut engine, mut commands, mut events) = AudioEngine::new();
        let track = TrackId::new();
        commands.push(EngineCommand::add_track(track)).unwrap();
        if playing_before_plan {
            commands.push(EngineCommand::Play).unwrap();
        }
        commands
            .push(EngineCommand::SetRouting(routing_plan(&[
                routing_channel(track),
                routing_channel(TrackId::MASTER),
            ])))
            .unwrap();
        if !playing_before_plan {
            commands.push(EngineCommand::Play).unwrap();
        }
        let input = [0.25; 32];
        let mut output = [0.0; 32];
        assert_eq!(
            crate::retirement::tests::allocations(|| engine.process_block(
                AudioProcessBlock::new(&mut output, 2).with_live_input(track.raw(), &input)
            )),
            (0, 0)
        );
        let expected = 0.25 * std::f32::consts::FRAC_1_SQRT_2;
        assert!(output
            .iter()
            .all(|sample| (*sample - expected).abs() < 1e-6));
        assert_eq!(engine.compensation_plan_fade, 0);
        let protocol: Vec<_> = std::iter::from_fn(|| events.pop().ok())
            .filter_map(|event| match event {
                EngineEvent::PlaybackStarted => Some("started"),
                EngineEvent::PlaybackPosition(16) => Some("position"),
                EngineEvent::Metering { .. } => Some("metering"),
                EngineEvent::RoutingRetired(_) => Some("retired"),
                _ => None,
            })
            .collect();
        assert_eq!(protocol, ["started", "position", "metering"]);
    }
}

#[test]
fn first_plan_is_retained_without_callback_destruction_when_the_ui_ring_is_full() {
    let (mut engine, mut commands, mut events) = AudioEngine::new();
    while engine
        .event_tx
        .push(EngineEvent::PlaybackPosition(0))
        .is_ok()
    {}
    let plan = routing_plan(&[routing_channel(TrackId::MASTER)]);
    let expected = &*plan as *const crate::routing::PreparedRouting;
    commands.push(EngineCommand::SetRouting(plan)).unwrap();
    assert_eq!(
        crate::retirement::tests::allocations(|| engine.process(&mut [0.0; 2], 2)),
        (0, 0)
    );
    assert!(std::ptr::eq(engine.routing.as_deref().unwrap(), expected));
    assert!(engine.retired_routing.is_none());
    while events.pop().is_ok() {}
    assert_eq!(
        crate::retirement::tests::allocations(|| engine.process(&mut [0.0; 2], 2)),
        (0, 0)
    );
    assert!(!std::iter::from_fn(|| events.pop().ok())
        .any(|event| matches!(event, EngineEvent::RoutingRetired(_))));
}

#[test]
fn later_project_plans_still_return_the_previous_project_owner() {
    let (mut engine, mut commands, mut events) = AudioEngine::new();
    let first = routing_plan(&[routing_channel(TrackId::MASTER)]);
    let expected = &*first as *const crate::routing::PreparedRouting;
    commands.push(EngineCommand::SetRouting(first)).unwrap();
    commands
        .push(EngineCommand::SetRouting(routing_plan(&[routing_channel(
            TrackId::MASTER,
        )])))
        .unwrap();
    assert_eq!(
        crate::retirement::tests::allocations(|| engine.process(&mut [0.0; 2], 2)),
        (0, 0)
    );
    let retired: Vec<_> = std::iter::from_fn(|| events.pop().ok())
        .filter_map(|event| match event {
            EngineEvent::RoutingRetired(plan) => Some(plan),
            _ => None,
        })
        .collect();
    assert_eq!(retired.len(), 1);
    assert!(std::ptr::eq(&*retired[0], expected));
}
