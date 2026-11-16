//! Clip notes on a track yield only the pitches a live key took over.

use std::sync::Arc;

use vibez_core::id::{ClipId, TrackId};
use vibez_core::midi::MidiNote;
use vibez_core::perform::{GrooveGrid, SwingAmount};
use vibez_core::time::TempoMap;

use super::{EngineNoteClip, EngineTrack, InstrumentRenderContext};
use crate::test_support::{NoteLog, NoteLogProbe};

const PITCH: u8 = 60;
const BLOCK: usize = 64;

fn note(start_beat: f64, duration_beats: f64) -> MidiNote {
    MidiNote {
        pitch: PITCH,
        velocity: 100,
        start_beat,
        duration_beats,
    }
}

/// A track playing one four-beat Clip at 100 samples per beat.
fn track_playing(notes: Vec<MidiNote>, log: &NoteLog, batch: bool) -> EngineTrack {
    let mut track = EngineTrack::new(TrackId::new());
    track.instrument = Some(Box::new(NoteLogProbe {
        log: Arc::clone(log),
        batch,
    }));
    track.playback_source.push_note_clip(EngineNoteClip::new(
        ClipId::new(),
        0.0,
        4.0,
        notes,
        0.0,
        false,
        0.0,
        4.0,
        GrooveGrid::Off,
    ));
    track
}

fn render(track: &mut EngineTrack, from: u64, to: u64) {
    let tempo_map = TempoMap::new(60.0, 100);
    for pos in (from..to).step_by(BLOCK) {
        track.render_instrument(
            InstrumentRenderContext {
                pos,
                repeat_pos: pos,
                frames: BLOCK,
                channels: 1,
                tempo_map: &tempo_map,
                project_swing: SwingAmount::STRAIGHT,
            },
            &mut |_| {},
        );
    }
}

fn heard(log: &NoteLog) -> Vec<(u64, u8, bool)> {
    log.lock().unwrap().1.clone()
}

#[test]
fn overlapping_notes_of_one_pitch_play_every_event_without_a_live_key() {
    for batch in [false, true] {
        let log = NoteLog::default();
        let mut track = track_playing(vec![note(0.0, 2.0), note(1.0, 2.0)], &log, batch);

        render(&mut track, 0, 512);

        assert_eq!(
            heard(&log),
            [
                (0, PITCH, true),
                (100, PITCH, true),
                (200, PITCH, false),
                (300, PITCH, false),
                (400, PITCH, false),
                (400, PITCH, false),
            ],
            "batch {batch}"
        );
    }
}

#[test]
fn a_new_instrument_lets_the_clip_play_a_pitch_a_held_key_took_over() {
    for batch in [false, true] {
        let log = NoteLog::default();
        let mut track = track_playing(vec![note(1.0, 1.0)], &log, batch);
        track.hold_live_key(PITCH);

        track.replace_instrument(Box::new(NoteLogProbe {
            log: Arc::clone(&log),
            batch,
        }));
        render(&mut track, 0, 512);

        assert_eq!(
            heard(&log),
            [(100, PITCH, true), (200, PITCH, false), (400, PITCH, false)],
            "batch {batch}"
        );
    }
}

#[test]
fn a_held_key_keeps_its_pitch_while_its_instrument_is_reconfigured() {
    for batch in [false, true] {
        let log = NoteLog::default();
        let mut track = track_playing(vec![note(1.0, 1.0)], &log, batch);
        track.hold_live_key(PITCH);

        // Device reconfiguration takes the instrument and restores it.
        let instrument = track.take_instrument();
        track.instrument = instrument;
        render(&mut track, 0, 512);

        assert_eq!(heard(&log), [], "batch {batch}");
    }
}
