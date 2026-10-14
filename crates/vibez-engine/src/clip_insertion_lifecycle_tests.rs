//! Pending AddClip owners survive native abandonment and both endpoint shutdown orders.

use super::*;

fn mailbox(delta: &Delta) -> *const rtrb::RingBuffer<ClipInsertionStorage> {
    let EngineCommand::AddClip { insertion, .. } = delta.command.as_ref().unwrap() else {
        unreachable!();
    };
    let pointer = std::ptr::from_ref(insertion.receiver.buffer());
    assert_eq!(std::ptr::from_ref(insertion.sender.as_ref().unwrap().buffer()), pointer);
    pointer
}

fn assert_abandoned(
    fixture: &Fixture,
    delta: &Delta,
    returned: &CompletedClipInsertion,
    ring: *const rtrb::RingBuffer<ClipInsertionStorage>,
) {
    assert_eq!(returned.id(), delta.id);
    assert_eq!((returned.track_id, returned.clip_id), (fixture.track, delta.clip));
    assert_eq!(returned.outcome, ClipInsertionOutcome::Abandoned);
    assert_eq!(std::ptr::from_ref(returned.owner.as_ref()), delta.control);
    assert_eq!(std::ptr::from_ref(returned.owner.receiver.buffer()), ring);
    assert!(returned.owner.receiver.is_abandoned());
    assert!(returned.owner.receiver.is_empty());
    assert!(returned.owner.sender.is_none());
    assert!(returned.owner.reply.is_none());
    assert_eq!(returned.owner.storage.clips.as_ptr(), delta.eager);
    assert_eq!(returned.owner.storage.removed_audio_clips.as_ptr(), delta.eager_staging);
    let clip = returned.owner.clip.as_ref().expect("abandonment returns the original delta");
    assert_eq!(clip.id, delta.clip);
    assert_eq!(Arc::as_ptr(&clip.audio), delta.audio.as_ptr());
    assert_eq!((clip.position, clip.source_offset, clip.start_marker, clip.duration), (0, 0, 0, 4096));
    assert!(clip.loop_enabled);
    assert_eq!((clip.loop_start, clip.loop_end), (0, 128));
    assert_eq!(clip.linear_gain, 1.0);
    assert_eq!(clip.fades, Default::default());
    assert_eq!(clip.playback_direction, Default::default());
    assert_eq!(clip.warp_markers.as_slice().as_ptr(), delta.markers);
    assert_eq!(clip.warp_markers, delta.expected_markers);
    assert_eq!(delta.audio.strong_count(), 1);
    let source = &fixture.engine.tracks()[0].playback_source;
    assert_eq!(source.clips.len(), 1);
    assert_eq!(source.clips.as_ptr(), fixture.live_allocation);
    assert_eq!(source.removed_audio_clips.as_ptr(), fixture.staging_allocation);
    assert_eq!(Arc::as_ptr(&source.clips[0].audio), fixture.resident.as_ptr());
    assert_eq!(fixture.resident.strong_count(), 1);
}

#[test]
fn abandoned_native_request_returns_complete_delta_and_resumes_fifo_after_credit() {
    for pressure in [false, true] {
        let mut fixture = setup();
        let mut delta = delta(fixture.track);
        let ring = mailbox(&delta);
        let identity = Arc::downgrade(&delta.id.0);
        fixture.commands.push(delta.command.take().unwrap()).unwrap();
        fixture.commands.push(EngineCommand::SetBpm(137.0)).unwrap();
        let slots = fixture.commands.slots();
        callback(&mut fixture.engine, false);
        let mut received = Received::default();
        received.drain(&mut fixture.events);
        assert_eq!(received.requests.len(), 1);
        let request = received.requests.pop().unwrap();
        assert_eq!(request.id, delta.id);
        assert_eq!(std::ptr::from_ref(request.sender.buffer()), ring);
        assert_signal(&callback(&mut fixture.engine, true), 0.25);
        assert_eq!(fixture.commands.slots(), slots + 1);
        assert_eq!(fixture.engine.transport().position(), 16);
        assert_eq!(fixture.engine.transport().bpm(), 120.0);
        received.drain(&mut fixture.events);
        let expected = if pressure {
            full_ui_ring(&mut fixture.engine);
            inject_full_retirement_store(&mut fixture.engine)
        } else { Vec::new() };
        assert_eq!(crate::retirement::tests::allocations(|| drop(request)), (0, 0));
        if pressure {
            assert_signal(&callback(&mut fixture.engine, true), 0.25);
            assert_eq!(fixture.engine.transport().position(), 24);
            assert_eq!(fixture.engine.pending_retirements.len(), 32);
            assert_eq!(fixture.engine.tracks()[0].playback_source.clips.len(), 1);
            assert_eq!(fixture.commands.slots(), slots + 1);
            assert_eq!(delta.audio.strong_count(), 1);
            assert!(matches!(fixture.events.pop().unwrap(), EngineEvent::PlaybackPosition(u64::MAX)));
        }
        callback(&mut fixture.engine, false);
        if pressure {
            let completion = fixture.engine.pending_retirements.iter().find_map(|event| {
                if let EngineEvent::ClipInsertionCompleted(owner) = event { Some(owner) } else { None }
            }).expect("one freed credit retains the complete abandoned owner");
            assert_abandoned(&fixture, &delta, completion, ring);
            assert_eq!(fixture.engine.pending_retirements.len(), 32);
            assert_eq!(fixture.engine.transport().bpm(), 120.0);
            assert_eq!(fixture.commands.slots(), slots + 1);
            received.drain(&mut fixture.events);
            assert!(received.completed.is_empty());
            callback(&mut fixture.engine, false);
        }
        received.drain(&mut fixture.events);
        received.assert_lanes(&expected);
        assert_eq!(received.completed.len(), 1);
        assert_abandoned(&fixture, &delta, &received.completed[0], ring);
        assert_eq!(fixture.engine.transport().bpm(), 137.0);
        assert_eq!(fixture.commands.slots(), slots + 2);
        assert!(fixture.engine.pending_retirements.is_empty());
        assert_eq!(fixture.engine.transport().audio_length(), Some(4096));
        assert_signal(&callback(&mut fixture.engine, true), 0.25);
        let disposal = crate::retirement::tests::allocations(|| drop(received));
        assert_eq!(disposal.0, 0);
        assert!(disposal.1 > 0);
        assert_eq!(delta.audio.strong_count(), 0);
        assert_eq!(identity.strong_count(), 1);
        let mut later = Received::default();
        later.drain(&mut fixture.events);
        callback(&mut fixture.engine, false);
        later.drain(&mut fixture.events);
        assert!(later.requests.is_empty());
        assert!(later.completed.is_empty());
        drop(delta);
        assert_eq!(identity.strong_count(), 0);
        drop(fixture.engine);
        assert_eq!(fixture.resident.strong_count(), 0);
    }
}

fn on_audio_thread(mut engine: AudioEngine, sounding: bool) -> (AudioEngine, [f32; 16]) {
    let owner_thread = std::thread::current().id();
    std::thread::spawn(move || {
        assert_ne!(std::thread::current().id(), owner_thread);
        let output = callback(&mut engine, sounding);
        (engine, output)
    }).join().unwrap()
}

#[test]
fn fulfilled_reply_survives_producer_drop_until_completion_or_owner_shutdown() {
    for complete in [true, false] {
        let mut fixture = setup();
        let mut delta = delta(fixture.track);
        let ring = mailbox(&delta);
        let identity = Arc::downgrade(&delta.id.0);
        fixture.commands.push(delta.command.take().unwrap()).unwrap();
        let (engine, _) = on_audio_thread(fixture.engine, false);
        fixture.engine = engine;
        let mut received = Received::default();
        received.drain(&mut fixture.events);
        assert_eq!(received.requests.len(), 1);
        let request = received.requests.pop().unwrap();
        assert_eq!(std::ptr::from_ref(request.sender.buffer()), ring);
        let fulfilled = crate::retirement::tests::allocations(|| request.fulfill());
        assert!(fulfilled.0 > 0);
        assert_eq!(fulfilled.1, 0);
        assert_eq!(identity.strong_count(), 2);
        assert_eq!(delta.audio.strong_count(), 1);
        if complete {
            let (engine, _) = on_audio_thread(fixture.engine, false);
            fixture.engine = engine;
            received.drain(&mut fixture.events);
            assert_eq!(received.completed.len(), 1);
            assert_applied(&fixture, &delta, &received.completed[0]);
            assert_eq!(std::ptr::from_ref(received.completed[0].owner.receiver.buffer()), ring);
            assert!(received.completed[0].owner.receiver.is_empty());
            let (engine, output) = on_audio_thread(fixture.engine, true);
            fixture.engine = engine;
            assert_signal(&output, 0.3125);
            let disposed = crate::retirement::tests::allocations(|| drop(received));
            assert_eq!(disposed.0, 0);
            assert!(disposed.1 > 0);
            assert_eq!(identity.strong_count(), 1);
            assert_eq!(delta.audio.strong_count(), 1);
        } else {
            // Fulfill completed before teardown, but no callback consumed its
            // queued reply. The final consumer owns its complete ring contents.
            assert_eq!(fixture.engine.tracks()[0].playback_source.clips.len(), 1);
            drop(received);
        }
        let disposed = crate::retirement::tests::allocations(|| drop(fixture.engine));
        assert_eq!(disposed.0, 0);
        assert!(disposed.1 > 0);
        assert_eq!(delta.audio.strong_count(), 0);
        assert_eq!(fixture.resident.strong_count(), 0);
        assert_eq!(identity.strong_count(), 1);
        drop(delta);
        assert_eq!(identity.strong_count(), 0);
    }
}

#[test]
fn consumer_first_shutdown_releases_successful_orphaned_replies_on_the_owner_thread() {
    for inspect_push in [false, true] {
        let mut fixture = setup();
        let mut delta = delta(fixture.track);
        let ring = mailbox(&delta);
        let identity = Arc::downgrade(&delta.id.0);
        fixture.commands.push(delta.command.take().unwrap()).unwrap();
        let (engine, _) = on_audio_thread(fixture.engine, false);
        fixture.engine = engine;
        let mut received = Received::default();
        received.drain(&mut fixture.events);
        assert_eq!(received.requests.len(), 1);
        let mut request = received.requests.pop().unwrap();
        assert_eq!(std::ptr::from_ref(request.sender.buffer()), ring);
        assert!(!request.sender.is_abandoned());
        assert_eq!(request.sender.slots(), 1);
        let engine_disposal = crate::retirement::tests::allocations(|| drop(fixture.engine));
        assert_eq!(engine_disposal.0, 0);
        assert!(engine_disposal.1 > 0);
        assert_eq!(delta.audio.strong_count(), 0);
        assert_eq!(fixture.resident.strong_count(), 0);
        assert_eq!(identity.strong_count(), 2);
        assert!(request.sender.is_abandoned());
        assert_eq!(std::ptr::from_ref(request.sender.buffer()), ring);
        if inspect_push {
            // The public fulfill leg below covers the production method. This
            // separate endpoint leg observes its successful orphaned push,
            // which cannot be justified with PushError-only disposal reasoning.
            let mut storage = None;
            let preparation = crate::retirement::tests::allocations(|| {
                storage = Some(ClipInsertionStorage::new(request.capacity));
            });
            assert!(preparation.0 > 0);
            assert_eq!(preparation.1, 0);
            let mut pushed = None;
            assert_eq!(crate::retirement::tests::allocations(|| {
                pushed = Some(request.sender.push(storage.take().unwrap()));
            }), (0, 0));
            assert!(pushed.unwrap().is_ok());
            assert_eq!(request.sender.slots(), 0);
            assert_eq!(std::ptr::from_ref(request.sender.buffer()), ring);
            let disposal = crate::retirement::tests::allocations(|| drop(request));
            assert_eq!(disposal.0, 0);
            // Pinned rtrb0.3.2 owns a slot allocation plus its Arc allocation;
            // the occupied reply must release every measured Vec allocation too.
            assert_eq!(disposal.1, preparation.0 + 2);
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
fn undrained_requests_and_unsent_controls_release_owners_after_ui_shutdown() {
    for admitted in [true, false] {
        let mut fixture = setup();
        let mut delta = delta(fixture.track);
        let identity = Arc::downgrade(&delta.id.0);
        if admitted {
            fixture.commands.push(delta.command.take().unwrap()).unwrap();
            let (engine, _) = on_audio_thread(fixture.engine, false);
            fixture.engine = engine;
            assert!(fixture.events.slots() > 0);
            assert_eq!(identity.strong_count(), 3);
            assert_eq!(crate::retirement::tests::allocations(|| drop(fixture.events)), (0, 0));
            let (engine, output) = on_audio_thread(fixture.engine, true);
            fixture.engine = engine;
            assert_signal(&output, 0.25);
            assert_eq!(fixture.engine.tracks()[0].playback_source.clips.len(), 1);
            assert_eq!(fixture.engine.tracks()[0].playback_source.clips.as_ptr(), fixture.live_allocation);
            assert_eq!(delta.audio.strong_count(), 1);
            // The request still lives inside the orphaned ordinary event ring.
            // Owner-thread engine teardown must release both connected rings.
            assert_eq!(identity.strong_count(), 3);
            let disposal = crate::retirement::tests::allocations(|| drop(fixture.engine));
            assert_eq!(disposal.0, 0);
            assert!(disposal.1 > 0);
        } else {
            drop(fixture.events);
            drop(fixture.engine);
            assert_eq!(identity.strong_count(), 2);
            assert_eq!(delta.audio.strong_count(), 1);
            let disposal = crate::retirement::tests::allocations(|| drop(delta.command.take()));
            assert_eq!(disposal.0, 0);
            assert!(disposal.1 > 0);
        }
        assert_eq!(delta.audio.strong_count(), 0);
        assert_eq!(fixture.resident.strong_count(), 0);
        assert_eq!(identity.strong_count(), 1);
        drop(delta);
        assert_eq!(identity.strong_count(), 0);
    }
}

#[test]
fn immediate_and_missing_track_completions_return_both_unused_mailbox_endpoints() {
    for missing in [false, true] {
        let mut fixture = setup();
        let target = TrackId::new();
        if !missing {
            let mut track = EngineTrack::new(target);
            track.pan = 0.0;
            fixture.commands.push(EngineCommand::AddTrack(Box::new(track))).unwrap();
            let model: Vec<_> = [fixture.track, target, TrackId::MASTER].into_iter().map(|id| RoutingChannel {
                id, is_bus: id.is_master(), sends: Vec::new(), effects: Vec::new(),
            }).collect();
            fixture.commands.push(EngineCommand::SetRouting(
                crate::routing::PreparedRouting::prepare(&model, 8).unwrap(),
            )).unwrap();
            callback(&mut fixture.engine, false);
            while fixture.events.pop().is_ok() {}
        }
        let mut delta = delta(target);
        let ring = mailbox(&delta);
        let identity = Arc::downgrade(&delta.id.0);
        fixture.commands.push(delta.command.take().unwrap()).unwrap();
        let (engine, _) = on_audio_thread(fixture.engine, false);
        fixture.engine = engine;
        let mut received = Received::default();
        received.drain(&mut fixture.events);
        assert!(received.requests.is_empty());
        assert_eq!(received.completed.len(), 1);
        let returned = &received.completed[0];
        assert_eq!(returned.id(), delta.id);
        assert_eq!((returned.track_id, returned.clip_id), (target, delta.clip));
        assert_eq!(std::ptr::from_ref(returned.owner.as_ref()), delta.control);
        assert_eq!(std::ptr::from_ref(returned.owner.receiver.buffer()), ring);
        let sender = returned.owner.sender.as_ref().expect("the request producer was never sent");
        assert_eq!(std::ptr::from_ref(sender.buffer()), ring);
        assert!(!sender.is_abandoned());
        assert!(!returned.owner.receiver.is_abandoned());
        assert!(returned.owner.receiver.is_empty());
        assert!(returned.owner.reply.is_none());
        if missing {
            assert_eq!(returned.outcome, ClipInsertionOutcome::MissingTrack);
            assert_eq!(returned.owner.storage.clips.as_ptr(), delta.eager);
            assert_eq!(returned.owner.storage.removed_audio_clips.as_ptr(), delta.eager_staging);
            let clip = returned.owner.clip.as_ref().expect("the missing target returns its original owner");
            assert_eq!(Arc::as_ptr(&clip.audio), delta.audio.as_ptr());
            assert_eq!(clip.warp_markers.as_slice().as_ptr(), delta.markers);
            assert_eq!(clip.warp_markers, delta.expected_markers);
        } else {
            assert_eq!(returned.outcome, ClipInsertionOutcome::Applied);
            assert!(returned.owner.clip.is_none());
            assert_eq!(returned.owner.storage.clips.capacity(), 0);
            assert_eq!(returned.owner.storage.removed_audio_clips.capacity(), 0);
            let source = &fixture.engine.tracks()[1].playback_source;
            assert_eq!(source.clips.as_ptr(), delta.eager);
            assert_eq!(source.removed_audio_clips.as_ptr(), delta.eager_staging);
            assert_eq!(Arc::as_ptr(&source.clips[0].audio), delta.audio.as_ptr());
            assert_eq!(source.clips[0].warp_markers.as_slice().as_ptr(), delta.markers);
        }
        assert_eq!(delta.audio.strong_count(), 1);
        let disposal = crate::retirement::tests::allocations(|| drop(received));
        assert_eq!(disposal.0, 0);
        assert!(disposal.1 > 0);
        assert_eq!(delta.audio.strong_count(), usize::from(!missing));
        assert_eq!(identity.strong_count(), 1);
        let (engine, output) = on_audio_thread(fixture.engine, true);
        fixture.engine = engine;
        assert_signal(&output, if missing { 0.25 } else { 0.3125 });
        drop(fixture.engine);
        assert_eq!(delta.audio.strong_count(), 0);
        assert_eq!(fixture.resident.strong_count(), 0);
        drop(delta);
        assert_eq!(identity.strong_count(), 0);
    }
}
