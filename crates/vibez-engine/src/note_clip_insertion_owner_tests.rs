//! Rejected prepared note clips return their complete unused control and storage owners.

use super::*;
use crate::commands::EngineCommand;
use crate::engine::{AudioEngine, AudioProcessBlock};
use crate::events::EngineEvent;

#[test]
fn public_missing_track_returns_complete_prepared_note_clip_without_callback_heap_work() {
    let (mut engine, mut commands, mut events) = AudioEngine::new();
    let track = TrackId::new();
    let clip = ClipId::new();
    let insertion =
        PreparedNoteClipInsertion::new(clip, 2.25, 5.5, 0.75, true, 0.25, 2.5, GrooveGrid::Eighth);
    let identity = Arc::downgrade(&insertion.0.id.0);
    let control = std::ptr::from_ref(insertion.0.as_ref());
    let ring = std::ptr::from_ref(insertion.0.receiver.buffer());
    let ClipInsertionStorage::Note(storage) = &insertion.0.storage else {
        unreachable!();
    };
    let live = storage.live.as_ptr();
    let removed = storage.removed.as_ptr();
    commands
        .push(EngineCommand::AddNoteClip {
            track_id: track,
            insertion,
        })
        .unwrap();
    assert_eq!(identity.strong_count(), 1);
    assert_eq!(
        crate::retirement::tests::allocations(|| {
            engine.process_block(AudioProcessBlock::new(&mut [], 2));
        }),
        (0, 0)
    );
    assert!(engine.tracks().is_empty());
    assert!(!engine.transport().is_playing());
    assert_eq!(engine.transport().position(), 0);
    assert_eq!(engine.transport().audio_length(), None);
    assert_eq!(identity.strong_count(), 1);
    let EngineEvent::ClipInsertionCompleted(completed) = events.pop().unwrap() else {
        panic!("missing target must return its complete prepared owner");
    };
    assert_eq!(completed.track_id, track);
    assert_eq!(completed.clip_id, clip);
    assert_eq!(completed.outcome, ClipInsertionOutcome::MissingTrack);
    assert_eq!(Arc::as_ptr(&completed.owner.id.0), identity.as_ptr());
    assert_eq!(std::ptr::from_ref(completed.owner.as_ref()), control);
    assert_eq!(std::ptr::from_ref(completed.owner.receiver.buffer()), ring);
    assert!(completed.owner.sender.is_some());
    assert!(!completed.owner.receiver.is_abandoned());
    assert!(completed.owner.reply.is_none());
    let ClipInsertionStorage::Note(storage) = &completed.owner.storage else {
        panic!("rejection must retain note storage");
    };
    assert!(storage.live.is_empty());
    assert!(storage.removed.is_empty());
    assert_eq!(storage.live.as_ptr(), live);
    assert_eq!(storage.removed.as_ptr(), removed);
    assert!(storage.live.capacity() >= 1);
    assert!(storage.removed.capacity() >= 1);
    let Some(ClipInsertionDelta::Note(rejected)) = &completed.owner.clip else {
        panic!("rejection must retain the original empty note clip");
    };
    assert_eq!(rejected.id, clip);
    assert_eq!(rejected.position_beats, 2.25);
    assert_eq!(rejected.duration_beats, 5.5);
    assert_eq!(rejected.start_marker_beats, 0.75);
    assert!(rejected.loop_enabled);
    assert_eq!(rejected.loop_start_beats, 0.25);
    assert_eq!(rejected.loop_end_beats, 2.5);
    assert_eq!(rejected.groove_grid, GrooveGrid::Eighth);
    assert!(rejected.notes.is_empty());
    drop(completed);
    assert_eq!(identity.strong_count(), 0);
    while let Ok(event) = events.pop() {
        assert!(!matches!(
            event,
            EngineEvent::ClipInsertionCompleted(_) | EngineEvent::ClipInsertionStorageRequested(_)
        ));
    }
}

#[path = "note_clip_insertion_pressure_tests.rs"]
mod pressure_tests;
