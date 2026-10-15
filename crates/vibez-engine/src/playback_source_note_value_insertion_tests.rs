//! Public note insertion preserves caller order, live voices and future same-pitch note endings.

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
    on: AtomicUsize,
    off: AtomicUsize,
    reset: AtomicUsize,
}

impl VoiceCalls {
    fn observed(&self) -> (usize, usize, usize) {
        (
            self.on.load(Ordering::Relaxed),
            self.off.load(Ordering::Relaxed),
            self.reset.load(Ordering::Relaxed),
        )
    }
}

struct VelocityProbe {
    native: crate::test_support::DelayProbeInstrument,
    scale: f32,
    calls: Arc<VoiceCalls>,
}

impl Instrument for VelocityProbe {
    fn instrument_kind(&self) -> InstrumentKind {
        self.native.instrument_kind()
    }
    fn param_descriptors(&self) -> &'static [ParamDescriptor] {
        self.native.param_descriptors()
    }
    fn set_param(&mut self, index: usize, value: f32) -> bool {
        self.native.set_param(index, value)
    }
    fn get_param(&self, index: usize) -> f32 {
        self.native.get_param(index)
    }
    fn note_on(&mut self, pitch: u8, velocity: u8) {
        self.calls.on.fetch_add(1, Ordering::Relaxed);
        self.scale = velocity as f32 / 32.0;
        self.native.note_on(pitch, velocity);
    }
    fn note_off(&mut self, pitch: u8) {
        self.calls.off.fetch_add(1, Ordering::Relaxed);
        self.native.note_off(pitch);
    }
    fn render(&mut self, output: &mut [f32], channels: usize) {
        self.native.render(output, channels);
        for value in output {
            *value *= self.scale;
        }
    }
    fn reset(&mut self) {
        self.calls.reset.fetch_add(1, Ordering::Relaxed);
        self.native.reset();
    }
}

pub(crate) struct ClipSnapshot {
    id: ClipId,
    geometry: (f64, f64, f64, bool, f64, f64, GrooveGrid),
    notes: Vec<MidiNote>,
    notes_owner: *const MidiNote,
    notes_capacity: usize,
    clamps: Vec<Option<f64>>,
    clamps_owner: *const Option<f64>,
    clamps_capacity: usize,
    latch: Option<(i64, SwingAmount)>,
}

impl ClipSnapshot {
    pub(crate) fn observe(clip: &EngineNoteClip) -> Self {
        Self {
            id: clip.id,
            geometry: (
                clip.position_beats,
                clip.duration_beats,
                clip.start_marker_beats,
                clip.loop_enabled,
                clip.loop_start_beats,
                clip.loop_end_beats,
                clip.groove_grid,
            ),
            notes: clip.notes.clone(),
            notes_owner: clip.notes.as_ptr(),
            notes_capacity: clip.notes.capacity(),
            clamps: clip.next_same_pitch_start_beats.clone(),
            clamps_owner: clip.next_same_pitch_start_beats.as_ptr(),
            clamps_capacity: clip.next_same_pitch_start_beats.capacity(),
            latch: clip.groove_latch.get(),
        }
    }

    pub(crate) fn assert_full(&self) {
        assert!(!self.notes.is_empty());
        assert_eq!(self.notes.len(), self.notes_capacity);
        assert_eq!(self.clamps.len(), self.clamps_capacity);
        assert_eq!(self.clamps, expected_clamps(&self.notes));
    }

    pub(crate) fn assert_appended(&self, clip: &EngineNoteClip, note: MidiNote) {
        self.assert_geometry(clip);
        let mut expected = self.notes.clone();
        expected.push(note);
        assert_eq!(clip.notes, expected);
        assert_eq!(clip.next_same_pitch_start_beats, expected_clamps(&expected));
        assert_eq!(clip.notes.len(), clip.next_same_pitch_start_beats.len());
        assert!(clip.notes.capacity() >= expected.len());
        assert!(clip.next_same_pitch_start_beats.capacity() >= expected.len());
        assert_ne!(clip.notes.as_ptr(), self.notes_owner);
        assert_ne!(clip.next_same_pitch_start_beats.as_ptr(), self.clamps_owner);
        assert_eq!(clip.groove_latch.get(), self.latch);
    }

    pub(crate) fn assert_retired_storage(&self, storage: &NoteInsertionStorage) {
        assert!(storage.notes.is_empty());
        assert!(storage.clamps.is_empty());
        assert_eq!(storage.notes.as_ptr(), self.notes_owner);
        assert_eq!(storage.notes.capacity(), self.notes_capacity);
        assert_eq!(storage.clamps.as_ptr(), self.clamps_owner);
        assert_eq!(storage.clamps.capacity(), self.clamps_capacity);
    }

    fn assert_geometry(&self, clip: &EngineNoteClip) {
        assert_eq!(clip.id, self.id);
        assert_eq!(
            (
                clip.position_beats,
                clip.duration_beats,
                clip.start_marker_beats,
                clip.loop_enabled,
                clip.loop_start_beats,
                clip.loop_end_beats,
                clip.groove_grid
            ),
            self.geometry
        );
    }

    pub(crate) fn assert_unchanged(&self, clip: &EngineNoteClip) {
        self.assert_geometry(clip);
        assert_eq!(clip.notes, self.notes);
        assert_eq!(clip.notes.as_ptr(), self.notes_owner);
        assert_eq!(clip.notes.capacity(), self.notes_capacity);
        assert_eq!(clip.next_same_pitch_start_beats, self.clamps);
        assert_eq!(clip.next_same_pitch_start_beats.as_ptr(), self.clamps_owner);
        assert_eq!(
            clip.next_same_pitch_start_beats.capacity(),
            self.clamps_capacity
        );
        assert_eq!(clip.groove_latch.get(), self.latch);
    }
}

pub(crate) struct EmptyNoteStorageSnapshot {
    notes_owner: *const MidiNote,
    notes_capacity: usize,
    clamps_owner: *const Option<f64>,
    clamps_capacity: usize,
}

impl EmptyNoteStorageSnapshot {
    pub(crate) fn observe(storage: &NoteInsertionStorage) -> Self {
        assert!(storage.notes.is_empty());
        assert!(storage.clamps.is_empty());
        Self {
            notes_owner: storage.notes.as_ptr(),
            notes_capacity: storage.notes.capacity(),
            clamps_owner: storage.clamps.as_ptr(),
            clamps_capacity: storage.clamps.capacity(),
        }
    }

    pub(crate) fn assert_installed_in(&self, clip: &EngineNoteClip) {
        assert_eq!(clip.notes.as_ptr(), self.notes_owner);
        assert_eq!(clip.notes.capacity(), self.notes_capacity);
        assert_eq!(clip.next_same_pitch_start_beats.as_ptr(), self.clamps_owner);
        assert_eq!(
            clip.next_same_pitch_start_beats.capacity(),
            self.clamps_capacity
        );
    }

    pub(crate) fn assert_unchanged(&self, storage: &NoteInsertionStorage) {
        assert!(storage.notes.is_empty());
        assert!(storage.clamps.is_empty());
        assert_eq!(storage.notes.as_ptr(), self.notes_owner);
        assert_eq!(storage.notes.capacity(), self.notes_capacity);
        assert_eq!(storage.clamps.as_ptr(), self.clamps_owner);
        assert_eq!(storage.clamps.capacity(), self.clamps_capacity);
    }
}

fn note(pitch: u8, velocity: u8, start_beat: f64, duration_beats: f64) -> MidiNote {
    MidiNote {
        pitch,
        velocity,
        start_beat,
        duration_beats,
    }
}

fn expected_clamps(notes: &[MidiNote]) -> Vec<Option<f64>> {
    notes
        .iter()
        .map(|note| {
            notes
                .iter()
                .filter_map(|candidate| {
                    (candidate.pitch == note.pitch && candidate.start_beat > note.start_beat)
                        .then_some(candidate.start_beat)
                })
                .min_by(f64::total_cmp)
        })
        .collect()
}

fn assert_signal(output: &[f32], value: f32) {
    for &[left, right] in output.as_chunks::<2>().0 {
        assert_eq!(left, value);
        assert_eq!(right, 0.0);
    }
}

#[test]
fn public_add_note_at_full_capacity_preserves_first_duplicate_voice_and_future_clamp_without_callback_heap_work(
) {
    let mut notes = Vec::with_capacity(4);
    notes.extend([
        note(60, 16, 2.0, 1.0),
        note(60, 32, 0.0, 4.0),
        note(60, 96, 2.0, 0.5),
        note(67, 64, 3.0, 1.0),
    ]);
    // Cross the actual nonzero allocation, including any allocator spare slots.
    while notes.len() < notes.capacity() {
        notes.push(note(72 + (notes.len() % 12) as u8, 32, 6.0, 0.125));
    }
    let count = notes.len();
    assert!(count >= 4);
    assert_eq!(count, notes.capacity());
    let duplicate = ClipId::new();
    let first = EngineNoteClip::new(
        duplicate,
        0.0,
        8.0,
        notes,
        0.0,
        false,
        0.0,
        8.0,
        GrooveGrid::Sixteenth,
    );
    assert_eq!(first.next_same_pitch_start_beats.len(), count);
    assert_eq!(first.next_same_pitch_start_beats.capacity(), count);
    assert_eq!(
        first.next_same_pitch_start_beats,
        expected_clamps(&first.notes)
    );
    let other = EngineNoteClip::new(
        duplicate,
        16.0,
        8.0,
        vec![note(72, 32, 0.5, 2.0)],
        0.25,
        true,
        0.25,
        4.0,
        GrooveGrid::Eighth,
    );
    let source = PreparedPlaybackSource::new(Vec::new(), vec![first, other], Vec::new());
    let track_id = TrackId::new();
    let unrelated = TrackId::new();
    let target_calls = Arc::new(VoiceCalls::default());
    let unrelated_calls = Arc::new(VoiceCalls::default());
    let mut target = EngineTrack::with_playback_source(track_id, source);
    target.pan = 0.0;
    target.instrument = Some(Box::new(VelocityProbe {
        native: crate::test_support::DelayProbeInstrument::new(0, 0.25),
        scale: 1.0,
        calls: Arc::clone(&target_calls),
    }));
    let mut held = EngineTrack::new(unrelated);
    held.pan = 0.0;
    held.instrument = Some(Box::new(VelocityProbe {
        native: crate::test_support::DelayProbeInstrument::new(0, 0.125),
        scale: 1.0,
        calls: Arc::clone(&unrelated_calls),
    }));
    let target_instrument = crate::engine::reconfiguration::instrument_owner_identity(
        target.instrument.as_deref().unwrap(),
    );
    let unrelated_instrument = crate::engine::reconfiguration::instrument_owner_identity(
        held.instrument.as_deref().unwrap(),
    );
    let (mut engine, mut commands, mut events) = AudioEngine::new();
    commands
        .push(EngineCommand::AddTrack(Box::new(target)))
        .unwrap();
    commands
        .push(EngineCommand::AddTrack(Box::new(held)))
        .unwrap();
    let channels: Vec<_> = [track_id, unrelated, TrackId::MASTER]
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
            crate::routing::PreparedRouting::prepare(&channels, 128).unwrap(),
        ))
        .unwrap();
    let swing = SwingAmount::new(0.62);
    commands
        .push(EngineCommand::SetProjectSwing(swing))
        .unwrap();
    commands
        .push(EngineCommand::ExternalNoteOn {
            track_id: unrelated,
            pitch: 72,
            velocity: 32,
        })
        .unwrap();
    commands.push(EngineCommand::Play).unwrap();
    let mut output = [0.0; 256];
    engine.process_block(AudioProcessBlock::new(&mut output, 2));
    assert_signal(&output, 0.375);
    while events.pop().is_ok() {}
    assert_eq!(engine.transport().position(), 128);
    assert!(engine.transport().is_playing());
    assert_eq!(engine.transport().bpm(), 120.0);
    assert_eq!(engine.transport().audio_length(), Some(24 * 22_050));
    assert_eq!(target_calls.observed(), (1, 0, 0));
    assert_eq!(unrelated_calls.observed(), (1, 0, 0));
    let source = &engine.tracks()[0].playback_source;
    let source_owner = std::ptr::from_ref(source.as_ref());
    let clips_owner = source.note_clips.as_ptr();
    let clips_capacity = source.note_clips.capacity();
    assert_eq!(source.note_clips.len(), 2);
    let first = ClipSnapshot::observe(&source.note_clips[0]);
    let other = ClipSnapshot::observe(&source.note_clips[1]);
    assert_eq!(first.latch, Some((0, swing)));
    assert_eq!(other.latch, None);
    assert_eq!(first.notes.len(), first.notes_capacity);
    assert_eq!(first.clamps.len(), first.clamps_capacity);
    let inserted = note(60, 64, 1.0, 0.5);
    let mut expected = first.notes.clone();
    expected.push(inserted);
    let clamps = expected_clamps(&expected);
    assert_eq!(clamps[0], None);
    assert_eq!(clamps[1], Some(1.0));
    assert_eq!(clamps[2], None);
    assert_eq!(clamps[3], None);
    assert_eq!(clamps[count], Some(2.0));
    let insertion = crate::clip_insertion::PreparedNoteInsertion::new(inserted);
    let operation = insertion.id();
    commands
        .push(EngineCommand::AddNote {
            track_id,
            clip_id: duplicate,
            insertion,
        })
        .unwrap();
    let mut heap = crate::retirement::tests::allocations(|| {
        engine.process_block(AudioProcessBlock::new(&mut [], 2));
    });
    let mut request = None;
    while let Ok(event) = events.pop() {
        if let crate::events::EngineEvent::ClipInsertionStorageRequested(value) = event {
            assert_eq!(value.id, operation);
            assert!(request.replace(value).is_none());
        }
    }
    let waiting = crate::retirement::tests::allocations(|| {
        engine.process_block(AudioProcessBlock::new(&mut [], 2));
    });
    heap.0 += waiting.0;
    heap.1 += waiting.1;
    request
        .expect("the original full note/cache fixture must request real storage")
        .fulfill();
    let resumed = crate::retirement::tests::allocations(|| {
        engine.process_block(AudioProcessBlock::new(&mut [], 2));
    });
    heap.0 += resumed.0;
    heap.1 += resumed.1;
    let mut completed = 0;
    while let Ok(event) = events.pop() {
        if let crate::events::EngineEvent::ClipInsertionCompleted(value) = event {
            assert_eq!(value.id(), operation);
            assert_eq!(
                value.outcome,
                crate::clip_insertion::ClipInsertionOutcome::Applied
            );
            assert_eq!(
                (value.track_id, value.target),
                (
                    track_id,
                    crate::clip_insertion::ClipInsertionTarget::Clip(duplicate)
                )
            );
            completed += 1;
        }
    }
    assert_eq!(completed, 1);
    let inserted_render = crate::retirement::tests::allocations(|| {
        engine.process_block(AudioProcessBlock::new(&mut output[..16], 2));
    });
    heap.0 += inserted_render.0;
    heap.1 += inserted_render.1;
    assert_signal(&output[..16], 0.375);
    assert_eq!(target_calls.observed(), (1, 0, 0));
    assert_eq!(unrelated_calls.observed(), (1, 0, 0));
    let source = &engine.tracks()[0].playback_source;
    assert_eq!(std::ptr::from_ref(source.as_ref()), source_owner);
    assert_eq!(source.note_clips.as_ptr(), clips_owner);
    assert_eq!(source.note_clips.capacity(), clips_capacity);
    assert_eq!(source.note_clips.len(), 2);
    first.assert_geometry(&source.note_clips[0]);
    assert_eq!(source.note_clips[0].notes, expected);
    assert_eq!(source.note_clips[0].next_same_pitch_start_beats, clamps);
    assert_eq!(source.note_clips[0].groove_latch.get(), first.latch);
    other.assert_unchanged(&source.note_clips[1]);
    assert_eq!(
        crate::engine::reconfiguration::instrument_owner_identity(
            engine.tracks()[0].instrument.as_deref().unwrap()
        ),
        target_instrument
    );
    assert_eq!(
        crate::engine::reconfiguration::instrument_owner_identity(
            engine.tracks()[1].instrument.as_deref().unwrap()
        ),
        unrelated_instrument
    );
    assert_eq!(engine.transport().position(), 136);
    assert!(engine.transport().is_playing());
    assert_eq!(engine.transport().audio_length(), Some(24 * 22_050));
    assert_eq!(
        commands.slots(),
        vibez_core::constants::RING_BUFFER_CAPACITY
    );
    while events.pop().is_ok() {}
    // These pair-aligned edges stay at beats1 and1.5 under Sixteenth Swing.
    // An old clamp, a missing new note or a flushed unrelated voice changes audio.
    let note_on = 22_050;
    let note_off = 33_075;
    let end = note_off + 16;
    let partitions = [1, 7, 31, 128];
    let mut block = 0;
    while engine.transport().position() < end {
        let start = engine.transport().position();
        let frames = partitions[block % partitions.len()].min((end - start) as usize);
        let measured = crate::retirement::tests::allocations(|| {
            engine.process_block(AudioProcessBlock::new(&mut output[..frames * 2], 2));
        });
        heap.0 += measured.0;
        heap.1 += measured.1;
        for (frame, &[left, right]) in output[..frames * 2].as_chunks::<2>().0.iter().enumerate() {
            let position = start + frame as u64;
            let expected = if position < note_on {
                0.375
            } else if position < note_off {
                0.625
            } else {
                0.125
            };
            assert_eq!(left, expected, "future note boundary at sample {position}");
            assert_eq!(right, 0.0);
        }
        while events.pop().is_ok() {}
        block += 1;
    }
    assert_eq!(target_calls.observed(), (2, 2, 0));
    assert_eq!(unrelated_calls.observed(), (1, 0, 0));
    assert_eq!(engine.transport().position(), end);
    assert!(engine.transport().is_playing());
    assert_eq!(engine.transport().bpm(), 120.0);
    assert_eq!(engine.transport().audio_length(), Some(24 * 22_050));
    assert_eq!(
        engine.tracks()[0].playback_source.note_clips[0].notes,
        expected
    );
    assert_eq!(
        engine.tracks()[0].playback_source.note_clips[0].next_same_pitch_start_beats,
        clamps
    );
    other.assert_unchanged(&engine.tracks()[0].playback_source.note_clips[1]);
    commands
        .push(EngineCommand::ExternalNoteOff {
            track_id: unrelated,
            pitch: 72,
        })
        .unwrap();
    let measured = crate::retirement::tests::allocations(|| {
        engine.process_block(AudioProcessBlock::new(&mut output[..16], 2));
    });
    heap.0 += measured.0;
    heap.1 += measured.1;
    assert_signal(&output[..16], 0.0);
    assert_eq!(unrelated_calls.observed(), (1, 1, 0));
    assert_eq!(target_calls.observed(), (2, 2, 0));
    assert_eq!(
        heap,
        (0, 0),
        "AddNote complete callback allocation/destruction counts"
    );
}

#[path = "playback_source_note_value_fifo_tests.rs"]
mod fifo_tests;

#[path = "playback_source_note_value_consumer_tests.rs"]
mod consumer_tests;

#[path = "playback_source_note_value_construction_tests.rs"]
pub(crate) mod construction_tests;
