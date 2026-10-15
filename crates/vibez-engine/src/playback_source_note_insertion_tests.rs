//! Public note-clip insertion preserves resident schedules and voices across full storage.

use super::*;
use crate::commands::EngineCommand;
use crate::engine::{AudioEngine, AudioProcessBlock};
use crate::mixer::EngineTrack;
use std::sync::atomic::{AtomicUsize, Ordering};
use vibez_core::effect::ParamDescriptor;
use vibez_core::midi::InstrumentKind;
use vibez_core::perform::SwingAmount;
use vibez_core::routing::RoutingChannel;
use vibez_instruments::Instrument;

#[derive(Default)]
struct VoiceCalls {
    note_on: AtomicUsize,
    note_off: AtomicUsize,
    reset: AtomicUsize,
}

impl VoiceCalls {
    fn observed(&self) -> (usize, usize, usize) {
        (
            self.note_on.load(Ordering::Relaxed),
            self.note_off.load(Ordering::Relaxed),
            self.reset.load(Ordering::Relaxed),
        )
    }
}

struct CountedInstrument {
    instrument: crate::test_support::DelayProbeInstrument,
    calls: Arc<VoiceCalls>,
}

impl Instrument for CountedInstrument {
    fn instrument_kind(&self) -> InstrumentKind {
        self.instrument.instrument_kind()
    }
    fn param_descriptors(&self) -> &'static [ParamDescriptor] {
        self.instrument.param_descriptors()
    }
    fn set_param(&mut self, index: usize, value: f32) -> bool {
        self.instrument.set_param(index, value)
    }
    fn get_param(&self, index: usize) -> f32 {
        self.instrument.get_param(index)
    }
    fn note_on(&mut self, pitch: u8, velocity: u8) {
        self.calls.note_on.fetch_add(1, Ordering::Relaxed);
        self.instrument.note_on(pitch, velocity);
    }
    fn note_off(&mut self, pitch: u8) {
        self.calls.note_off.fetch_add(1, Ordering::Relaxed);
        self.instrument.note_off(pitch);
    }
    fn render(&mut self, buffer: &mut [f32], channels: usize) {
        self.instrument.render(buffer, channels);
    }
    fn reset(&mut self) {
        self.calls.reset.fetch_add(1, Ordering::Relaxed);
        self.instrument.reset();
    }
}

struct ResidentSchedule {
    id: ClipId,
    position: f64,
    duration: f64,
    start_marker: f64,
    looping: bool,
    loop_start: f64,
    loop_end: f64,
    groove: GrooveGrid,
    notes: Vec<MidiNote>,
    notes_allocation: *const MidiNote,
    notes_capacity: usize,
    clamps: Vec<Option<f64>>,
    clamp_allocation: *const Option<f64>,
    clamp_capacity: usize,
    latch: Option<(i64, SwingAmount)>,
}

impl ResidentSchedule {
    fn observe(clip: &EngineNoteClip) -> Self {
        Self {
            id: clip.id,
            position: clip.position_beats,
            duration: clip.duration_beats,
            start_marker: clip.start_marker_beats,
            looping: clip.loop_enabled,
            loop_start: clip.loop_start_beats,
            loop_end: clip.loop_end_beats,
            groove: clip.groove_grid,
            notes: clip.notes.clone(),
            notes_allocation: clip.notes.as_ptr(),
            notes_capacity: clip.notes.capacity(),
            clamps: clip.next_same_pitch_start_beats.clone(),
            clamp_allocation: clip.next_same_pitch_start_beats.as_ptr(),
            clamp_capacity: clip.next_same_pitch_start_beats.capacity(),
            latch: clip.groove_latch.get(),
        }
    }

    fn assert_unchanged(&self, clip: &EngineNoteClip) {
        assert_eq!(clip.id, self.id);
        assert_eq!(clip.position_beats, self.position);
        assert_eq!(clip.duration_beats, self.duration);
        assert_eq!(clip.start_marker_beats, self.start_marker);
        assert_eq!(clip.loop_enabled, self.looping);
        assert_eq!(clip.loop_start_beats, self.loop_start);
        assert_eq!(clip.loop_end_beats, self.loop_end);
        assert_eq!(clip.groove_grid, self.groove);
        assert_eq!(clip.notes, self.notes);
        assert_eq!(clip.notes.as_ptr(), self.notes_allocation);
        assert_eq!(clip.notes.capacity(), self.notes_capacity);
        assert_eq!(clip.next_same_pitch_start_beats, self.clamps);
        assert_eq!(clip.next_same_pitch_start_beats.as_ptr(), self.clamp_allocation);
        assert_eq!(clip.next_same_pitch_start_beats.capacity(), self.clamp_capacity);
        assert_eq!(clip.groove_latch.get(), self.latch);
    }
}

pub(crate) fn resident_clip(index: usize, id: ClipId) -> EngineNoteClip {
    let note = |pitch, start_beat, duration_beats| MidiNote {
        pitch,
        velocity: 100,
        start_beat,
        duration_beats,
    };
    EngineNoteClip::new(
        id,
        index as f64 * 16.0,
        8.0,
        vec![note(60, 2.0, 1.0), note(60, 0.0, 4.0), note(60, 2.0, 0.5), note(67, 3.0, 1.0)],
        0.0,
        false,
        0.0,
        8.0,
        GrooveGrid::Sixteenth,
    )
}

fn assert_sounding(output: &[f32]) {
    for &[left, right] in output.as_chunks::<2>().0 {
        assert_eq!(left, 0.25);
        assert_eq!(right, 0.0);
    }
}

#[test]
fn public_add_note_clip_at_exhausted_capacity_preserves_schedule_and_voice_without_callback_heap_work() {
    let mut clips = Vec::with_capacity(2);
    let duplicate = ClipId::new();
    clips.push(resident_clip(0, duplicate));
    // Fill the actual nonzero allocation; allocator growth is not a content contract.
    while clips.len() < clips.capacity() {
        clips.push(resident_clip(clips.len(), ClipId::new()));
    }
    let capacity = clips.capacity();
    assert!(capacity >= 2);
    assert_eq!(clips.len(), capacity);
    let old_end = clips.last().unwrap().position_beats + 8.0;
    let source = PreparedPlaybackSource::new(Vec::new(), clips, Vec::new());
    let track_id = TrackId::new();
    let calls = Arc::new(VoiceCalls::default());
    let mut track = EngineTrack::with_playback_source(track_id, source);
    track.pan = 0.0;
    track.instrument = Some(Box::new(CountedInstrument {
        instrument: crate::test_support::DelayProbeInstrument::new(0, 0.25),
        calls: Arc::clone(&calls),
    }));
    let instrument_owner = crate::engine::reconfiguration::instrument_owner_identity(
        track.instrument.as_deref().unwrap(),
    );
    let (mut engine, mut commands, mut events) = AudioEngine::new();
    commands.push(EngineCommand::AddTrack(Box::new(track))).unwrap();
    let model: Vec<_> = [track_id, TrackId::MASTER]
        .into_iter()
        .map(|id| RoutingChannel {
            id,
            is_bus: id.is_master(),
            sends: Vec::new(),
            effects: Vec::new(),
        })
        .collect();
    commands.push(EngineCommand::SetRouting(
        crate::routing::PreparedRouting::prepare(&model, 128).unwrap(),
    )).unwrap();
    let swing = SwingAmount::new(0.62);
    commands.push(EngineCommand::SetProjectSwing(swing)).unwrap();
    commands.push(EngineCommand::Play).unwrap();
    let mut warmup = [0.0; 256];
    engine.process_block(AudioProcessBlock::new(&mut warmup, 2));
    assert_sounding(&warmup);
    while events.pop().is_ok() {}
    assert_eq!(engine.transport().position(), 128);
    assert!(engine.transport().is_playing());
    let samples_per_beat = 44_100.0 * 60.0 / 120.0;
    assert_eq!(engine.transport().audio_length(), Some((old_end * samples_per_beat).round() as u64));
    let resident = &engine.tracks()[0].playback_source.note_clips;
    assert_eq!(resident.len(), capacity);
    assert_eq!(resident.capacity(), capacity);
    assert_eq!(resident[0].next_same_pitch_start_beats, [None, Some(2.0), None, None]);
    assert_eq!(resident[0].groove_latch.get(), Some((0, swing)));
    let expected: Vec<_> = resident.iter().map(ResidentSchedule::observe).collect();
    let previous_calls = calls.observed();
    assert_eq!(previous_calls.0, 1);
    assert_eq!(previous_calls.1, 0);
    let position = old_end + 1.25;
    let duration = 5.5;
    let insertion = crate::clip_insertion::PreparedNoteClipInsertion::new(
        duplicate, position, duration, 0.75, true, 0.25, 2.5, GrooveGrid::Eighth,
    );
    let operation = insertion.id();
    commands.push(EngineCommand::AddNoteClip {
        track_id,
        insertion,
    }).unwrap();
    let mut measured = crate::retirement::tests::allocations(|| {
        engine.process_block(AudioProcessBlock::new(&mut [], 2));
    });
    let mut request = None;
    while let Ok(event) = events.pop() {
        if let crate::events::EngineEvent::ClipInsertionStorageRequested(value) = event {
            assert_eq!(value.id, operation);
            assert!(request.replace(value).is_none());
        }
    }
    assert!(request.is_some());
    let waiting = crate::retirement::tests::allocations(|| {
        engine.process_block(AudioProcessBlock::new(&mut [], 2));
    });
    measured.0 += waiting.0;
    measured.1 += waiting.1;
    request.unwrap().fulfill();
    let completion = crate::retirement::tests::allocations(|| {
        engine.process_block(AudioProcessBlock::new(&mut [], 2));
    });
    measured.0 += completion.0;
    measured.1 += completion.1;
    let mut completed = 0;
    while let Ok(event) = events.pop() {
        if let crate::events::EngineEvent::ClipInsertionCompleted(value) = event {
            assert_eq!(value.id(), operation);
            assert_eq!(value.outcome, crate::clip_insertion::ClipInsertionOutcome::Applied);
            completed += 1;
        }
    }
    assert_eq!(completed, 1);
    let mut output = [0.0; 16];
    let render = crate::retirement::tests::allocations(|| {
        engine.process_block(AudioProcessBlock::new(&mut output, 2));
    });
    measured.0 += render.0;
    measured.1 += render.1;
    assert_sounding(&output);
    assert_eq!(calls.observed(), previous_calls, "empty insertion must not retrigger, flush or reset a voice");
    assert_eq!(crate::engine::reconfiguration::instrument_owner_identity(
        engine.tracks()[0].instrument.as_deref().unwrap(),
    ), instrument_owner);
    assert!(engine.transport().is_playing());
    assert_eq!(engine.transport().position(), 136);
    assert_eq!(engine.transport().bpm(), 120.0);
    assert_eq!(engine.transport().audio_length(), Some(((position + duration) * samples_per_beat).round() as u64));
    let resident = &engine.tracks()[0].playback_source.note_clips;
    assert_eq!(resident.len(), capacity + 1);
    for (original, actual) in expected.iter().zip(&resident[..capacity]) {
        original.assert_unchanged(actual);
    }
    assert_eq!(resident.iter().filter(|clip| clip.id == duplicate).count(), 2);
    let appended = &resident[capacity];
    assert_eq!(appended.id, duplicate);
    assert_eq!(appended.position_beats, position);
    assert_eq!(appended.duration_beats, duration);
    assert_eq!(appended.start_marker_beats, 0.75);
    assert!(appended.loop_enabled);
    assert_eq!(appended.loop_start_beats, 0.25);
    assert_eq!(appended.loop_end_beats, 2.5);
    assert_eq!(appended.groove_grid, GrooveGrid::Eighth);
    assert!(appended.notes.is_empty());
    assert!(appended.next_same_pitch_start_beats.is_empty());
    assert_eq!(appended.groove_latch.get(), None);
    assert_eq!(measured, (0, 0), "AddNoteClip callback allocation/destruction counts");
}

fn measured_callback(engine: &mut AudioEngine, output: &mut [f32]) {
    assert_eq!(crate::retirement::tests::allocations(|| {
        engine.process_block(AudioProcessBlock::new(output, 2));
    }), (0, 0));
}

#[test]
fn pending_note_clip_growth_preserves_current_duplicate_edits_and_voice_release_fifo() {
    use crate::clip_insertion::{ClipInsertionOutcome, PreparedNoteClipInsertion};
    use crate::events::EngineEvent;

    let duplicate = ClipId::new();
    let clips = vec![resident_clip(0, duplicate), resident_clip(1, ClipId::new())]
        .into_boxed_slice().into_vec();
    assert_eq!(clips.len(), clips.capacity());
    let source = PreparedPlaybackSource::new(Vec::new(), clips, Vec::new());
    let scheduled = TrackId::new();
    let live = TrackId::new();
    let scheduled_calls = Arc::new(VoiceCalls::default());
    let live_calls = Arc::new(VoiceCalls::default());
    let (mut engine, mut commands, mut events) = AudioEngine::new();
    for (id, source, amplitude, calls) in [
        (scheduled, source, 0.25, Arc::clone(&scheduled_calls)),
        (live, PreparedPlaybackSource::default(), 0.125, Arc::clone(&live_calls)),
    ] {
        let mut track = EngineTrack::with_playback_source(id, source);
        track.pan = 0.0;
        track.instrument = Some(Box::new(CountedInstrument {
            instrument: crate::test_support::DelayProbeInstrument::new(0, amplitude),
            calls,
        }));
        commands.push(EngineCommand::AddTrack(Box::new(track))).unwrap();
    }
    let model: Vec<_> = [scheduled, live, TrackId::MASTER]
        .into_iter().map(|id| RoutingChannel {
            id, is_bus: id.is_master(), sends: Vec::new(), effects: Vec::new(),
        }).collect();
    commands.push(EngineCommand::SetRouting(
        crate::routing::PreparedRouting::prepare(&model, 128).unwrap(),
    )).unwrap();
    let swing = SwingAmount::new(0.62);
    commands.push(EngineCommand::SetProjectSwing(swing)).unwrap();
    commands.push(EngineCommand::Play).unwrap();
    commands.push(EngineCommand::ExternalNoteOn { track_id: live, pitch: 72, velocity: 100 }).unwrap();
    let mut warmup = [0.0; 256];
    engine.process_block(AudioProcessBlock::new(&mut warmup, 2));
    for &[left, right] in warmup.as_chunks::<2>().0 {
        assert_eq!((left, right), (0.375, 0.0));
    }
    while events.pop().is_ok() {}
    assert_eq!(engine.transport().position(), 128);
    let resident = &engine.tracks()[0].playback_source.note_clips;
    assert_eq!(resident.len(), 2);
    assert_eq!(resident.capacity(), 2);
    assert_eq!(resident[0].groove_latch.get(), Some((0, swing)));
    let mut expected: Vec<_> = resident.iter().map(ResidentSchedule::observe).collect();
    let owners: Vec<_> = engine.tracks().iter().map(|track| {
        crate::engine::reconfiguration::instrument_owner_identity(track.instrument.as_deref().unwrap())
    }).collect();
    let before_scheduled = scheduled_calls.observed();
    let before_live = live_calls.observed();
    assert_eq!(before_scheduled.0, 1);
    assert_eq!(before_scheduled.1, 0);
    assert_eq!(before_live.0, 1);
    assert_eq!(before_live.1, 0);
    let empty_slots = commands.slots();
    let first = PreparedNoteClipInsertion::new(
        duplicate, 26.0, 2.5, 0.5, true, 0.25, 1.5, GrooveGrid::Eighth,
    );
    let first_id = first.id();
    let second = PreparedNoteClipInsertion::new(
        duplicate, 29.0, 3.5, 0.25, false, 0.0, 0.0, GrooveGrid::Off,
    );
    let second_id = second.id();
    commands.push(EngineCommand::AddNoteClip { track_id: scheduled, insertion: first }).unwrap();
    commands.push(EngineCommand::MoveNoteClip {
        track_id: scheduled, clip_id: duplicate, new_position_beats: 0.5,
    }).unwrap();
    commands.push(EngineCommand::AddNoteClip { track_id: scheduled, insertion: second }).unwrap();
    commands.push(EngineCommand::ExternalNoteOff { track_id: live, pitch: 72 }).unwrap();
    let queued_slots = commands.slots();
    assert_eq!(empty_slots - queued_slots, 4);
    measured_callback(&mut engine, &mut []);
    let mut request = None;
    while let Ok(event) = events.pop() {
        match event {
            EngineEvent::ClipInsertionStorageRequested(value) => {
                assert_eq!(value.id, first_id);
                assert_eq!((value.track_id, value.clip_id), (scheduled, duplicate));
                assert!(request.replace(value).is_none());
            }
            EngineEvent::ClipInsertionCompleted(_) => panic!("growth cannot complete before fulfillment"),
            _ => {}
        }
    }
    let request = request.expect("actually full note storage must request growth");
    assert_eq!(commands.slots(), queued_slots + 1);
    for callback in 0..4 {
        let mut output = [0.0; 16];
        measured_callback(&mut engine, &mut output);
        for &[left, right] in output.as_chunks::<2>().0 {
            assert_eq!((left, right), (0.375, 0.0));
        }
        assert_eq!(engine.transport().position(), 128 + (callback + 1) * 8);
        assert!(engine.transport().is_playing());
        assert_eq!(commands.slots(), queued_slots + 1);
        assert_eq!(scheduled_calls.observed(), before_scheduled);
        assert_eq!(live_calls.observed(), before_live);
        let resident = &engine.tracks()[0].playback_source.note_clips;
        assert_eq!(resident.len(), 2);
        for (original, actual) in expected.iter().zip(resident) {
            original.assert_unchanged(actual);
        }
        while let Ok(event) = events.pop() {
            assert!(!matches!(event, EngineEvent::ClipInsertionCompleted(_)
                | EngineEvent::ClipInsertionStorageRequested(_)
                | EngineEvent::SourceNoteInput { on: false, .. }
                | EngineEvent::InstrumentNoteInput { on: false, .. }));
        }
    }
    request.fulfill();
    measured_callback(&mut engine, &mut []);
    let mut completed = Vec::new();
    let mut order = Vec::new();
    while let Ok(event) = events.pop() {
        match event {
            EngineEvent::ClipInsertionCompleted(value) => {
                assert_eq!(value.outcome, ClipInsertionOutcome::Applied);
                assert_eq!((value.track_id, value.clip_id), (scheduled, duplicate));
                order.push("complete");
                completed.push(value);
            }
            EngineEvent::ClipInsertionStorageRequested(_) => panic!("the shared grown capacity must hold both deltas"),
            EngineEvent::SourceNoteInput { track_id, pitch, on: false, position, .. } => {
                assert_eq!((track_id, pitch), (live, 72));
                assert_eq!((position.effective_at_samples, position.canonical_at_samples), (160, 160));
                assert_eq!(position.section_id, None);
                assert_eq!(position.section_position_samples, None);
                assert_eq!(position.canonical_section_position_samples, None);
                order.push("source-off");
            }
            EngineEvent::InstrumentNoteInput { track_id, pitch, on: false, effective_at_samples, .. } => {
                assert_eq!((track_id, pitch, effective_at_samples), (live, 72, 160));
                order.push("heard-off");
            }
            _ => {}
        }
    }
    assert_eq!(order, ["complete", "complete", "source-off", "heard-off"]);
    assert_eq!(completed.len(), 2);
    assert_eq!(completed[0].id(), first_id);
    assert_eq!(completed[1].id(), second_id);
    assert_eq!(commands.slots(), empty_slots);
    assert_eq!(scheduled_calls.observed(), before_scheduled);
    assert_eq!(live_calls.observed(), (before_live.0, before_live.1 + 1, before_live.2));
    assert_eq!(engine.transport().position(), 160);
    assert!(engine.transport().is_playing());
    assert_eq!(engine.transport().audio_length(), Some((32.5_f64 * 22_050.0).round() as u64));
    expected[0].position = 0.5;
    let resident = &engine.tracks()[0].playback_source.note_clips;
    assert_eq!(resident.len(), 4);
    for (original, actual) in expected.iter().zip(&resident[..2]) {
        original.assert_unchanged(actual);
    }
    assert_eq!(resident.iter().filter(|clip| clip.id == duplicate).count(), 3);
    for (index, position, duration, start, looping, loop_start, loop_end, groove) in [
        (2, 26.0, 2.5, 0.5, true, 0.25, 1.5, GrooveGrid::Eighth),
        (3, 29.0, 3.5, 0.25, false, 0.0, 0.0, GrooveGrid::Off),
    ] {
        let clip = &resident[index];
        assert_eq!(clip.id, duplicate);
        assert_eq!(clip.position_beats, position);
        assert_eq!(clip.duration_beats, duration);
        assert_eq!(clip.start_marker_beats, start);
        assert_eq!(clip.loop_enabled, looping);
        assert_eq!(clip.loop_start_beats, loop_start);
        assert_eq!(clip.loop_end_beats, loop_end);
        assert_eq!(clip.groove_grid, groove);
        assert!(clip.notes.is_empty());
        assert!(clip.next_same_pitch_start_beats.is_empty());
        assert_eq!(clip.groove_latch.get(), None);
    }
    for (track, owner) in engine.tracks().iter().zip(owners) {
        assert_eq!(crate::engine::reconfiguration::instrument_owner_identity(
            track.instrument.as_deref().unwrap()), owner);
    }
    drop(completed);
    let mut output = [0.0; 16];
    measured_callback(&mut engine, &mut output);
    assert_sounding(&output);
    assert_eq!(scheduled_calls.observed(), before_scheduled);
    assert_eq!(live_calls.observed(), (before_live.0, before_live.1 + 1, before_live.2));
    // Stop clears groove latches by policy, so insertion preservation is checked before it.
    commands.push(EngineCommand::Stop).unwrap();
    measured_callback(&mut engine, &mut []);
    assert!(!engine.transport().is_playing());
    assert_eq!(scheduled_calls.observed(), (before_scheduled.0, before_scheduled.1 + 1, before_scheduled.2));
    assert_eq!(live_calls.observed(), (before_live.0, before_live.1 + 1, before_live.2));
    assert!(engine.tracks()[0].playback_source.note_clips.iter().all(|clip| clip.groove_latch.get().is_none()));
}


pub(crate) fn verify_prepared_note_storage_growth(mut source: PreparedPlaybackSource, count: usize) {
    use crate::clip_insertion::{ClipInsertionOutcome, PreparedNoteClipInsertion};
    use crate::events::EngineEvent;

    assert_eq!(source.note_clips.len(), count);
    assert!(source.removed_note_clips.is_empty());
    assert!(source.removed_note_clips.capacity() >= count);
    for (index, clip) in source.note_clips.iter().enumerate() {
        let expected = resident_clip(index, clip.id);
        assert_eq!(clip.position_beats, expected.position_beats);
        assert_eq!(clip.duration_beats, expected.duration_beats);
        assert_eq!(clip.notes, expected.notes);
        assert_eq!(clip.next_same_pitch_start_beats, [None, Some(2.0), None, None]);
        assert_eq!(clip.groove_latch.get(), None);
    }
    // Offline Vec growth may leave spare slots. Fill its actual capacity off
    // callback so every row exercises exhaustion instead of trusting a hint.
    let prepared: Vec<_> = source.note_clips.iter().map(ResidentSchedule::observe).collect();
    let full = source.note_clips.capacity();
    while source.note_clips.len() < full {
        source.push_note_clip(resident_clip(source.note_clips.len(), ClipId::new()));
    }
    for (snapshot, clip) in prepared.iter().zip(&source.note_clips) { snapshot.assert_unchanged(clip); }
    assert!(source.removed_note_clips.capacity() >= full);
    let live = source.note_clips.as_ptr();
    let staging = source.removed_note_clips.as_ptr();
    let old_live_capacity = source.note_clips.capacity();
    let old_staging_capacity = source.removed_note_clips.capacity();
    let swing = SwingAmount::new(0.62);
    for (index, clip) in source.note_clips.iter().enumerate() {
        clip.groove_latch.set(Some((index as i64, swing)));
    }
    let snapshots: Vec<_> = source.note_clips.iter().map(ResidentSchedule::observe).collect();
    let track_id = TrackId::new();
    let calls = Arc::new(VoiceCalls::default());
    let mut track = EngineTrack::with_playback_source(track_id, source);
    track.pan = 0.0;
    track.instrument = Some(Box::new(CountedInstrument {
        instrument: crate::test_support::DelayProbeInstrument::new(0, 0.25),
        calls: Arc::clone(&calls),
    }));
    let (mut engine, mut commands, mut events) = AudioEngine::new();
    commands.push(EngineCommand::AddTrack(Box::new(track))).unwrap();
    let model: Vec<_> = [track_id, TrackId::MASTER].into_iter().map(|id| RoutingChannel {
        id, is_bus: id.is_master(), sends: Vec::new(), effects: Vec::new(),
    }).collect();
    commands.push(EngineCommand::SetRouting(crate::routing::PreparedRouting::prepare(&model, 128).unwrap())).unwrap();
    commands.push(EngineCommand::SetProjectSwing(swing)).unwrap();
    commands.push(EngineCommand::Play).unwrap();
    let mut warm = [0.0; 256];
    engine.process_block(AudioProcessBlock::new(&mut warm, 2));
    assert_sounding(&warm);
    while events.pop().is_ok() {}
    for (snapshot, clip) in snapshots.iter().zip(&engine.tracks()[0].playback_source.note_clips) {
        snapshot.assert_unchanged(clip);
    }
    let previous_calls = calls.observed();
    let clip_id = ClipId::new();
    let insertion = PreparedNoteClipInsertion::new(clip_id, full as f64 * 16.0, 5.5,
        0.75, true, 0.25, 2.5, GrooveGrid::Eighth);
    let operation = insertion.id();
    commands.push(EngineCommand::AddNoteClip { track_id, insertion }).unwrap();
    measured_callback(&mut engine, &mut []);
    let EngineEvent::ClipInsertionStorageRequested(request) = events.pop().unwrap() else {
        panic!("actual exhausted note storage must request growth");
    };
    assert_eq!(request.id, operation);
    assert_eq!((request.track_id, request.clip_id), (track_id, clip_id));
    let mut output = [0.0; 16];
    measured_callback(&mut engine, &mut output);
    assert_sounding(&output);
    assert_eq!(engine.tracks()[0].playback_source.note_clips.as_ptr(), live);
    assert_eq!(engine.tracks()[0].playback_source.removed_note_clips.as_ptr(), staging);
    for (snapshot, clip) in snapshots.iter().zip(&engine.tracks()[0].playback_source.note_clips) {
        snapshot.assert_unchanged(clip);
    }
    while let Ok(event) = events.pop() {
        assert!(!matches!(event, EngineEvent::ClipInsertionStorageRequested(_) | EngineEvent::ClipInsertionCompleted(_)));
    }
    request.fulfill();
    measured_callback(&mut engine, &mut []);
    let EngineEvent::ClipInsertionCompleted(completed) = events.pop().unwrap() else {
        panic!("fulfilled current capacity must return its complete old storage");
    };
    assert_eq!(completed.id(), operation);
    assert_eq!(completed.outcome, ClipInsertionOutcome::Applied);
    assert_eq!((completed.track_id, completed.clip_id), (track_id, clip_id));
    let source = &engine.tracks()[0].playback_source;
    assert_eq!(source.note_clips.len(), full + 1);
    assert!(source.note_clips.capacity() >= full + 1);
    assert!(source.removed_note_clips.capacity() >= full + 1);
    assert!(source.removed_note_clips.is_empty());
    assert_ne!(source.note_clips.as_ptr(), live);
    assert_ne!(source.removed_note_clips.as_ptr(), staging);
    assert!(old_live_capacity >= count && old_staging_capacity >= count);
    for (snapshot, clip) in snapshots.iter().zip(&source.note_clips[..full]) { snapshot.assert_unchanged(clip); }
    let appended = &source.note_clips[full];
    assert_eq!(appended.id, clip_id);
    assert_eq!(appended.position_beats, full as f64 * 16.0);
    assert_eq!(appended.duration_beats, 5.5);
    assert_eq!(appended.start_marker_beats, 0.75);
    assert!(appended.loop_enabled);
    assert_eq!(appended.loop_start_beats, 0.25);
    assert_eq!(appended.loop_end_beats, 2.5);
    assert_eq!(appended.groove_grid, GrooveGrid::Eighth);
    assert!(appended.notes.is_empty() && appended.next_same_pitch_start_beats.is_empty());
    assert_eq!(appended.groove_latch.get(), None);
    // The retained identity clone excludes its Arc from this count: two eager
    // Vecs, two emptied old Vecs, two mailbox allocations and the original Box.
    assert_eq!(crate::retirement::tests::allocations(|| drop(completed)), (0, 7));
    measured_callback(&mut engine, &mut output);
    assert_sounding(&output);
    assert_eq!(calls.observed(), previous_calls);
    assert_eq!(engine.transport().position(), 144);
    assert!(engine.transport().is_playing());
    assert_eq!(engine.transport().audio_length(), Some(((full as f64 * 16.0 + 5.5) * 22_050.0).round() as u64));
    while let Ok(event) = events.pop() {
        assert!(!matches!(event, EngineEvent::ClipInsertionStorageRequested(_) | EngineEvent::ClipInsertionCompleted(_)));
    }
}

#[test]
fn prepared_note_constructor_preserves_complete_schedules_and_grows_live_and_staging_at_boundaries() {
    for count in [1, 2, 20, 21, 128, 129, 257] {
        let clips: Vec<_> = (0..count).map(|index| resident_clip(index, ClipId::new())).collect();
        let snapshots: Vec<_> = clips.iter().map(ResidentSchedule::observe).collect();
        let source = PreparedPlaybackSource::new(Vec::new(), clips, Vec::new());
        for (snapshot, clip) in snapshots.iter().zip(&source.note_clips) { snapshot.assert_unchanged(clip); }
        verify_prepared_note_storage_growth(source, count);
    }
}
