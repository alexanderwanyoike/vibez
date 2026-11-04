//! A looped clip plays its loop whatever the clip's own length.

use vibez_core::id::ClipId;
use vibez_core::midi::MidiNote;
use vibez_core::perform::{GrooveGrid, SwingAmount};

use super::{collect_timed_note_events, EngineNoteClip};

const SAMPLES_PER_BEAT: f64 = 100.0;

fn note(pitch: u8, start_beat: f64) -> MidiNote {
    MidiNote {
        pitch,
        velocity: 100,
        start_beat,
        duration_beats: 0.5,
    }
}

fn note_ons(clip: EngineNoteClip, frames: usize) -> Vec<(u32, u8)> {
    let mut ons = Vec::new();
    let mut offs = Vec::new();
    collect_timed_note_events(
        &[clip],
        0,
        frames,
        SAMPLES_PER_BEAT,
        SwingAmount::STRAIGHT,
        &mut ons,
        &mut offs,
    );
    ons.into_iter()
        .map(|(frame, pitch, _)| (frame, pitch))
        .collect()
}

#[test]
fn a_midi_clip_shorter_than_its_loop_plays_the_loop_from_its_start_marker() {
    let clip = EngineNoteClip::new(
        ClipId::new(),
        0.0,
        3.0,
        vec![note(60, 0.0), note(62, 3.0)],
        2.0,
        true,
        0.0,
        4.0,
        GrooveGrid::Off,
    );

    assert_eq!(note_ons(clip, 600), [(100, 62), (200, 60)]);
}

#[test]
fn a_midi_clip_longer_than_its_loop_repeats_the_loop() {
    let clip = EngineNoteClip::new(
        ClipId::new(),
        0.0,
        8.0,
        vec![note(60, 0.0), note(62, 3.0)],
        0.0,
        true,
        0.0,
        2.0,
        GrooveGrid::Off,
    );

    assert_eq!(
        note_ons(clip, 1_000),
        [(0, 60), (200, 60), (400, 60), (600, 60)]
    );
}
