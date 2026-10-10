//! Source takes and Arrange Capture consume their own observations once.

use super::*;
use crate::domains::perform::{
    capture::CapturePhase,
    clip_record::{empty_midi_clip, ClipRecordSession},
    loop_record::LoopRecordQuantization,
};
use vibez_core::id::{ClipId, SectionId, TrackId};
use vibez_core::perform::NoteRepeatRate;
use vibez_engine::events::SourceRecordingPosition;

#[test]
fn source_observations_preserve_clip_section_and_arrange_takes_without_duplicates() {
    let mut state = crate::state::AppState::default();
    let track = TrackId::new();
    let section = SectionId::new();
    let clip = ClipId::new();
    state.perform.clip_record.notes.quantization = LoopRecordQuantization::Off;
    state.perform.clip_record.begin_session(
        ClipRecordSession {
            original: None,
            working: empty_midi_clip(clip, track, 0, "Take".into(), 4.0),
            audio: false,
            length_samples: Some(512),
            start: Some(1024),
            output_start: Some(1024),
            pass: 0,
            stop: None,
            sample_rate: 128,
            bpm: 60.0,
        },
        true,
    );
    state.perform.clip_record.notes.start(clip, track, 1024, 0);
    state.perform.section_record.quantization = LoopRecordQuantization::Off;
    state.perform.section_record.sync_clock(true, 60.0, 128);
    assert!(state
        .perform
        .section_record
        .request_start(section, track, false, 4.0)
        .is_some());
    state.perform.section_record.start(section, track, 1024, 0);
    state.perform.capture.phase = CapturePhase::Starting;
    state.perform.capture.prepare(0, 128, 60.0);
    state
        .perform
        .capture
        .prepare_controlled_tracks([(track, false)]);
    state.perform.capture.start(1024, None);
    for (on, at, local) in [(true, 1024, 0), (false, 1040, 16)] {
        apply_performed_note(
            &mut state.perform,
            EngineEvent::SourceNoteInput {
                track_id: track,
                pitch: 60,
                velocity: if on { 100 } else { 0 },
                on,
                position: SourceRecordingPosition {
                    effective_at_samples: at,
                    canonical_at_samples: at,
                    section_id: Some(section),
                    section_position_samples: Some(local),
                    canonical_section_position_samples: Some(local),
                },
            },
        );
        apply_performed_note(
            &mut state.perform,
            EngineEvent::InstrumentNoteInput {
                track_id: track,
                pitch: 60,
                velocity: if on { 100 } else { 0 },
                on,
                effective_at_samples: at,
                section_id: Some(section),
                section_position_samples: Some(local),
            },
        );
    }
    apply_performed_note(
        &mut state.perform,
        EngineEvent::SourceNoteRepeated {
            track_id: track,
            pitch: 62,
            velocity: 100,
            rate: NoteRepeatRate::Eighth,
            position: SourceRecordingPosition {
                effective_at_samples: 1096,
                canonical_at_samples: 1088,
                section_id: Some(section),
                section_position_samples: Some(72),
                canonical_section_position_samples: Some(64),
            },
        },
    );
    apply_performed_note(
        &mut state.perform,
        EngineEvent::NoteRepeated {
            track_id: track,
            pitch: 62,
            velocity: 100,
            rate: NoteRepeatRate::Eighth,
            effective_at_samples: 1096,
            canonical_at_samples: 1088,
            section_id: Some(section),
            section_position_samples: Some(72),
            canonical_section_position_samples: Some(64),
        },
    );
    let clip_take = state
        .perform
        .clip_record
        .notes
        .finish(clip, track, 1200, 176, true)
        .unwrap();
    let section_take = state
        .perform
        .section_record
        .finish(section, track, 1200, 176, true)
        .unwrap();
    for notes in [&clip_take.notes, &section_take.notes] {
        assert_eq!(notes.len(), 2);
        assert_eq!(notes[0].pitch, 60);
        assert_eq!(notes[0].start_beat, 0.0);
        assert_eq!(notes[0].duration_beats, 0.125);
        assert_eq!(notes[1].pitch, 62);
        assert_eq!(notes[1].start_beat, 0.5);
        assert_eq!(notes[1].duration_beats, 0.5);
    }
    let capture = state.perform.capture.finish(1200).unwrap().materialize();
    let mut notes: Vec<_> = capture.by_track[&track]
        .note_clips
        .iter()
        .flat_map(|clip| {
            clip.notes.iter().map(|note| {
                (
                    note.pitch,
                    clip.position_beats + note.start_beat,
                    note.duration_beats,
                )
            })
        })
        .collect();
    notes.sort_by_key(|note| note.0);
    assert_eq!(notes, [(60, 0.0, 0.125), (62, 0.5, 0.5)]);
}
