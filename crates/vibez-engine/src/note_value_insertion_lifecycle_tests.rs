//! Complete note-value data, cache allocations and mailbox endpoints leave the audio worker intact.

use crate::playback_source::note_value_insertion_tests::{
    INSERTION_CONTROL_ALLOCATIONS, NOTE_ARRAY_ALLOCATIONS,
};

use super::super::lifecycle_tests::on_audio_thread;
use super::*;

fn publish_request(
    mut fixture: Fixture,
    delta: &mut NoteDelta,
    with_sentinel: bool,
) -> (Fixture, ClipInsertionStorageRequest) {
    fixture
        .commands
        .push(delta.command.take().unwrap())
        .unwrap();
    if with_sentinel {
        fixture.commands.push(EngineCommand::SetBpm(137.0)).unwrap();
    }
    let (engine, _) = on_audio_thread(fixture.engine, false);
    fixture.engine = engine;
    let mut received = Received::default();
    received.drain(&mut fixture.events);
    assert_eq!(received.requests.len(), 1);
    assert!(received.completed.is_empty());
    let request = received.requests.pop().unwrap();
    assert_eq!(request.id, delta.id);
    assert_eq!(std::ptr::from_ref(request.sender.buffer()), delta.ring);
    delta.assert_waiting(&fixture);
    (fixture, request)
}

#[test]
fn ready_and_abandoned_note_values_return_complete_array_bundles_from_audio_worker() {
    for outcome in [
        ClipInsertionOutcome::Applied,
        ClipInsertionOutcome::Abandoned,
    ] {
        let fixture = setup();
        let mut delta = NoteDelta::prepare(&fixture);
        let identity = delta.identity.clone();
        let (mut fixture, request) = publish_request(fixture, &mut delta, true);
        if outcome == ClipInsertionOutcome::Applied {
            // Public fulfillment publishes the reply before dropping its producer.
            assert_eq!(
                crate::retirement::tests::allocations(|| request.fulfill()),
                (NOTE_ARRAY_ALLOCATIONS, 0)
            );
        } else {
            assert_eq!(
                crate::retirement::tests::allocations(|| drop(request)),
                (0, 0)
            );
        }
        assert_eq!(identity.strong_count(), 2);
        let (engine, _) = on_audio_thread(fixture.engine, false);
        fixture.engine = engine;
        let mut received = Received::default();
        received.drain(&mut fixture.events);
        assert_eq!(received.completed.len(), 1);
        assert!(received.requests.is_empty());
        delta.assert_complete(&fixture, &received.completed[0], outcome);
        assert!(received.completed[0].owner.receiver.is_empty());
        assert_eq!(fixture.engine.transport().bpm(), 137.0);
        let (engine, output) = on_audio_thread(fixture.engine, true);
        fixture.engine = engine;
        assert_signal(&output);
        assert_eq!(fixture.engine.transport().position(), 16);
        let owner = received.completed.pop().unwrap();
        drop(received);
        let old_arrays = if outcome == ClipInsertionOutcome::Applied {
            NOTE_ARRAY_ALLOCATIONS
        } else {
            0
        };
        // The held operation ID excludes its Arc from these final-owner frees.
        assert_eq!(
            crate::retirement::tests::allocations(|| drop(owner)),
            (
                0,
                NOTE_ARRAY_ALLOCATIONS + INSERTION_CONTROL_ALLOCATIONS + old_arrays
            )
        );
        assert_eq!(identity.strong_count(), 1);
        let disposed = crate::retirement::tests::allocations(|| drop(fixture.engine));
        assert_eq!(disposed.0, 0);
        assert!(disposed.1 > 0);
        drop(delta);
        assert_eq!(identity.strong_count(), 0);
    }
}

#[test]
fn producer_first_note_value_shutdown_drops_queued_three_array_reply_on_owner_thread() {
    let fixture = setup();
    let mut delta = NoteDelta::prepare(&fixture);
    let identity = delta.identity.clone();
    let (fixture, request) = publish_request(fixture, &mut delta, false);
    assert_eq!(
        crate::retirement::tests::allocations(|| request.fulfill()),
        (NOTE_ARRAY_ALLOCATIONS, 0)
    );
    delta.assert_waiting(&fixture);
    assert_eq!(identity.strong_count(), 2);
    // No callback pops the occupied mailbox; final consumer Drop owns that reply.
    let disposed = crate::retirement::tests::allocations(|| drop(fixture.engine));
    assert_eq!(disposed.0, 0);
    assert!(disposed.1 > 0);
    assert_eq!(identity.strong_count(), 1);
    drop(delta);
    assert_eq!(identity.strong_count(), 0);
}

#[test]
fn consumer_first_note_value_shutdown_releases_public_and_successfully_orphaned_replies() {
    for inspect_push in [false, true] {
        let fixture = setup();
        let mut delta = NoteDelta::prepare(&fixture);
        let identity = delta.identity.clone();
        let (fixture, mut request) = publish_request(fixture, &mut delta, false);
        assert!(!request.sender.is_abandoned());
        assert_eq!(request.sender.slots(), 1);
        let disposed = crate::retirement::tests::allocations(|| drop(fixture.engine));
        assert_eq!(disposed.0, 0);
        assert!(disposed.1 > 0);
        assert_eq!(identity.strong_count(), 2);
        assert!(request.sender.is_abandoned());
        assert_eq!(std::ptr::from_ref(request.sender.buffer()), delta.ring);
        if inspect_push {
            let mut storage = None;
            assert_eq!(
                crate::retirement::tests::allocations(|| {
                    storage = Some(ClipInsertionStorage::new(request.kind, request.capacity));
                }),
                (NOTE_ARRAY_ALLOCATIONS, 0)
            );
            let mut result = None;
            // rtrb0.3.2 can accept this reply after consumer destruction; prove
            // final producer Drop reclaims the occupied arrays, not just Err cleanup.
            assert_eq!(
                crate::retirement::tests::allocations(|| {
                    result = Some(request.sender.push(storage.take().unwrap()));
                }),
                (0, 0)
            );
            assert!(result.unwrap().is_ok());
            assert_eq!(request.sender.slots(), 0);
            assert_eq!(
                crate::retirement::tests::allocations(|| drop(request)),
                (0, NOTE_ARRAY_ALLOCATIONS + 2)
            );
        } else {
            assert_eq!(
                crate::retirement::tests::allocations(|| request.fulfill()),
                (NOTE_ARRAY_ALLOCATIONS, NOTE_ARRAY_ALLOCATIONS + 2)
            );
        }
        assert_eq!(identity.strong_count(), 1);
        drop(delta);
        assert_eq!(identity.strong_count(), 0);
    }
}

#[test]
fn ui_first_and_unsent_note_value_shutdown_release_event_and_reply_rings() {
    for admitted in [true, false] {
        let mut fixture = setup();
        let mut delta = NoteDelta::prepare(&fixture);
        let identity = delta.identity.clone();
        if admitted {
            fixture
                .commands
                .push(delta.command.take().unwrap())
                .unwrap();
            let (engine, _) = on_audio_thread(fixture.engine, false);
            fixture.engine = engine;
            assert!(fixture.events.slots() > 0);
            assert_eq!(identity.strong_count(), 3);
            delta.assert_waiting(&fixture);
            assert_eq!(
                crate::retirement::tests::allocations(|| drop(fixture.events)),
                (0, 0)
            );
            let (engine, output) = on_audio_thread(fixture.engine, true);
            fixture.engine = engine;
            assert_signal(&output);
            assert_eq!(fixture.engine.transport().position(), 16);
            delta
                .original
                .assert_unchanged(&fixture.engine.tracks()[0].playback_source.note_clips[0]);
            assert_eq!(identity.strong_count(), 3);
            let disposed = crate::retirement::tests::allocations(|| drop(fixture.engine));
            assert_eq!(disposed.0, 0);
            assert!(disposed.1 > 0);
        } else {
            drop(fixture.events);
            let disposed = crate::retirement::tests::allocations(|| drop(fixture.engine));
            assert_eq!(disposed.0, 0);
            assert!(disposed.1 > 0);
            assert_eq!(identity.strong_count(), 2);
            assert_eq!(
                crate::retirement::tests::allocations(|| drop(delta.command.take())),
                (0, NOTE_ARRAY_ALLOCATIONS + INSERTION_CONTROL_ALLOCATIONS)
            );
        }
        assert_eq!(identity.strong_count(), 1);
        drop(delta);
        assert_eq!(identity.strong_count(), 0);
    }
}

#[derive(Clone, Copy)]
enum EagerTarget {
    EmptyClip,
    ResidentClip,
    MissingTrack,
    MissingClip,
}

#[test]
fn adequate_eager_and_missing_note_values_return_unused_endpoints_and_zero_or_nonzero_old_arrays() {
    for target in [
        EagerTarget::EmptyClip,
        EagerTarget::ResidentClip,
        EagerTarget::MissingTrack,
        EagerTarget::MissingClip,
    ] {
        let empty = matches!(target, EagerTarget::EmptyClip);
        let mut fixture = if empty {
            setup_with_notes(Vec::new())
        } else {
            setup()
        };
        let mut delta = NoteDelta::with_capacity(&fixture, 2);
        let identity = delta.identity.clone();
        let track_id = if matches!(target, EagerTarget::MissingTrack) {
            TrackId::new()
        } else {
            fixture.track
        };
        let clip_id = if matches!(target, EagerTarget::MissingClip) {
            ClipId::new()
        } else {
            fixture.resident
        };
        let Some(EngineCommand::AddNote {
            track_id: command_track,
            clip_id: command_clip,
            ..
        }) = delta.command.as_mut()
        else {
            unreachable!()
        };
        *command_track = track_id;
        *command_clip = clip_id;
        fixture
            .commands
            .push(delta.command.take().unwrap())
            .unwrap();
        let (engine, _) = on_audio_thread(fixture.engine, false);
        fixture.engine = engine;
        let mut received = Received::default();
        received.drain(&mut fixture.events);
        assert!(received.requests.is_empty());
        assert_eq!(received.completed.len(), 1);
        let returned = &received.completed[0];
        assert_eq!(returned.id(), delta.id);
        assert_eq!(
            (returned.track_id, returned.target),
            (
                track_id,
                crate::clip_insertion::ClipInsertionTarget::Clip(clip_id)
            )
        );
        assert_eq!(std::ptr::from_ref(returned.owner.as_ref()), delta.control);
        assert_eq!(
            std::ptr::from_ref(returned.owner.receiver.buffer()),
            delta.ring
        );
        let sender = returned
            .owner
            .sender
            .as_ref()
            .expect("unused producer stays inside its complete owner");
        assert_eq!(std::ptr::from_ref(sender.buffer()), delta.ring);
        assert!(!sender.is_abandoned());
        assert!(!returned.owner.receiver.is_abandoned());
        assert!(returned.owner.receiver.is_empty());
        assert!(returned.owner.reply.is_none());
        let ClipInsertionStorage::NoteValue(storage) = &returned.owner.storage else {
            panic!("all eager or displaced note arrays belong to this complete owner");
        };
        let missing = matches!(target, EagerTarget::MissingTrack | EagerTarget::MissingClip);
        if missing {
            let expected = if matches!(target, EagerTarget::MissingTrack) {
                ClipInsertionOutcome::MissingTrack
            } else {
                ClipInsertionOutcome::MissingClip
            };
            assert_eq!(returned.outcome, expected);
            delta.eager.assert_unchanged(storage);
            let Some(ClipInsertionDelta::NoteValue(original)) = &returned.owner.clip else {
                panic!("complete rejected note value")
            };
            assert_eq!(*original, delta.note);
            delta.assert_waiting(&fixture);
        } else {
            assert_eq!(returned.outcome, ClipInsertionOutcome::Applied);
            delta.original.assert_retired_storage(storage);
            assert!(returned.owner.clip.is_none());
            let source = &fixture.engine.tracks()[0].playback_source;
            assert_eq!(source.note_clips.len(), 1);
            assert_eq!(source.note_clips.as_ptr(), fixture.live);
            assert_eq!(source.removed_note_clips.as_ptr(), fixture.staging);
            assert!(source.removed_note_clips.is_empty());
            delta
                .original
                .assert_appended(&source.note_clips[0], delta.note);
            delta.eager.assert_installed_in(&source.note_clips[0]);
        }
        assert_eq!(fixture.engine.transport().audio_length(), Some(8 * 22_050));
        let (engine, output) = on_audio_thread(fixture.engine, true);
        fixture.engine = engine;
        if empty {
            assert_eq!(output, [0.0; 16]);
        } else {
            assert_signal(&output);
        }
        assert_eq!(fixture.engine.transport().position(), 16);
        let owner = received.completed.pop().unwrap();
        drop(received);
        assert_eq!(
            crate::retirement::tests::allocations(|| drop(owner)),
            (
                0,
                INSERTION_CONTROL_ALLOCATIONS + if empty { 0 } else { NOTE_ARRAY_ALLOCATIONS }
            )
        );
        assert_eq!(identity.strong_count(), 1);
        let disposed = crate::retirement::tests::allocations(|| drop(fixture.engine));
        assert_eq!(disposed.0, 0);
        assert!(disposed.1 > 0);
        drop(delta);
        assert_eq!(identity.strong_count(), 0);
    }
}
