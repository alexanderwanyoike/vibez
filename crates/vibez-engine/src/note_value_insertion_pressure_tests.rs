//! Note values and both live cache allocations remain indivisible under retirement pressure.

use super::*;
use crate::playback_source::note_value_insertion_tests::{ClipSnapshot, EmptyNoteStorageSnapshot};

struct NoteDelta {
    command: Option<EngineCommand>,
    id: ClipInsertionId,
    identity: Weak<()>,
    note: MidiNote,
    control: *const PreparedClipInsertion,
    ring: *const RingBuffer<ClipInsertionStorage>,
    eager: EmptyNoteStorageSnapshot,
    original: ClipSnapshot,
}

impl NoteDelta {
    fn prepare(fixture: &Fixture) -> Self {
        let delta = Self::with_capacity(fixture, 1);
        delta.original.assert_full();
        delta
    }

    fn with_capacity(fixture: &Fixture, capacity: usize) -> Self {
        let original =
            ClipSnapshot::observe(&fixture.engine.tracks()[0].playback_source.note_clips[0]);
        let note = MidiNote {
            pitch: 60,
            velocity: 93,
            start_beat: 1.0,
            duration_beats: 0.5,
        };
        let insertion = PreparedNoteInsertion::with_capacity(note, capacity);
        let id = insertion.id();
        let identity = Arc::downgrade(&id.0);
        let control = std::ptr::from_ref(insertion.0.as_ref());
        let ring = std::ptr::from_ref(insertion.0.receiver.buffer());
        let ClipInsertionStorage::NoteValue(storage) = &insertion.0.storage else {
            panic!("prepared note value must own both eager arrays");
        };
        let eager = EmptyNoteStorageSnapshot::observe(storage);
        Self {
            command: Some(EngineCommand::AddNote {
                track_id: fixture.track,
                clip_id: fixture.resident,
                insertion,
            }),
            id,
            identity,
            note,
            control,
            ring,
            eager,
            original,
        }
    }

    fn assert_waiting(&self, fixture: &Fixture) {
        assert_unchanged(fixture);
        self.original
            .assert_unchanged(&fixture.engine.tracks()[0].playback_source.note_clips[0]);
        assert_eq!(fixture.engine.transport().audio_length(), Some(8 * 22_050));
        assert!(fixture.engine.transport().is_playing());
    }

    fn assert_complete(
        &self,
        fixture: &Fixture,
        completed: &CompletedClipInsertion,
        outcome: ClipInsertionOutcome,
    ) {
        assert_eq!(completed.id(), self.id);
        assert_eq!(
            (completed.track_id, completed.clip_id),
            (fixture.track, fixture.resident)
        );
        assert_eq!(completed.outcome, outcome);
        assert_eq!(std::ptr::from_ref(completed.owner.as_ref()), self.control);
        assert_eq!(
            std::ptr::from_ref(completed.owner.receiver.buffer()),
            self.ring
        );
        assert!(completed.owner.sender.is_none());
        assert!(completed.owner.receiver.is_abandoned());
        let ClipInsertionStorage::NoteValue(eager) = &completed.owner.storage else {
            panic!("complete note value must retain both eager arrays");
        };
        self.eager.assert_unchanged(eager);
        let source = &fixture.engine.tracks()[0].playback_source;
        assert_eq!(source.note_clips.len(), 1);
        assert_eq!(source.note_clips.as_ptr(), fixture.live);
        assert_eq!(source.removed_note_clips.as_ptr(), fixture.staging);
        assert!(source.removed_note_clips.is_empty());
        match outcome {
            ClipInsertionOutcome::Applied => {
                let Some(ClipInsertionStorage::NoteValue(old)) = &completed.owner.reply else {
                    panic!("one complete return must retain both displaced live arrays");
                };
                self.original.assert_retired_storage(old);
                assert!(completed.owner.clip.is_none());
                self.original
                    .assert_appended(&source.note_clips[0], self.note);
            }
            ClipInsertionOutcome::Abandoned => {
                assert!(completed.owner.reply.is_none());
                let Some(ClipInsertionDelta::NoteValue(original)) = &completed.owner.clip else {
                    panic!("abandonment must return the original exact delta");
                };
                assert_eq!(*original, self.note);
                self.original.assert_unchanged(&source.note_clips[0]);
            }
            ClipInsertionOutcome::MissingTrack | ClipInsertionOutcome::MissingClip => {
                unreachable!()
            }
        }
    }
}

#[test]
fn full_retirement_store_blocks_note_value_before_pop_until_two_current_credits() {
    let mut fixture = setup();
    let mut delta = NoteDelta::prepare(&fixture);
    let full_ring = full_ui_ring(&mut fixture.engine);
    // Ordinary owners are injected off callback; public AddNote and its exchange
    // still exercise the actual full event ring and bounded retirement storage.
    let expected = inject_full_retirement_store(&mut fixture.engine);
    let retirement_allocation = fixture.engine.pending_retirements.as_ptr();
    fixture
        .commands
        .push(delta.command.take().unwrap())
        .unwrap();
    fixture.commands.push(EngineCommand::SetBpm(137.0)).unwrap();
    let slots = fixture.commands.slots();
    for block in 0..3 {
        assert_signal(&callback(&mut fixture.engine, true));
        assert_eq!(fixture.commands.slots(), slots);
        assert_eq!(fixture.engine.transport().position(), 16 + block * 8);
        assert_eq!(fixture.engine.transport().bpm(), 120.0);
        assert_eq!(fixture.engine.pending_retirements.len(), 32);
        assert_eq!(
            fixture.engine.pending_retirements.as_ptr(),
            retirement_allocation
        );
        delta.assert_waiting(&fixture);
        assert_eq!(delta.identity.strong_count(), 2);
    }
    assert!(matches!(
        fixture.events.pop().unwrap(),
        EngineEvent::PlaybackPosition(u64::MAX)
    ));
    callback(&mut fixture.engine, false);
    assert_eq!(fixture.engine.pending_retirements.len(), 31);
    assert_eq!(fixture.commands.slots(), slots);
    delta.assert_waiting(&fixture);
    let mut received = Received::default();
    received.drain(&mut fixture.events);
    assert_eq!(received.fillers, full_ring - 1);
    assert_eq!(received.lanes.len(), 1);
    assert!(received.requests.is_empty());
    callback(&mut fixture.engine, false);
    received.drain(&mut fixture.events);
    received.assert_lanes(&expected);
    assert_eq!(received.requests.len(), 1);
    assert!(received.completed.is_empty());
    assert_eq!(fixture.commands.slots(), slots + 1);
    assert_signal(&callback(&mut fixture.engine, true));
    assert_eq!(fixture.engine.transport().position(), 40);
    assert_eq!(fixture.engine.transport().bpm(), 120.0);
    delta.assert_waiting(&fixture);
    let request = received.requests.pop().unwrap();
    assert_eq!(request.id, delta.id);
    assert_eq!(std::ptr::from_ref(request.sender.buffer()), delta.ring);
    request.fulfill();
    callback(&mut fixture.engine, false);
    received.drain(&mut fixture.events);
    assert_eq!(received.completed.len(), 1);
    delta.assert_complete(
        &fixture,
        &received.completed[0],
        ClipInsertionOutcome::Applied,
    );
    assert_eq!(fixture.engine.transport().bpm(), 137.0);
    assert_eq!(fixture.commands.slots(), slots + 2);
    assert!(fixture.engine.pending_retirements.is_empty());
    assert_eq!(fixture.engine.pending_retirements.capacity(), 32);
    assert_eq!(
        fixture.engine.pending_retirements.as_ptr(),
        retirement_allocation
    );
    assert_signal(&callback(&mut fixture.engine, true));
    assert_eq!(fixture.engine.transport().position(), 48);
    received.drain(&mut fixture.events);
    received.assert_lanes(&expected);
    assert_eq!(received.completed.len(), 1);
    let identity = delta.identity.clone();
    drop(received);
    assert_eq!(identity.strong_count(), 1);
    drop(delta);
    assert_eq!(identity.strong_count(), 0);
}

#[test]
fn ready_or_unfulfilled_note_value_waits_at_zero_then_returns_both_arrays_on_one_credit() {
    for outcome in [
        ClipInsertionOutcome::Applied,
        ClipInsertionOutcome::Abandoned,
    ] {
        let mut fixture = setup();
        let mut delta = NoteDelta::prepare(&fixture);
        fixture
            .commands
            .push(delta.command.take().unwrap())
            .unwrap();
        fixture.commands.push(EngineCommand::SetBpm(137.0)).unwrap();
        let slots = fixture.commands.slots();
        callback(&mut fixture.engine, false);
        let mut received = Received::default();
        received.drain(&mut fixture.events);
        assert_eq!(received.requests.len(), 1);
        let request = received.requests.pop().unwrap();
        assert_eq!(request.id, delta.id);
        assert_eq!(std::ptr::from_ref(request.sender.buffer()), delta.ring);
        if outcome == ClipInsertionOutcome::Applied {
            request.fulfill();
        } else {
            drop(request);
        }
        let full_ring = full_ui_ring(&mut fixture.engine);
        let expected = inject_full_retirement_store(&mut fixture.engine);
        let retirement_allocation = fixture.engine.pending_retirements.as_ptr();
        assert_signal(&callback(&mut fixture.engine, true));
        assert_eq!(fixture.engine.transport().position(), 16);
        assert_eq!(fixture.engine.transport().bpm(), 120.0);
        assert_eq!(fixture.commands.slots(), slots + 1);
        assert_eq!(fixture.engine.pending_retirements.len(), 32);
        delta.assert_waiting(&fixture);
        assert_eq!(delta.identity.strong_count(), 2);
        assert!(matches!(
            fixture.events.pop().unwrap(),
            EngineEvent::PlaybackPosition(u64::MAX)
        ));
        callback(&mut fixture.engine, false);
        assert_eq!(fixture.engine.pending_retirements.len(), 32);
        assert_eq!(
            fixture.engine.pending_retirements.as_ptr(),
            retirement_allocation
        );
        let completions: Vec<_> = fixture
            .engine
            .pending_retirements
            .iter()
            .filter_map(|event| {
                if let EngineEvent::ClipInsertionCompleted(owner) = event {
                    Some(owner)
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(completions.len(), 1);
        delta.assert_complete(&fixture, completions[0], outcome);
        assert_eq!(fixture.engine.transport().bpm(), 120.0);
        assert_eq!(fixture.engine.transport().audio_length(), Some(8 * 22_050));
        assert_eq!(fixture.commands.slots(), slots + 1);
        assert_signal(&callback(&mut fixture.engine, true));
        assert_eq!(fixture.engine.transport().position(), 24);
        assert_eq!(fixture.engine.transport().bpm(), 120.0);
        assert_eq!(fixture.commands.slots(), slots + 1);
        received.drain(&mut fixture.events);
        assert_eq!(received.fillers, full_ring - 1);
        assert_eq!(received.lanes.len(), 1);
        assert!(received.completed.is_empty());
        assert!(received.requests.is_empty());
        callback(&mut fixture.engine, false);
        received.drain(&mut fixture.events);
        received.assert_lanes(&expected);
        assert_eq!(received.completed.len(), 1);
        delta.assert_complete(&fixture, &received.completed[0], outcome);
        assert_eq!(fixture.engine.transport().bpm(), 137.0);
        assert_eq!(fixture.commands.slots(), slots + 2);
        assert!(fixture.engine.pending_retirements.is_empty());
        assert_eq!(fixture.engine.pending_retirements.capacity(), 32);
        assert_eq!(
            fixture.engine.pending_retirements.as_ptr(),
            retirement_allocation
        );
        assert_eq!(
            fixture.engine.transport().audio_length(),
            Some((8.0_f64 * 44_100.0 * 60.0 / 137.0).round() as u64)
        );
        assert_signal(&callback(&mut fixture.engine, true));
        assert_eq!(fixture.engine.transport().position(), 32);
        received.drain(&mut fixture.events);
        received.assert_lanes(&expected);
        assert_eq!(received.completed.len(), 1);
        let identity = delta.identity.clone();
        drop(received);
        assert_eq!(identity.strong_count(), 1);
        drop(delta);
        assert_eq!(identity.strong_count(), 0);
    }
}

#[path = "note_value_insertion_lifecycle_tests.rs"]
mod lifecycle_tests;
