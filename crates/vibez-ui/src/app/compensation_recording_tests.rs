use super::*;
use crate::domains::perform::{
    capture::CapturePhase,
    clip_record::{empty_midi_clip, ClipRecordSession},
    loop_record::LoopRecordQuantization,
};
use vibez_core::id::{ClipId, SectionId, TrackId};
use vibez_engine::events::SourceRecordingPosition;

#[test]
fn source_take_first_note_survives_count_in_while_capture_uses_the_heard_coordinate() {
    let mut state = crate::state::AppState::default();
    let track = TrackId::new();
    let section = SectionId::new();
    let heard_section = SectionId::new();
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
    state.perform.capture.start(600, None);
    for (on, raw, local) in [(true, 1024, 0), (false, 1040, 16)] {
        apply_performed_note(
            &mut state.perform,
            EngineEvent::SourceNoteInput {
                track_id: track,
                pitch: 60,
                velocity: if on { 100 } else { 0 },
                on,
                position: SourceRecordingPosition {
                    effective_at_samples: raw,
                    canonical_at_samples: raw,
                    section_id: Some(section),
                    section_position_samples: Some(local),
                    canonical_section_position_samples: Some(local),
                },
            },
        );
    }
    let clip_take = state
        .perform
        .clip_record
        .notes
        .finish(clip, track, 1100, 76, true)
        .unwrap();
    let section_take = state
        .perform
        .section_record
        .finish(section, track, 1100, 76, true)
        .unwrap();
    for take in [&clip_take.notes, &section_take.notes] {
        assert_eq!(take.len(), 1);
        assert_eq!(take[0].start_beat, 0.0);
        assert_eq!(take[0].duration_beats, 0.125);
    }
    for (on, _raw, heard, local) in [(true, 1024, 640, 128), (false, 1040, 656, 144)] {
        apply_performed_note(
            &mut state.perform,
            EngineEvent::InstrumentNoteInput {
                track_id: track,
                pitch: 60,
                velocity: if on { 100 } else { 0 },
                on,
                effective_at_samples: heard,
                section_id: Some(heard_section),
                section_position_samples: Some(local),
            },
        );
    }
    let result = state.perform.capture.finish(700).unwrap().materialize();
    let clip = &result.by_track[&track].note_clips[0];
    let note = &clip.notes[0];
    assert_eq!(clip.position_beats + note.start_beat, 40.0 / 128.0);
    assert_eq!(note.duration_beats, 0.125);
}
