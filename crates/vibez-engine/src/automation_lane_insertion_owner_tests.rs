//! Immediate lane edits retain current outer storage and return each point/control owner off callback.

use super::*;
use crate::commands::EngineCommand;
use crate::engine::{AudioEngine, AudioProcessBlock};
use crate::events::EngineEvent;
use crate::mixer::EngineTrack;
use vibez_core::automation::{AutomationPoint, AutomationTarget};

fn lane(target: AutomationTarget, value: f32) -> AutomationLane {
    let mut lane = AutomationLane::new(target);
    lane.points = vec![
        AutomationPoint {
            beat: 0.0,
            value,
            curve: -0.5,
        },
        AutomationPoint {
            beat: 2.25,
            value,
            curve: 0.75,
        },
    ];
    lane
}

fn setup(
    lanes: Vec<AutomationLane>,
) -> (
    AudioEngine,
    Producer<EngineCommand>,
    Consumer<EngineEvent>,
    TrackId,
) {
    let track = TrackId::new();
    let source = PreparedPlaybackSource::new(Vec::new(), Vec::new(), lanes);
    let (mut engine, mut commands, mut events) = AudioEngine::new();
    commands
        .push(EngineCommand::AddTrack(Box::new(
            EngineTrack::with_playback_source(track, source),
        )))
        .unwrap();
    measured(&mut engine);
    while events.pop().is_ok() {}
    (engine, commands, events, track)
}

fn measured(engine: &mut AudioEngine) {
    assert_eq!(
        crate::retirement::tests::allocations(|| {
            engine.process_block(AudioProcessBlock::new(&mut [], 2));
        }),
        (0, 0)
    );
}

fn assert_stopped_zero_frame_telemetry(events: &mut Consumer<EngineEvent>) {
    assert_eq!(
        events.pop().unwrap(),
        EngineEvent::TrackMeter(crate::events::TrackMeterEvent {
            track_id: TrackId::MASTER,
            peak_l: 0.0,
            peak_r: 0.0,
        })
    );
    assert!(matches!(
        events.pop().unwrap(),
        EngineEvent::PlaybackPosition(0)
    ));
    let EngineEvent::Metering(crate::events::MeteringEvent {
        peak_l,
        peak_r,
        rms_l,
        rms_r,
    }) = events.pop().unwrap()
    else {
        panic!("the stopped zero-frame callback must publish silent master meters");
    };
    assert_eq!((peak_l, peak_r, rms_l, rms_r), (0.0, 0.0, 0.0, 0.0));
    assert!(
        events.is_empty(),
        "no further owner or other event may remain"
    );
}

#[test]
fn an_eager_append_uses_actual_spare_lane_capacity_without_a_storage_request() {
    let old = lane(AutomationTarget::TrackPan, 0.0);
    let old_points = old.points.as_ptr();
    let incoming = lane(AutomationTarget::TrackGain, 0.25);
    let incoming_points = incoming.points.as_ptr();
    let expected = [old.clone(), incoming.clone()];
    let mut lanes = Vec::with_capacity(2);
    lanes.push(old);
    let outer = lanes.as_ptr();
    let capacity = lanes.capacity();
    let (mut engine, mut commands, mut events, track) = setup(lanes);
    let insertion = PreparedAutomationLaneInsertion::with_capacity(incoming, 0);
    let operation = insertion.id();
    let control = std::ptr::from_ref(insertion.0.as_ref());
    commands
        .push(EngineCommand::SetAutomationLane {
            track_id: track,
            insertion,
        })
        .unwrap();
    measured(&mut engine);
    assert_eq!(
        commands.slots(),
        vibez_core::constants::RING_BUFFER_CAPACITY
    );
    let current = &engine.tracks()[0].playback_source.automation;
    assert_eq!(current, &expected);
    assert_eq!(current.as_ptr(), outer);
    assert_eq!(current.capacity(), capacity);
    assert_eq!(current[0].points.as_ptr(), old_points);
    assert_eq!(current[1].points.as_ptr(), incoming_points);
    let EngineEvent::ClipInsertionCompleted(completed) = events.pop().unwrap() else {
        panic!("spare lane storage completes without a request");
    };
    assert_eq!(completed.id(), operation);
    assert_eq!(
        (completed.track_id, completed.target),
        (track, ClipInsertionTarget::AutomationLane(expected[1].id))
    );
    assert_eq!(completed.outcome, ClipInsertionOutcome::Applied);
    assert_eq!(std::ptr::from_ref(completed.owner.as_ref()), control);
    assert!(completed.owner.clip.is_none());
    assert!(completed.owner.reply.is_none());
    assert!(completed.owner.sender.is_some());
    let ClipInsertionStorage::AutomationLane(unused) = &completed.owner.storage else {
        panic!("complete control retains its concrete unused lane storage");
    };
    assert!(unused.is_empty());
    assert_eq!(unused.capacity(), 0);
    drop(completed);
    assert_stopped_zero_frame_telemetry(&mut events);
    assert_eq!(engine.transport().position(), 0);
    assert_eq!(engine.transport().audio_length(), None);
}

#[test]
fn full_capacity_replacement_returns_lane_before_control_and_preserves_full_ring_lifo() {
    for full in [false, true] {
        let old = lane(AutomationTarget::TrackGain, 0.5);
        let survivor = lane(AutomationTarget::TrackMute, 0.0);
        let old_points = old.points.as_ptr();
        let survivor_points = survivor.points.as_ptr();
        let expected_old = old.clone();
        let expected_survivor = survivor.clone();
        let mut incoming = lane(AutomationTarget::TrackPan, 0.25);
        incoming.id = old.id;
        let incoming_points = incoming.points.as_ptr();
        let expected_incoming = incoming.clone();
        let lanes = vec![old, survivor].into_boxed_slice().into_vec();
        assert_eq!((lanes.len(), lanes.capacity()), (2, 2));
        let outer = lanes.as_ptr();
        let (mut engine, mut commands, mut events, track) = setup(lanes);
        let insertion = PreparedAutomationLaneInsertion::with_capacity(incoming, 0);
        let operation = insertion.id();
        let control = std::ptr::from_ref(insertion.0.as_ref());
        let retained = engine.pending_retirements.as_ptr();
        let retained_capacity = engine.pending_retirements.capacity();
        let mut fillers = 0;
        if full {
            while engine
                .event_tx
                .push(EngineEvent::PlaybackPosition(123))
                .is_ok()
            {
                fillers += 1;
            }
            assert_eq!(fillers, vibez_core::constants::RING_BUFFER_CAPACITY);
        }
        commands
            .push(EngineCommand::SetAutomationLane {
                track_id: track,
                insertion,
            })
            .unwrap();
        measured(&mut engine);
        assert_eq!(
            commands.slots(),
            vibez_core::constants::RING_BUFFER_CAPACITY
        );
        let current = &engine.tracks()[0].playback_source.automation;
        assert_eq!(current, &[expected_incoming, expected_survivor]);
        assert_eq!(current.as_ptr(), outer);
        assert_eq!((current.len(), current.capacity()), (2, 2));
        assert_eq!(current[0].points.as_ptr(), incoming_points);
        assert_eq!(current[1].points.as_ptr(), survivor_points);
        assert_eq!(engine.pending_retirements.as_ptr(), retained);
        assert_eq!(engine.pending_retirements.capacity(), retained_capacity);
        assert_eq!(engine.pending_retirements.len(), usize::from(full) * 2);
        if full {
            assert!(
                matches!(&engine.pending_retirements[0], EngineEvent::RetiredAutomationLane(value) if value.points.as_ptr() == old_points)
            );
            assert!(
                matches!(&engine.pending_retirements[1], EngineEvent::ClipInsertionCompleted(value) if value.id() == operation)
            );
            for _ in 0..fillers {
                assert!(matches!(
                    events.pop().unwrap(),
                    EngineEvent::PlaybackPosition(123)
                ));
            }
            assert!(events.is_empty());
            measured(&mut engine);
        }
        let first = events.pop().unwrap();
        let second = events.pop().unwrap();
        let (returned, completed) = match (full, first, second) {
            (
                false,
                EngineEvent::RetiredAutomationLane(lane),
                EngineEvent::ClipInsertionCompleted(control),
            ) => (lane, control),
            (
                true,
                EngineEvent::ClipInsertionCompleted(control),
                EngineEvent::RetiredAutomationLane(lane),
            ) => (lane, control),
            (_, first, second) => {
                panic!("actual lane/control publication order changed: {first:?}, {second:?}")
            }
        };
        assert_eq!(returned, expected_old);
        assert_eq!(returned.points.as_ptr(), old_points);
        assert_eq!(completed.id(), operation);
        assert_eq!(
            completed.target,
            ClipInsertionTarget::AutomationLane(returned.id)
        );
        assert_eq!(completed.outcome, ClipInsertionOutcome::Applied);
        assert_eq!(std::ptr::from_ref(completed.owner.as_ref()), control);
        assert!(completed.owner.clip.is_none());
        assert!(completed.owner.reply.is_none());
        assert!(completed.owner.sender.is_some());
        drop(completed);
        assert_eq!(
            crate::retirement::tests::allocations(|| drop(returned)),
            (0, 1)
        );
        assert_stopped_zero_frame_telemetry(&mut events);
        assert!(engine.pending_retirements.is_empty());
        assert_eq!(engine.transport().position(), 0);
        assert_eq!(engine.transport().audio_length(), None);
    }
}

#[test]
fn a_missing_channel_returns_original_points_and_the_complete_unused_control() {
    let (mut engine, mut commands, mut events) = AudioEngine::new();
    let track = TrackId::new();
    let incoming = lane(AutomationTarget::TrackGain, 0.25);
    let expected = incoming.clone();
    let points = incoming.points.as_ptr();
    let insertion = PreparedAutomationLaneInsertion::with_capacity(incoming, 3);
    let operation = insertion.id();
    let identity = Arc::downgrade(&insertion.0.id.0);
    let control = std::ptr::from_ref(insertion.0.as_ref());
    let ring = std::ptr::from_ref(insertion.0.receiver.buffer());
    let ClipInsertionStorage::AutomationLane(storage) = &insertion.0.storage else {
        panic!("opaque lane preparation must retain its concrete outer Vec");
    };
    let eager = storage.as_ptr();
    let capacity = storage.capacity();
    commands
        .push(EngineCommand::SetAutomationLane {
            track_id: track,
            insertion,
        })
        .unwrap();
    measured(&mut engine);
    assert!(engine.tracks().is_empty());
    assert_eq!(
        commands.slots(),
        vibez_core::constants::RING_BUFFER_CAPACITY
    );
    let EngineEvent::RetiredAutomationLane(returned) = events.pop().unwrap() else {
        panic!("missing channel must return its lane at the original emission position");
    };
    assert_eq!(returned, expected);
    assert_eq!(returned.points.as_ptr(), points);
    let EngineEvent::ClipInsertionCompleted(completed) = events.pop().unwrap() else {
        panic!("missing lane target must also return its entire unused control");
    };
    assert_eq!(completed.id(), operation);
    assert_eq!(
        (completed.track_id, completed.target),
        (track, ClipInsertionTarget::AutomationLane(returned.id))
    );
    assert_eq!(completed.outcome, ClipInsertionOutcome::MissingChannel);
    assert_eq!(std::ptr::from_ref(completed.owner.as_ref()), control);
    assert_eq!(std::ptr::from_ref(completed.owner.receiver.buffer()), ring);
    assert!(completed.owner.sender.is_some());
    assert!(!completed.owner.receiver.is_abandoned());
    assert!(completed.owner.reply.is_none());
    assert!(completed.owner.clip.is_none());
    let ClipInsertionStorage::AutomationLane(storage) = &completed.owner.storage else {
        panic!("rejection must retain the original unused outer allocation");
    };
    assert!(storage.is_empty());
    assert_eq!(storage.as_ptr(), eager);
    assert_eq!(storage.capacity(), capacity);
    drop(operation);
    assert_eq!(identity.strong_count(), 1);
    drop(completed);
    assert_eq!(identity.strong_count(), 0);
    assert_eq!(
        crate::retirement::tests::allocations(|| drop(returned)),
        (0, 1)
    );
    assert_stopped_zero_frame_telemetry(&mut events);
    assert_eq!(engine.transport().position(), 0);
    assert_eq!(engine.transport().audio_length(), None);
}

#[path = "automation_lane_insertion_pressure_tests.rs"]
mod pressure_tests;
