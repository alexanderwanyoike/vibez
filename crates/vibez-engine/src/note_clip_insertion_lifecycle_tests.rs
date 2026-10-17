//! Note clip mailbox endpoints and nested storage owners finish on the non-audio owner thread.

use super::*;

pub(super) fn on_audio_thread(mut engine: AudioEngine, sounding: bool) -> (AudioEngine, [f32; 16]) {
    let owner_thread = std::thread::current().id();
    std::thread::spawn(move || {
        assert_ne!(std::thread::current().id(), owner_thread);
        let output = callback(&mut engine, sounding);
        (engine, output)
    })
    .join()
    .unwrap()
}

fn publish_request(
    mut fixture: Fixture,
    delta: &mut Delta,
    with_sentinel: bool,
) -> (Fixture, ClipInsertionStorageRequest) {
    fixture
        .commands
        .push(delta.command.take().unwrap())
        .unwrap();
    if with_sentinel {
        fixture.commands.push(EngineCommand::SetBpm(137.0)).unwrap();
    }
    // Moving only the engine avoids sending any raw identity-observer pointers to the worker.
    let (engine, _) = on_audio_thread(fixture.engine, false);
    fixture.engine = engine;
    let mut received = Received::default();
    received.drain(&mut fixture.events);
    assert_eq!(received.requests.len(), 1);
    assert!(received.completed.is_empty());
    let request = received.requests.pop().unwrap();
    assert_eq!(request.id, delta.id);
    assert_eq!(std::ptr::from_ref(request.sender.buffer()), delta.ring);
    (fixture, request)
}

#[test]
fn ready_and_unfulfilled_note_requests_complete_on_audio_and_release_control_on_owner_thread() {
    for outcome in [
        ClipInsertionOutcome::Applied,
        ClipInsertionOutcome::Abandoned,
    ] {
        let fixture = setup();
        let mut delta = delta(fixture.track);
        let identity = delta.identity.clone();
        let (mut fixture, request) = publish_request(fixture, &mut delta, true);
        if outcome == ClipInsertionOutcome::Applied {
            let prepared = crate::retirement::tests::allocations(|| request.fulfill());
            assert!(prepared.0 > 0);
            assert_eq!(prepared.1, 0);
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
        assert_complete(&fixture, &delta, &received.completed[0], outcome);
        assert!(received.completed[0].owner.receiver.is_empty());
        assert_eq!(fixture.engine.transport().bpm(), 137.0);
        let (engine, output) = on_audio_thread(fixture.engine, true);
        fixture.engine = engine;
        assert_signal(&output);
        let owner = received.completed.pop().unwrap();
        drop(received);
        let disposed = crate::retirement::tests::allocations(|| drop(owner));
        assert_eq!(disposed.0, 0);
        // Both eager Vecs, the final ring's two allocations and the control Box
        // return together. Applied also returns both emptied source allocations.
        let old_vectors = if outcome == ClipInsertionOutcome::Applied {
            2
        } else {
            0
        };
        assert_eq!(disposed.1, 2 + 2 + 1 + old_vectors);
        assert_eq!(identity.strong_count(), 1);
        assert_resident(&fixture);
        let disposed = crate::retirement::tests::allocations(|| drop(fixture.engine));
        assert_eq!(disposed.0, 0);
        assert!(disposed.1 > 0);
        // The populated resident notes and their constructor cache remain in
        // this uniquely owned source until this owner-thread engine destruction.
        assert_eq!(identity.strong_count(), 1);
        drop(delta);
        assert_eq!(identity.strong_count(), 0);
    }
}

#[test]
fn producer_first_note_shutdown_releases_the_still_queued_reply_on_owner_thread() {
    let fixture = setup();
    let mut delta = delta(fixture.track);
    let identity = delta.identity.clone();
    let (fixture, request) = publish_request(fixture, &mut delta, false);
    let prepared = crate::retirement::tests::allocations(|| request.fulfill());
    assert!(prepared.0 > 0);
    assert_eq!(prepared.1, 0);
    assert_eq!(identity.strong_count(), 2);
    assert_unchanged(&fixture);
    // No callback consumes this reply. The consumer's final ring Drop owns its occupied slot.
    let disposed = crate::retirement::tests::allocations(|| drop(fixture.engine));
    assert_eq!(disposed.0, 0);
    assert!(disposed.1 > 0);
    assert_eq!(identity.strong_count(), 1);
    drop(delta);
    assert_eq!(identity.strong_count(), 0);
}

#[test]
fn consumer_first_note_shutdown_releases_successful_orphaned_replies_on_owner_thread() {
    for inspect_push in [false, true] {
        let fixture = setup();
        let mut delta = delta(fixture.track);
        let identity = delta.identity.clone();
        let (fixture, mut request) = publish_request(fixture, &mut delta, false);
        assert!(!request.sender.is_abandoned());
        assert_eq!(request.sender.slots(), 1);
        assert_resident(&fixture);
        let disposed = crate::retirement::tests::allocations(|| drop(fixture.engine));
        assert_eq!(disposed.0, 0);
        assert!(disposed.1 > 0);
        assert_eq!(identity.strong_count(), 2);
        assert!(request.sender.is_abandoned());
        assert_eq!(std::ptr::from_ref(request.sender.buffer()), delta.ring);
        if inspect_push {
            // The public fulfill leg covers production. This endpoint leg proves
            // rtrb's orphaned push actually succeeds instead of relying on Err cleanup.
            let mut storage = None;
            let prepared = crate::retirement::tests::allocations(|| {
                storage = Some(ClipInsertionStorage::new(request.kind, request.capacity));
            });
            assert!(prepared.0 > 0);
            assert_eq!(prepared.1, 0);
            let mut pushed = None;
            assert_eq!(
                crate::retirement::tests::allocations(|| {
                    pushed = Some(request.sender.push(storage.take().unwrap()));
                }),
                (0, 0)
            );
            assert!(pushed.unwrap().is_ok());
            assert_eq!(request.sender.slots(), 0);
            let disposed = crate::retirement::tests::allocations(|| drop(request));
            assert_eq!(disposed.0, 0);
            // Pinned rtrb0.3.2 adds its slot Vec and Arc allocation to every
            // occupied reply allocation released by the final endpoint.
            assert_eq!(disposed.1, prepared.0 + 2);
        } else {
            let fulfilled = crate::retirement::tests::allocations(|| request.fulfill());
            assert!(fulfilled.0 > 0);
            assert_eq!(fulfilled.1, fulfilled.0 + 2);
        }
        assert_eq!(identity.strong_count(), 1);
        drop(delta);
        assert_eq!(identity.strong_count(), 0);
    }
}

#[test]
fn undrained_event_requests_and_unsent_note_controls_release_both_rings_after_owner_shutdown() {
    for admitted in [true, false] {
        let mut fixture = setup();
        let mut delta = delta(fixture.track);
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
            assert_unchanged(&fixture);
            assert_eq!(
                crate::retirement::tests::allocations(|| drop(fixture.events)),
                (0, 0)
            );
            let (engine, output) = on_audio_thread(fixture.engine, true);
            fixture.engine = engine;
            assert_signal(&output);
            let source = &fixture.engine.tracks()[0].playback_source;
            assert_eq!(source.note_clips.len(), 1);
            assert_eq!(source.note_clips.as_ptr(), fixture.live);
            assert_eq!(source.note_clips[0].notes.as_ptr(), fixture.notes);
            assert_eq!(source.removed_note_clips.as_ptr(), fixture.staging);
            assert_eq!(identity.strong_count(), 3);
            // The undrained request still lives in the ordinary event ring;
            // final engine destruction must release that ring and the mailbox.
            let disposed = crate::retirement::tests::allocations(|| drop(fixture.engine));
            assert_eq!(disposed.0, 0);
            assert!(disposed.1 > 0);
        } else {
            drop(fixture.events);
            let disposed = crate::retirement::tests::allocations(|| drop(fixture.engine));
            assert_eq!(disposed.0, 0);
            assert!(disposed.1 > 0);
            assert_eq!(identity.strong_count(), 2);
            let disposed = crate::retirement::tests::allocations(|| drop(delta.command.take()));
            assert_eq!(disposed, (0, 5));
        }
        assert_eq!(identity.strong_count(), 1);
        drop(delta);
        assert_eq!(identity.strong_count(), 0);
    }
}

#[test]
fn eager_and_missing_note_completions_return_both_unused_mailbox_endpoints() {
    for missing in [false, true] {
        let mut fixture = setup();
        let target = if missing {
            TrackId::new()
        } else {
            fixture.track
        };
        let mut delta = delta_with_capacity(target, 2);
        let identity = delta.identity.clone();
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
                target,
                crate::clip_insertion::ClipInsertionTarget::Clip(delta.clip)
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
            .expect("the producer never left its original owner");
        assert_eq!(std::ptr::from_ref(sender.buffer()), delta.ring);
        assert!(!sender.is_abandoned());
        assert!(!returned.owner.receiver.is_abandoned());
        assert!(returned.owner.receiver.is_empty());
        assert!(returned.owner.reply.is_none());
        let ClipInsertionStorage::NoteClip(storage) = &returned.owner.storage else {
            panic!("complete note storage")
        };
        assert!(storage.live.is_empty());
        assert!(storage.removed.is_empty());
        if missing {
            assert_eq!(returned.outcome, ClipInsertionOutcome::MissingTrack);
            assert_eq!(storage.live.as_ptr(), delta.eager);
            assert_eq!(storage.removed.as_ptr(), delta.eager_staging);
            let Some(ClipInsertionDelta::NoteClip(clip)) = &returned.owner.clip else {
                panic!("complete original note owner")
            };
            assert_eq!(clip.id, delta.clip);
            assert!(clip.notes.is_empty());
            assert_unchanged(&fixture);
        } else {
            assert_eq!(returned.outcome, ClipInsertionOutcome::Applied);
            assert_eq!(storage.live.as_ptr(), fixture.live);
            assert_eq!(storage.removed.as_ptr(), fixture.staging);
            assert!(returned.owner.clip.is_none());
            let source = &fixture.engine.tracks()[0].playback_source;
            assert_eq!(source.note_clips.as_ptr(), delta.eager);
            assert_eq!(source.removed_note_clips.as_ptr(), delta.eager_staging);
            assert_eq!(source.note_clips.len(), 2);
            assert_eq!(source.note_clips[1].id, delta.clip);
            assert!(source.note_clips[1].notes.is_empty());
            assert_resident(&fixture);
        }
        let (engine, output) = on_audio_thread(fixture.engine, true);
        fixture.engine = engine;
        assert_signal(&output);
        let owner = received.completed.pop().unwrap();
        drop(received);
        let disposed = crate::retirement::tests::allocations(|| drop(owner));
        assert_eq!(disposed, (0, 5));
        assert_eq!(identity.strong_count(), 1);
        let disposed = crate::retirement::tests::allocations(|| drop(fixture.engine));
        assert_eq!(disposed.0, 0);
        assert!(disposed.1 > 0);
        drop(delta);
        assert_eq!(identity.strong_count(), 0);
    }
}
