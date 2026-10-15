//! Public note clip exchanges retain complete owners at bounded retirement pressure.

use super::*;
use crate::mixer::EngineTrack;
use std::sync::Weak;
use vibez_core::automation::{AutomationLane, AutomationPoint, AutomationTarget};
use vibez_core::midi::MidiNote;
use vibez_core::routing::RoutingChannel;

struct Fixture {
    engine: AudioEngine,
    commands: Producer<EngineCommand>,
    events: Consumer<EngineEvent>,
    track: TrackId,
    resident: ClipId,
    notes: *const MidiNote,
    live: *const EngineNoteClip,
    staging: *const EngineNoteClip,
}

fn setup() -> Fixture {
    setup_with_notes(vec![MidiNote {
        pitch: 60,
        velocity: 100,
        start_beat: 0.0,
        duration_beats: 4.0,
    }])
}

fn setup_with_notes(notes: Vec<MidiNote>) -> Fixture {
    let sounding = !notes.is_empty();
    let (mut engine, mut commands, mut events) = AudioEngine::new();
    let track = TrackId::new();
    let resident = ClipId::new();
    let source = PreparedPlaybackSource::new(
        Vec::new(),
        vec![EngineNoteClip::new(
            resident,
            0.0,
            8.0,
            notes,
            0.0,
            false,
            0.0,
            8.0,
            GrooveGrid::Off,
        )],
        Vec::new(),
    );
    let mut channel = EngineTrack::with_playback_source(track, source);
    channel.pan = 0.0;
    channel.instrument = Some(Box::new(crate::test_support::DelayProbeInstrument::new(
        0, 0.25,
    )));
    commands
        .push(EngineCommand::AddTrack(Box::new(channel)))
        .unwrap();
    let model: Vec<_> = [track, TrackId::MASTER]
        .into_iter()
        .map(|id| RoutingChannel {
            id,
            is_bus: id.is_master(),
            sends: Vec::new(),
            effects: Vec::new(),
        })
        .collect();
    commands
        .push(EngineCommand::SetRouting(
            crate::routing::PreparedRouting::prepare(&model, 8).unwrap(),
        ))
        .unwrap();
    commands.push(EngineCommand::Play).unwrap();
    let mut warmup = [0.0; 16];
    engine.process_block(AudioProcessBlock::new(&mut warmup, 2));
    if sounding {
        assert_signal(&warmup);
    } else {
        assert_eq!(warmup, [0.0; 16]);
    }
    while events.pop().is_ok() {}
    let source = &engine.tracks()[0].playback_source;
    assert_eq!(source.note_clips.len(), source.note_clips.capacity());
    let notes = source.note_clips[0].notes.as_ptr();
    let live = source.note_clips.as_ptr();
    let staging = source.removed_note_clips.as_ptr();
    Fixture {
        engine,
        commands,
        events,
        track,
        resident,
        notes,
        live,
        staging,
    }
}

fn callback(engine: &mut AudioEngine, sounding: bool) -> [f32; 16] {
    let mut output = [0.0; 16];
    let length = if sounding { output.len() } else { 0 };
    assert_eq!(
        crate::retirement::tests::allocations(|| {
            engine.process_block(AudioProcessBlock::new(&mut output[..length], 2));
        }),
        (0, 0)
    );
    output
}

fn assert_signal(output: &[f32]) {
    for &[left, right] in output.as_chunks::<2>().0 {
        assert_eq!((left, right), (0.25, 0.0));
    }
}

struct Delta {
    command: Option<EngineCommand>,
    id: ClipInsertionId,
    identity: Weak<()>,
    clip: ClipId,
    control: *const PreparedClipInsertion,
    ring: *const RingBuffer<ClipInsertionStorage>,
    eager: *const EngineNoteClip,
    eager_staging: *const EngineNoteClip,
}

fn delta(track: TrackId) -> Delta {
    delta_with_capacity(track, 1)
}

fn delta_with_capacity(track: TrackId, capacity: usize) -> Delta {
    let clip = ClipId::new();
    let insertion = PreparedNoteClipInsertion::with_capacity(
        clip,
        10.0,
        2.0,
        0.75,
        true,
        0.25,
        1.5,
        GrooveGrid::Eighth,
        capacity,
    );
    let id = insertion.id();
    let identity = Arc::downgrade(&id.0);
    let control = std::ptr::from_ref(insertion.0.as_ref());
    let ring = std::ptr::from_ref(insertion.0.receiver.buffer());
    let ClipInsertionStorage::NoteClip(storage) = &insertion.0.storage else {
        unreachable!()
    };
    let eager = storage.live.as_ptr();
    let eager_staging = storage.removed.as_ptr();
    Delta {
        command: Some(EngineCommand::AddNoteClip {
            track_id: track,
            insertion,
        }),
        id,
        identity,
        clip,
        control,
        ring,
        eager,
        eager_staging,
    }
}

fn full_ui_ring(engine: &mut AudioEngine) -> usize {
    let mut count = 0;
    while engine
        .event_tx
        .push(EngineEvent::PlaybackPosition(u64::MAX))
        .is_ok()
    {
        count += 1;
    }
    assert!(count > 0);
    assert_eq!(engine.event_tx.slots(), 0);
    count
}

fn inject_full_retirement_store(
    engine: &mut AudioEngine,
) -> Vec<(AutomationLane, *const AutomationPoint)> {
    assert!(engine.pending_retirements.is_empty());
    assert_eq!(engine.pending_retirements.capacity(), 32);
    let mut expected = Vec::new();
    for index in 0..engine.pending_retirements.capacity() {
        let mut lane = AutomationLane::new(AutomationTarget::TrackPan);
        lane.points = vec![AutomationPoint {
            beat: 0.0,
            value: index as f32 / 64.0,
            curve: 0.0,
        }];
        let allocation = lane.points.as_ptr();
        expected.push((lane.clone(), allocation));
        // Explicit off-callback fixture injection tests the bounded store;
        // Insertion, request serving, rendering and completion remain public.
        engine.retire_event(EngineEvent::RetiredAutomationLane(lane));
    }
    assert_eq!(engine.pending_retirements.len(), 32);
    expected
}

#[derive(Default)]
struct Received {
    fillers: usize,
    lanes: Vec<AutomationLane>,
    requests: Vec<ClipInsertionStorageRequest>,
    completed: Vec<CompletedClipInsertion>,
}

impl Received {
    fn drain(&mut self, events: &mut Consumer<EngineEvent>) {
        while let Ok(event) = events.pop() {
            match event {
                EngineEvent::PlaybackPosition(u64::MAX) => self.fillers += 1,
                EngineEvent::RetiredAutomationLane(lane) => self.lanes.push(lane),
                EngineEvent::ClipInsertionStorageRequested(request) => self.requests.push(request),
                EngineEvent::ClipInsertionCompleted(completed) => self.completed.push(completed),
                _ => {}
            }
        }
    }

    fn assert_lanes(&self, expected: &[(AutomationLane, *const AutomationPoint)]) {
        assert_eq!(self.lanes.len(), expected.len());
        for (lane, allocation) in expected {
            let matching: Vec<_> = self
                .lanes
                .iter()
                .filter(|value| value.id == lane.id)
                .collect();
            assert_eq!(matching.len(), 1);
            assert_eq!(matching[0], lane);
            assert_eq!(matching[0].points.as_ptr(), *allocation);
        }
    }
}

fn assert_resident(fixture: &Fixture) {
    let resident = &fixture.engine.tracks()[0].playback_source.note_clips[0];
    assert_eq!(resident.id, fixture.resident);
    assert_eq!(resident.notes.as_ptr(), fixture.notes);
    assert_eq!(
        resident.notes,
        [MidiNote {
            pitch: 60,
            velocity: 100,
            start_beat: 0.0,
            duration_beats: 4.0
        }]
    );
    assert_eq!(
        (
            resident.position_beats,
            resident.duration_beats,
            resident.start_marker_beats
        ),
        (0.0, 8.0, 0.0)
    );
    assert!(!resident.loop_enabled);
    assert_eq!(
        (resident.loop_start_beats, resident.loop_end_beats),
        (0.0, 8.0)
    );
    assert_eq!(resident.groove_grid, GrooveGrid::Off);
}

fn assert_unchanged(fixture: &Fixture) {
    let source = &fixture.engine.tracks()[0].playback_source;
    assert_eq!(source.note_clips.len(), 1);
    assert_eq!(source.note_clips.as_ptr(), fixture.live);
    assert_eq!(source.removed_note_clips.as_ptr(), fixture.staging);
    assert!(source.removed_note_clips.is_empty());
    assert_resident(fixture);
}

fn assert_complete(
    fixture: &Fixture,
    delta: &Delta,
    owner: &CompletedClipInsertion,
    outcome: ClipInsertionOutcome,
) {
    assert_eq!(owner.id(), delta.id);
    assert_eq!((owner.track_id, owner.target), (fixture.track, crate::clip_insertion::ClipInsertionTarget::Clip(delta.clip)));
    assert_eq!(owner.outcome, outcome);
    assert_eq!(std::ptr::from_ref(owner.owner.as_ref()), delta.control);
    assert_eq!(
        std::ptr::from_ref(owner.owner.receiver.buffer()),
        delta.ring
    );
    assert!(owner.owner.sender.is_none());
    assert!(owner.owner.receiver.is_abandoned());
    let ClipInsertionStorage::NoteClip(eager) = &owner.owner.storage else {
        panic!("complete note eager owner")
    };
    assert_eq!(eager.live.as_ptr(), delta.eager);
    assert_eq!(eager.removed.as_ptr(), delta.eager_staging);
    assert!(eager.live.is_empty());
    assert!(eager.removed.is_empty());
    let clip = match outcome {
        ClipInsertionOutcome::Applied => {
            let Some(ClipInsertionStorage::NoteClip(old)) = &owner.owner.reply else {
                panic!("complete old note storage")
            };
            assert!(old.live.is_empty());
            assert!(old.removed.is_empty());
            assert_eq!(old.live.as_ptr(), fixture.live);
            assert_eq!(old.removed.as_ptr(), fixture.staging);
            assert!(owner.owner.clip.is_none());
            let source = &fixture.engine.tracks()[0].playback_source;
            assert_eq!(source.note_clips.len(), 2);
            assert!(source.note_clips.capacity() >= 2);
            assert!(source.removed_note_clips.capacity() >= 2);
            assert!(source.removed_note_clips.is_empty());
            &source.note_clips[1]
        }
        ClipInsertionOutcome::Abandoned => {
            assert!(owner.owner.reply.is_none());
            assert_unchanged(fixture);
            let Some(ClipInsertionDelta::NoteClip(clip)) = &owner.owner.clip else {
                panic!("complete rejected original note clip")
            };
            clip
        }
        ClipInsertionOutcome::MissingTrack | ClipInsertionOutcome::MissingClip => unreachable!(),
    };
    assert_eq!(clip.id, delta.clip);
    assert_eq!(
        (
            clip.position_beats,
            clip.duration_beats,
            clip.start_marker_beats
        ),
        (10.0, 2.0, 0.75)
    );
    assert!(clip.loop_enabled);
    assert_eq!((clip.loop_start_beats, clip.loop_end_beats), (0.25, 1.5));
    assert_eq!(clip.groove_grid, GrooveGrid::Eighth);
    assert!(clip.notes.is_empty());
    assert_resident(fixture);
}

#[test]
fn full_retirement_store_blocks_note_clip_admission_until_two_current_credits() {
    let mut fixture = setup();
    let mut delta = delta(fixture.track);
    let full_ring = full_ui_ring(&mut fixture.engine);
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
        assert_unchanged(&fixture);
        assert_eq!(delta.identity.strong_count(), 2);
    }
    assert!(matches!(
        fixture.events.pop().unwrap(),
        EngineEvent::PlaybackPosition(u64::MAX)
    ));
    callback(&mut fixture.engine, false);
    assert_eq!(fixture.engine.pending_retirements.len(), 31);
    assert_eq!(fixture.commands.slots(), slots);
    assert_unchanged(&fixture);
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
    assert_unchanged(&fixture);
    let request = received.requests.pop().unwrap();
    assert_eq!(request.id, delta.id);
    assert_eq!(std::ptr::from_ref(request.sender.buffer()), delta.ring);
    request.fulfill();
    callback(&mut fixture.engine, false);
    received.drain(&mut fixture.events);
    assert_eq!(received.completed.len(), 1);
    assert_complete(
        &fixture,
        &delta,
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
fn ready_or_abandoned_note_clip_waits_at_zero_and_uses_only_the_last_completion_credit() {
    for outcome in [
        ClipInsertionOutcome::Applied,
        ClipInsertionOutcome::Abandoned,
    ] {
        let mut fixture = setup();
        let mut delta = delta(fixture.track);
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
        assert_unchanged(&fixture);
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
        assert_complete(&fixture, &delta, completions[0], outcome);
        assert_eq!(fixture.engine.transport().bpm(), 120.0);
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
        assert_complete(&fixture, &delta, &received.completed[0], outcome);
        assert_eq!(fixture.engine.transport().bpm(), 137.0);
        assert_eq!(fixture.commands.slots(), slots + 2);
        assert!(fixture.engine.pending_retirements.is_empty());
        assert_eq!(fixture.engine.pending_retirements.capacity(), 32);
        assert_eq!(
            fixture.engine.pending_retirements.as_ptr(),
            retirement_allocation
        );
        let end = if outcome == ClipInsertionOutcome::Applied {
            12.0_f64
        } else {
            8.0_f64
        };
        assert_eq!(
            fixture.engine.transport().audio_length(),
            Some((end * 44_100.0 * 60.0 / 137.0).round() as u64)
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

#[path = "note_clip_insertion_lifecycle_tests.rs"]
mod lifecycle_tests;

#[path = "note_value_insertion_pressure_tests.rs"]
mod note_value_tests;
