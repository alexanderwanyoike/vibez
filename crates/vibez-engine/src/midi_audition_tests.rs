//! A Browser MIDI audition plays its notes on their exact samples, once or looped.

use super::*;
use vibez_core::note_output::BlockNote;

/// One beat per 100 samples keeps the expected frames readable.
const SAMPLES_PER_BEAT: f64 = 100.0;

fn note(pitch: u8, start_beat: f64, duration_beats: f64) -> MidiNote {
    MidiNote {
        pitch,
        velocity: 100,
        start_beat,
        duration_beats,
    }
}

fn collected(
    pattern: &MidiAuditionPattern,
    pos: u64,
    frames: usize,
    looped: bool,
) -> Vec<BlockNote> {
    let mut out = NoteOutput::default();
    pattern.collect(pos, frames, SAMPLES_PER_BEAT, looped, &mut out);
    out.notes().to_vec()
}

#[test]
fn notes_play_on_the_samples_their_beats_fall_on() {
    let pattern = MidiAuditionPattern::new(vec![note(36, 0.0, 1.0), note(38, 2.5, 0.5)], 4.0);

    assert_eq!(
        collected(&pattern, 0, 256, false),
        [
            BlockNote::off(100, 36),
            BlockNote::on(0, 36, 100),
            BlockNote::on(250, 38, 100),
        ]
    );
    assert_eq!(
        collected(&pattern, 256, 256, false),
        [BlockNote::off(44, 38)]
    );
}

#[test]
fn a_looped_pattern_starts_again_after_its_length() {
    let pattern = MidiAuditionPattern::new(vec![note(36, 0.0, 1.0)], 4.0);

    assert_eq!(
        collected(&pattern, 390, 20, true),
        [BlockNote::on(10, 36, 100)]
    );
    assert_eq!(
        collected(&pattern, 1_190, 20, true),
        [BlockNote::on(10, 36, 100)]
    );
}

#[test]
fn a_pattern_played_once_stays_silent_after_its_length() {
    let pattern = MidiAuditionPattern::new(vec![note(36, 0.0, 1.0)], 4.0);

    assert!(collected(&pattern, 390, 20, false).is_empty());
}

#[test]
fn a_note_held_past_the_length_ends_there_before_the_next_pass_starts_it() {
    let pattern = MidiAuditionPattern::new(vec![note(36, 0.0, 1.0), note(40, 3.0, 2.0)], 4.0);

    assert_eq!(
        collected(&pattern, 390, 20, true),
        [BlockNote::off(10, 40), BlockNote::on(10, 36, 100)]
    );
    assert_eq!(
        collected(&pattern, 390, 20, false),
        [BlockNote::off(10, 40)]
    );
}

#[test]
fn a_note_starting_at_or_after_the_length_never_plays() {
    let pattern = MidiAuditionPattern::new(vec![note(36, 4.0, 1.0)], 4.0);

    assert!(collected(&pattern, 0, 1_000, true).is_empty());
}

#[test]
fn a_block_spanning_several_passes_plays_each_of_them() {
    let pattern = MidiAuditionPattern::new(vec![note(36, 0.0, 0.5)], 1.0);

    assert_eq!(
        collected(&pattern, 0, 250, true),
        [
            BlockNote::off(50, 36),
            BlockNote::off(150, 36),
            BlockNote::on(0, 36, 100),
            BlockNote::on(100, 36, 100),
            BlockNote::on(200, 36, 100),
        ]
    );
}

#[test]
fn played_once_the_audition_ends_after_its_length() {
    let pattern = MidiAuditionPattern::new(vec![note(36, 0.0, 1.0)], 4.0);

    assert!(!pattern.ended(400, SAMPLES_PER_BEAT, false));
    assert!(pattern.ended(401, SAMPLES_PER_BEAT, false));
    assert!(!pattern.ended(4_000, SAMPLES_PER_BEAT, true));
}

#[test]
fn a_note_shorter_than_a_sample_ends_after_it_starts() {
    let pattern = MidiAuditionPattern::new(vec![note(36, 0.005, 0.0005)], 4.0);

    assert_eq!(
        collected(&pattern, 0, 8, false),
        [BlockNote::on(1, 36, 100), BlockNote::off(1, 36)]
    );
}
