//! A public native consumer serves stale note-storage hints without losing MIDI edits or FIFO progress.

use vibez_core::effect::ParamDescriptor;
use vibez_core::id::{ClipId, TrackId};
use vibez_core::midi::{InstrumentKind, MidiNote};
use vibez_core::perform::GrooveGrid;
use vibez_core::routing::RoutingChannel;
use vibez_engine::clip_insertion::{
    ClipInsertionId, ClipInsertionOutcome, PreparedNoteClipInsertion, PreparedNoteInsertion,
};
use vibez_engine::commands::EngineCommand;
use vibez_engine::engine::{AudioEngine, AudioProcessBlock};
use vibez_engine::events::EngineEvent;
use vibez_engine::mixer::EngineTrack;
use vibez_engine::playback_source::{EngineNoteClip, PreparedPlaybackSource};
use vibez_engine::routing::PreparedRouting;
use vibez_instruments::Instrument;

#[derive(Default)]
struct VelocityInstrument(f32);

impl Instrument for VelocityInstrument {
    fn instrument_kind(&self) -> InstrumentKind {
        InstrumentKind::SubtractiveSynth
    }
    fn param_descriptors(&self) -> &'static [ParamDescriptor] {
        &[]
    }
    fn set_param(&mut self, _: usize, _: f32) -> bool {
        false
    }
    fn get_param(&self, _: usize) -> f32 {
        0.0
    }
    fn note_on(&mut self, _: u8, velocity: u8) {
        self.0 = velocity as f32 / 128.0;
    }
    fn note_off(&mut self, _: u8) {
        self.0 = 0.0;
    }
    fn render(&mut self, output: &mut [f32], _: usize) {
        output.fill(self.0);
    }
    fn reset(&mut self) {
        self.0 = 0.0;
    }
}

fn render(engine: &mut AudioEngine, sounding: bool) -> [f32; 16] {
    let mut output = [0.0; 16];
    let length = if sounding { output.len() } else { 0 };
    engine.process_block(AudioProcessBlock::new(&mut output[..length], 2));
    output
}

fn assert_signal(output: &[f32], expected: f32) {
    for &[left, right] in output.as_chunks::<2>().0 {
        assert_eq!(left, expected);
        assert_eq!(right, 0.0);
    }
}

struct Expected {
    id: ClipId,
    operation: ClipInsertionId,
    note_operation: ClipInsertionId,
    position: f64,
    duration: f64,
    note: MidiNote,
}

#[test]
fn native_note_consumer_fulfills_stale_hints_and_preserves_first_duplicate_note_target() {
    let (mut engine, mut commands, mut events) = AudioEngine::new();
    let track_id = TrackId::new();
    let duplicate = ClipId::new();
    let resident_note = MidiNote {
        pitch: 60,
        velocity: 32,
        start_beat: 0.0,
        duration_beats: 4.0,
    };
    let source = PreparedPlaybackSource::new(
        Vec::new(),
        vec![EngineNoteClip::new(
            duplicate,
            0.0,
            8.0,
            vec![resident_note],
            0.0,
            false,
            0.0,
            8.0,
            GrooveGrid::Off,
        )],
        Vec::new(),
    );
    let resident_notes = source.note_clips[0].notes.as_ptr();
    assert_eq!(
        (source.note_clips.len(), source.note_clips.capacity()),
        (1, 1)
    );
    let mut track = EngineTrack::with_playback_source(track_id, source);
    track.pan = 0.0;
    track.instrument = Some(Box::new(VelocityInstrument::default()));
    commands
        .push(EngineCommand::AddTrack(Box::new(track)))
        .unwrap();
    let channels: Vec<_> = [track_id, TrackId::MASTER]
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
            PreparedRouting::prepare(&channels, 8).unwrap(),
        ))
        .unwrap();
    commands.push(EngineCommand::Play).unwrap();
    assert_signal(&render(&mut engine, true), 0.25);
    while events.pop().is_ok() {}
    let mut expected = Vec::new();
    for index in 0..257 {
        let id = if index == 0 { duplicate } else { ClipId::new() };
        let position = 16.0 + index as f64 * 4.0;
        let duration = 2.0 + (index % 2) as f64;
        let insertion = PreparedNoteClipInsertion::new(
            id,
            position,
            duration,
            0.0,
            false,
            0.0,
            duration,
            GrooveGrid::Off,
        );
        let operation = insertion.id();
        commands
            .push(EngineCommand::AddNoteClip {
                track_id,
                insertion,
            })
            .unwrap();
        let note = MidiNote {
            pitch: 61 + (index % 60) as u8,
            velocity: 1 + (index % 127) as u8,
            start_beat: if index == 0 { 2.0 } else { 0.0 },
            duration_beats: 1.0,
        };
        // The separate existing AddNote command must stay behind its own empty
        // insertion; duplicate IDs continue resolving to the first resident clip.
        let insertion = PreparedNoteInsertion::new(note);
        let note_operation = insertion.id();
        commands
            .push(EngineCommand::AddNote {
                track_id,
                clip_id: id,
                insertion,
            })
            .unwrap();
        expected.push(Expected {
            id,
            operation,
            note_operation,
            position,
            duration,
            note,
        });
    }
    commands.push(EngineCommand::SetBpm(137.0)).unwrap();
    commands.push(EngineCommand::Stop).unwrap();
    render(&mut engine, false);
    assert!(
        matches!(events.peek().unwrap(), EngineEvent::ClipInsertionStorageRequested(request)
        if request.id == expected[0].operation)
    );
    assert_signal(&render(&mut engine, true), 0.25);
    assert_eq!(engine.transport().position(), 16);
    assert!(engine.transport().is_playing());
    assert_eq!(engine.transport().bpm(), 120.0);
    assert_eq!(engine.tracks()[0].playback_source.note_clips.len(), 1);
    assert_eq!(
        engine.tracks()[0].playback_source.note_clips[0].notes,
        [resident_note]
    );
    assert_eq!(
        engine.tracks()[0].playback_source.note_clips[0]
            .notes
            .as_ptr(),
        resident_notes
    );
    let mut completed = 0;
    let mut notes_completed = 0;
    let mut requested = Vec::new();
    // Each outer clip and its following note can request one exchange.
    // Servicing both actual operations preserves the original public FIFO.
    for _ in 0..=expected.len() * 2 {
        render(&mut engine, false);
        while let Ok(event) = events.pop() {
            match event {
                EngineEvent::ClipInsertionStorageRequested(request) => {
                    let item = expected
                        .iter()
                        .find(|item| {
                            item.operation == request.id || item.note_operation == request.id
                        })
                        .unwrap();
                    assert_eq!((request.track_id, request.target), (track_id, vibez_engine::clip_insertion::ClipInsertionTarget::Clip(item.id)));
                    assert!(!requested.contains(&request.id));
                    requested.push(request.id.clone());
                    request.fulfill();
                }
                EngineEvent::ClipInsertionCompleted(value) => {
                    let item = if completed == notes_completed {
                        let item = &expected[completed];
                        assert_eq!(value.id(), item.operation);
                        completed += 1;
                        item
                    } else {
                        assert_eq!(completed, notes_completed + 1);
                        let item = &expected[notes_completed];
                        assert_eq!(value.id(), item.note_operation);
                        notes_completed += 1;
                        item
                    };
                    assert_eq!((value.track_id, value.target), (track_id, vibez_engine::clip_insertion::ClipInsertionTarget::Clip(item.id)));
                    assert_eq!(value.outcome, ClipInsertionOutcome::Applied);
                    drop(value);
                }
                _ => {}
            }
        }
        if completed == expected.len() && notes_completed == expected.len() {
            break;
        }
    }
    assert_eq!(completed, 257);
    assert_eq!(notes_completed, 257);
    assert!(!requested.is_empty());
    assert_eq!(
        commands.slots(),
        vibez_core::constants::RING_BUFFER_CAPACITY
    );
    assert!(!engine.transport().is_playing());
    assert_eq!(engine.transport().position(), 16);
    assert_eq!(engine.transport().bpm(), 137.0);
    let clips = &engine.tracks()[0].playback_source.note_clips;
    assert_eq!(clips.len(), 258);
    assert_eq!(clips[0].id, duplicate);
    assert_eq!(clips[0].notes, [resident_note, expected[0].note]);
    for (actual, item) in clips[1..].iter().zip(&expected) {
        assert_eq!(actual.id, item.id);
        assert_eq!(actual.position_beats, item.position);
        assert_eq!(actual.duration_beats, item.duration);
        assert_eq!(actual.start_marker_beats, 0.0);
        assert!(!actual.loop_enabled);
        assert_eq!(actual.loop_start_beats, 0.0);
        assert_eq!(actual.loop_end_beats, item.duration);
        assert_eq!(actual.groove_grid, GrooveGrid::Off);
        if item.id == duplicate {
            assert!(actual.notes.is_empty());
        } else {
            assert_eq!(actual.notes, [item.note]);
        }
    }
    let last = expected.last().unwrap();
    let samples_per_beat = 44_100.0 * 60.0 / 137.0;
    assert_eq!(
        engine.transport().audio_length(),
        Some(((last.position + last.duration) * samples_per_beat).round() as u64)
    );
    for item in &expected[1..] {
        // The scheduler maps note-on beats to their first reaching sample with ceil.
        commands
            .push(EngineCommand::Seek(
                (item.position * samples_per_beat).ceil() as u64,
            ))
            .unwrap();
        commands.push(EngineCommand::Play).unwrap();
        assert_signal(
            &render(&mut engine, true),
            item.note.velocity as f32 / 128.0,
        );
        while events.pop().is_ok() {}
    }
}
