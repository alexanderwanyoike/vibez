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

fn resident_clip(index: usize, id: ClipId) -> EngineNoteClip {
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
    commands.push(EngineCommand::AddNoteClip {
        track_id,
        clip_id: duplicate,
        position_beats: position,
        duration_beats: duration,
        start_marker_beats: 0.75,
        loop_enabled: true,
        loop_start_beats: 0.25,
        loop_end_beats: 2.5,
        groove_grid: GrooveGrid::Eighth,
    }).unwrap();
    let mut output = [0.0; 16];
    let measured = crate::retirement::tests::allocations(|| {
        engine.process_block(AudioProcessBlock::new(&mut output, 2));
    });
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
