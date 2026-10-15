//! Native lane growth uses actual constructor and offline outer capacity without replacing point owners.

use super::*;
use crate::clip_insertion::{
    ClipInsertionOutcome, ClipInsertionTarget, PreparedAutomationLaneInsertion,
};
use crate::commands::EngineCommand;
use crate::engine::{AudioEngine, AudioProcessBlock};
use crate::events::EngineEvent;
use crate::mixer::EngineTrack;
use vibez_core::automation::{AutomationPoint, AutomationTarget};

pub(crate) fn lane(value: f32) -> AutomationLane {
    let mut lane = AutomationLane::new(AutomationTarget::TrackPan);
    lane.points = vec![
        AutomationPoint {
            beat: 0.0,
            value,
            curve: -0.5,
        },
        AutomationPoint {
            beat: 4.0,
            value,
            curve: 0.75,
        },
    ];
    lane
}

fn callback(engine: &mut AudioEngine) {
    assert_eq!(
        crate::retirement::tests::allocations(|| {
            engine.process_block(AudioProcessBlock::new(&mut [], 2));
        }),
        (0, 0)
    );
}

fn telemetry(events: &mut rtrb::Consumer<EngineEvent>) {
    assert!(matches!(
        events.pop().unwrap(),
        EngineEvent::PlaybackPosition(0)
    ));
    let EngineEvent::Metering {
        peak_l,
        peak_r,
        rms_l,
        rms_r,
    } = events.pop().unwrap()
    else {
        panic!("zero-frame constructor work must publish silent meters")
    };
    assert_eq!((peak_l, peak_r, rms_l, rms_r), (0.0, 0.0, 0.0, 0.0));
    assert!(events.is_empty());
}

pub(crate) fn verify_lane_source_growth(source: PreparedPlaybackSource) {
    assert!(!source.automation.is_empty());
    assert_eq!(source.automation.len(), source.automation.capacity());
    let mut expected = source.automation.clone();
    let mut points: Vec<_> = source
        .automation
        .iter()
        .map(|lane| lane.points.as_ptr())
        .collect();
    let clips = source.clips.as_ptr();
    let notes = source.note_clips.as_ptr();
    let removed_audio = source.removed_audio_clips.as_ptr();
    let removed_notes = source.removed_note_clips.as_ptr();
    assert!(source.clips.is_empty() && source.note_clips.is_empty());
    let track = TrackId::new();
    let (mut engine, mut commands, mut events) = AudioEngine::new();
    commands
        .push(EngineCommand::AddTrack(Box::new(
            EngineTrack::with_playback_source(track, source),
        )))
        .unwrap();
    callback(&mut engine);
    while events.pop().is_ok() {}
    let source_owner = std::ptr::from_ref(engine.tracks()[0].playback_source.as_ref());
    let mut boundaries = Vec::new();
    let mut controls = Vec::new();
    for index in 0..8 {
        let incoming = lane((index + 1) as f32 / 16.0);
        expected.push(incoming.clone());
        points.push(incoming.points.as_ptr());
        let insertion = PreparedAutomationLaneInsertion::new(incoming);
        let operation = insertion.id();
        let current = &engine.tracks()[0].playback_source.automation;
        let (outer, length, capacity) = (current.as_ptr(), current.len(), current.capacity());
        commands
            .push(EngineCommand::SetAutomationLane {
                track_id: track,
                insertion,
            })
            .unwrap();
        callback(&mut engine);
        if length == capacity {
            let EngineEvent::ClipInsertionStorageRequested(request) = events.pop().unwrap() else {
                panic!("the same default hint must use actual current capacity")
            };
            assert_eq!(request.id, operation);
            assert_eq!(
                (request.track_id, request.target),
                (
                    track,
                    ClipInsertionTarget::AutomationLane(expected.last().unwrap().id)
                )
            );
            assert_eq!(
                engine.tracks()[0].playback_source.automation,
                expected[..expected.len() - 1]
            );
            assert_eq!(
                engine.tracks()[0].playback_source.automation.as_ptr(),
                outer
            );
            telemetry(&mut events);
            boundaries.push(length);
            request.fulfill();
            callback(&mut engine);
        }
        let EngineEvent::ClipInsertionCompleted(control) = events.pop().unwrap() else {
            panic!("each native lane command must return exactly one complete control")
        };
        assert_eq!(control.id(), operation);
        assert_eq!(control.outcome, ClipInsertionOutcome::Applied);
        assert_eq!(
            (control.track_id, control.target),
            (
                track,
                ClipInsertionTarget::AutomationLane(expected.last().unwrap().id)
            )
        );
        controls.push(control);
        telemetry(&mut events);
        let source = &engine.tracks()[0].playback_source;
        assert_eq!(std::ptr::from_ref(source.as_ref()), source_owner);
        assert_eq!(
            (
                source.clips.as_ptr(),
                source.note_clips.as_ptr(),
                source.removed_audio_clips.as_ptr(),
                source.removed_note_clips.as_ptr()
            ),
            (clips, notes, removed_audio, removed_notes)
        );
        assert!(source.removed_audio_clips.is_empty() && source.removed_note_clips.is_empty());
        assert_eq!(source.automation, expected);
        for (lane, pointer) in source.automation.iter().zip(&points) {
            assert_eq!(lane.points.as_ptr(), *pointer);
        }
        assert_eq!(engine.transport().position(), 0);
        assert_eq!(engine.output_position_samples(), 0);
        assert_eq!(engine.transport().audio_length(), None);
        assert_eq!(
            commands.slots(),
            vibez_core::constants::RING_BUFFER_CAPACITY
        );
    }
    assert!(boundaries.len() >= 2);
    assert!(boundaries.windows(2).all(|pair| pair[1] > pair[0]));
    assert_eq!(controls.len(), 8);
    eprintln!("native default1 lane growth crossed actual full capacities {boundaries:?}");
}

#[test]
#[allow(clippy::field_reassign_with_default, reason = "Exercises the public Default constructor independently of struct literal construction")]
fn public_new_default_and_struct_lane_sources_cross_repeated_actual_capacity_boundaries() {
    for constructor in 0..3 {
        let lanes = vec![lane(0.0), lane(0.75)].into_boxed_slice().into_vec();
        let source = match constructor {
            0 => PreparedPlaybackSource::new(Vec::new(), Vec::new(), lanes),
            1 => {
                let mut source = PreparedPlaybackSource::default();
                source.automation = lanes;
                source
            }
            _ => PreparedPlaybackSource {
                clips: vec![],
                note_clips: vec![],
                automation: lanes,
                removed_audio_clips: vec![],
                removed_note_clips: vec![],
            },
        };
        verify_lane_source_growth(source);
    }
}
