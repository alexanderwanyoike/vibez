//! Lane point, outer-storage and mailbox owners are destroyed only on the non-audio owner thread.

use super::*;

fn on_audio_thread(mut engine: AudioEngine, track: TrackId, sounding: bool) -> AudioEngine {
    let owner = std::thread::current().id();
    std::thread::spawn(move || {
        assert_ne!(std::thread::current().id(), owner);
        let mut output = [0.0; 128];
        let mut capture = [0.0; 128];
        let length = if sounding { output.len() } else { 0 };
        let position = engine.transport().position();
        let output_position = engine.output_position_samples();
        assert_eq!(crate::retirement::tests::allocations(|| {
            engine.process_block(AudioProcessBlock::new(&mut output[..length], 2)
                .with_track_output_capture(track.raw(), &mut capture[..length]));
        }), (0, 0));
        let frames = u64::from(sounding) * 64;
        assert_eq!(engine.transport().position(), position + frames);
        assert_eq!(engine.output_position_samples(), output_position + frames);
        if sounding {
            for frame in output.as_chunks::<2>().0.iter().chain(capture.as_chunks::<2>().0) {
                assert_eq!(*frame, [0.25, 0.0]);
            }
        }
        engine
    }).join().unwrap()
}

fn publish_request(with_stop: bool) -> (Fixture, Delta, ClipInsertionStorageRequest) {
    let mut fixture = Fixture::new();
    let (delta, command) = Delta::prepare(lane(AutomationTarget::TrackPan, 0.0), fixture.track);
    fixture.commands.push(command).unwrap();
    if with_stop { fixture.commands.push(EngineCommand::Stop).unwrap(); }
    // Only the engine crosses threads; raw allocation-observer pointers remain on the owner.
    fixture.engine = on_audio_thread(fixture.engine, fixture.track, false);
    fixture.assert_original();
    assert!(fixture.engine.transport().is_playing());
    let EngineEvent::ClipInsertionStorageRequested(request) = fixture.events.pop().unwrap() else {
        panic!("the native worker must publish the original lane request");
    };
    assert_eq!(request.id, delta.id);
    assert_eq!((request.track_id, request.target), (fixture.track, ClipInsertionTarget::AutomationLane(delta.incoming.id)));
    assert_eq!(std::ptr::from_ref(request.sender.buffer()), delta.ring);
    assert!(matches!(request.kind, ClipInsertionKind::AutomationLane));
    assert_eq!(request.capacity, 4);
    assert_eq!(request.sender.slots(), 1);
    assert!(!request.sender.is_abandoned());
    assert_eq!(delta.identity.strong_count(), 3);
    let mut positions = 0;
    let mut meters = 0;
    while let Ok(event) = fixture.events.pop() {
        match event {
            EngineEvent::PlaybackPosition(value) => {
                assert_eq!(value, fixture.engine.transport().position());
                positions += 1;
            }
            EngineEvent::Metering { peak_l, peak_r, rms_l, rms_r } => {
                assert_eq!((peak_l, peak_r, rms_l, rms_r), (0.0, 0.0, 0.0, 0.0));
                meters += 1;
            }
            event => panic!("unexpected lane request-boundary event: {event:?}"),
        }
    }
    assert_eq!((positions, meters), (1, 1));
    fixture.engine = on_audio_thread(fixture.engine, fixture.track, true);
    fixture.assert_original();
    let mut positions = 0;
    let mut meters = 0;
    while let Ok(event) = fixture.events.pop() {
        match event {
            EngineEvent::PlaybackPosition(value) => {
                assert_eq!(value, fixture.engine.transport().position());
                positions += 1;
            }
            EngineEvent::TrackMeter { track_id, peak_l, peak_r } => {
                assert!([fixture.track, TrackId::MASTER].contains(&track_id));
                assert_eq!((peak_l, peak_r), (0.25, 0.0));
            }
            EngineEvent::Metering { peak_l, peak_r, rms_l, rms_r } => {
                assert_eq!((peak_l, peak_r, rms_l, rms_r), (0.25, 0.0, 0.25, 0.0));
                meters += 1;
            }
            event => panic!("a waiting lane endpoint must not admit a later command or owner: {event:?}"),
        }
    }
    assert_eq!((positions, meters), (1, 1));
    assert_eq!(delta.identity.strong_count(), 3);
    (fixture, delta, request)
}

#[test]
fn ready_after_producer_drop_and_abandoned_lanes_return_complete_owners_from_audio_worker() {
    let owner_thread = std::thread::current().id();
    for outcome in [ClipInsertionOutcome::Applied, ClipInsertionOutcome::Abandoned] {
        let (mut fixture, delta, request) = publish_request(true);
        if outcome == ClipInsertionOutcome::Applied {
            // Public fulfill publishes the single lane Vec before destroying its producer.
            assert_eq!(crate::retirement::tests::allocations(|| request.fulfill()), (1, 0));
        } else {
            assert_eq!(crate::retirement::tests::allocations(|| drop(request)), (0, 0));
        }
        assert_eq!(delta.identity.strong_count(), 2);
        fixture.engine = on_audio_thread(fixture.engine, fixture.track, false);
        let completed = take_control(&mut fixture.events);
        delta.assert_control(&completed, outcome, true);
        assert!(completed.owner.receiver.is_empty());
        if outcome == ClipInsertionOutcome::Applied {
            let Some(ClipInsertionStorage::AutomationLane(old)) = &completed.owner.reply else {
                panic!("the complete lane control must retain the original outer allocation");
            };
            assert_eq!(old.as_ptr(), fixture.outer);
            assert_eq!((old.len(), old.capacity()), (0, 1));
            assert!(completed.owner.clip.is_none());
            fixture.assert_current(&[fixture.resident.clone(), delta.incoming.clone()], &[fixture.points, delta.points], None);
            assert_eq!(fixture.engine.tracks()[0].playback_source.automation.capacity(), 4);
        } else {
            assert!(completed.owner.reply.is_none());
            let Some(ClipInsertionDelta::AutomationLane(original)) = &completed.owner.clip else {
                panic!("abandonment must return the original populated point owner");
            };
            assert_eq!(original, &delta.incoming);
            assert_eq!(original.points.as_ptr(), delta.points);
            fixture.assert_original();
        }
        fixture.finish_telemetry();
        // The held operation ID excludes its Arc from the two ring allocations,
        // control Box and either old outer Vec or abandoned point Vec freed here.
        assert_eq!(crate::retirement::tests::allocations(|| {
            assert_eq!(std::thread::current().id(), owner_thread);
            drop(completed);
        }), (0, 4));
        assert_eq!(delta.identity.strong_count(), 1);
        let disposed = crate::retirement::tests::allocations(|| {
            assert_eq!(std::thread::current().id(), owner_thread);
            drop(fixture.engine);
        });
        assert_eq!(disposed.0, 0);
        assert!(disposed.1 > 0);
        assert_eq!(fixture.audio.strong_count(), 0);
        let identity = delta.identity.clone();
        drop(delta);
        assert_eq!(identity.strong_count(), 0);
    }
}

#[test]
fn producer_first_lane_shutdown_reclaims_an_unconsumed_reply_on_owner_thread() {
    let owner_thread = std::thread::current().id();
    let (fixture, delta, request) = publish_request(false);
    assert_eq!(crate::retirement::tests::allocations(|| request.fulfill()), (1, 0));
    assert_eq!(delta.identity.strong_count(), 2);
    fixture.assert_original();
    // No callback consumes the occupied reply; final consumer destruction owns it.
    let disposed = crate::retirement::tests::allocations(|| {
        assert_eq!(std::thread::current().id(), owner_thread);
        drop(fixture.engine);
    });
    assert_eq!(disposed.0, 0);
    assert!(disposed.1 > 0);
    assert_eq!(fixture.audio.strong_count(), 0);
    assert_eq!(delta.identity.strong_count(), 1);
    let identity = delta.identity.clone();
    drop(delta);
    assert_eq!(identity.strong_count(), 0);
}

#[test]
fn consumer_first_lane_shutdown_reclaims_public_and_successfully_orphaned_replies() {
    let owner_thread = std::thread::current().id();
    for inspect_push in [false, true] {
        let (fixture, delta, mut request) = publish_request(false);
        fixture.assert_original();
        let disposed = crate::retirement::tests::allocations(|| {
            assert_eq!(std::thread::current().id(), owner_thread);
            drop(fixture.engine);
        });
        assert_eq!(disposed.0, 0);
        assert!(disposed.1 > 0);
        assert_eq!(fixture.audio.strong_count(), 0);
        assert_eq!(delta.identity.strong_count(), 2);
        assert!(request.sender.is_abandoned());
        assert_eq!(std::ptr::from_ref(request.sender.buffer()), delta.ring);
        if inspect_push {
            let mut storage = None;
            assert_eq!(crate::retirement::tests::allocations(|| {
                storage = Some(ClipInsertionStorage::new(request.kind, request.capacity));
            }), (1, 0));
            let ClipInsertionStorage::AutomationLane(lanes) = storage.as_ref().unwrap() else { unreachable!() };
            assert_eq!((lanes.len(), lanes.capacity()), (0, 4));
            let mut pushed = None;
            // This private endpoint observation distinguishes a successful orphan
            // push from Err cleanup; the other leg uses public production fulfill.
            assert_eq!(crate::retirement::tests::allocations(|| {
                pushed = Some(request.sender.push(storage.take().unwrap()));
            }), (0, 0));
            assert!(pushed.unwrap().is_ok());
            assert_eq!(request.sender.slots(), 0);
            assert_eq!(crate::retirement::tests::allocations(|| {
                assert_eq!(std::thread::current().id(), owner_thread);
                drop(request);
            }), (0, 3));
        } else {
            assert_eq!(crate::retirement::tests::allocations(|| {
                assert_eq!(std::thread::current().id(), owner_thread);
                request.fulfill();
            }), (1, 3));
        }
        assert_eq!(delta.identity.strong_count(), 1);
        let identity = delta.identity.clone();
        drop(delta);
        assert_eq!(identity.strong_count(), 0);
    }
}
