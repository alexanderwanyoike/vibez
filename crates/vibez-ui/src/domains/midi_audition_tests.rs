//! A selected MIDI file previews as the Clip a drop lands, plays on the selected MIDI track's instrument, and lays out a strip with bar lines.

use super::*;
use std::path::PathBuf;
use vibez_core::midi::{InstrumentKind, MidiNote, TrackKind};
use vibez_core::midi_file::{MidiFile, MidiFileTrack};

use crate::domains::midi_file_import::MidiFileLayout;

fn hit(pitch: u8, start_beat: f64, duration_beats: f64) -> MidiNote {
    MidiNote {
        pitch,
        velocity: 100,
        start_beat,
        duration_beats,
    }
}

fn source() -> MediaSourceRef {
    MediaSourceRef::LocalFile {
        path: PathBuf::from("/samples/bass.mid"),
    }
}

fn import(tracks: Vec<Vec<MidiNote>>, length_beats: f64) -> MidiFileImport {
    MidiFileImport {
        name: "bass".into(),
        file: MidiFile {
            tracks: tracks
                .into_iter()
                .map(|notes| MidiFileTrack {
                    name: None,
                    notes,
                    on_drum_channel: false,
                })
                .collect(),
            length_beats,
        },
    }
}

fn preview(notes: Vec<MidiNote>, length_beats: f64) -> MidiPreview {
    MidiPreview::of(source(), &import(vec![notes], length_beats))
}

#[test]
fn a_preview_plays_every_track_of_the_file_over_the_clip_a_drop_lands() {
    let file = import(vec![vec![hit(36, 1.0, 0.5)], vec![hit(48, 0.0, 1.0)]], 8.0);
    let landed = &file.plan(None, 0.0, MidiFileLayout::OneClip)[0].clip;

    let preview = MidiPreview::of(source(), &file);

    assert_eq!(preview.source, source());
    assert_eq!(preview.pattern.notes(), landed.notes.as_slice());
    assert_eq!(preview.pattern.length_beats(), landed.duration_beats);
}

#[test]
fn the_strip_spans_the_clip_with_a_line_on_every_bar() {
    let strip = preview(vec![hit(36, 0.0, 1.0)], 16.0).strip();

    assert_eq!(strip.bar_lines, [0.25, 0.5, 0.75]);
}

#[test]
fn strip_notes_sit_across_the_clip_where_they_play() {
    let strip = preview(vec![hit(36, 4.0, 2.0)], 16.0).strip();

    assert_eq!(strip.notes.len(), 1);
    assert_eq!((strip.notes[0].start, strip.notes[0].end), (0.25, 0.375));
}

#[test]
fn higher_notes_sit_higher_and_a_narrow_range_keeps_an_octave_of_rows() {
    let strip = preview(vec![hit(36, 0.0, 1.0), hit(43, 1.0, 1.0)], 4.0).strip();

    assert_eq!(strip.row_height, 1.0 / 12.0);
    let (low, high) = (strip.notes[0], strip.notes[1]);
    assert!(high.top < low.top);
    assert!((low.top - high.top - 7.0 / 12.0).abs() < 1e-6);
    assert!(high.top >= 0.0 && low.top + strip.row_height <= 1.0);
}

#[test]
fn a_wide_range_gives_every_pitch_its_own_row_top_to_bottom() {
    let strip = preview(vec![hit(24, 0.0, 1.0), hit(48, 1.0, 1.0)], 4.0).strip();

    assert_eq!(strip.row_height, 1.0 / 25.0);
    assert_eq!(strip.notes[1].top, 0.0);
    assert_eq!(strip.notes[0].top, 24.0 / 25.0);
}

#[test]
fn a_note_held_past_the_clip_ends_at_its_edge() {
    let strip = preview(vec![hit(36, 3.0, 4.0)], 4.0).strip();

    assert_eq!(strip.notes[0].end, 1.0);
}

#[test]
fn the_playhead_follows_the_audition_and_wraps_a_loop() {
    let pattern = MidiAuditionPattern::new(vec![hit(36, 0.0, 1.0)], 4.0);

    assert_eq!(playhead_fraction(&pattern, 100, 100.0, false), Some(0.25));
    assert_eq!(playhead_fraction(&pattern, 500, 100.0, true), Some(0.25));
    assert_eq!(playhead_fraction(&pattern, 500, 100.0, false), Some(1.0));
    assert_eq!(playhead_fraction(&pattern, 100, 0.0, true), None);
}

fn midi_track(instrument: bool) -> ProjectTrack {
    let kind = if instrument {
        TrackKind::Instrument(InstrumentKind::SubtractiveSynth)
    } else {
        TrackKind::Midi
    };
    ProjectTrack::new_instrument(TrackId::new(), "Bass".into(), kind, 0)
}

#[test]
fn the_selected_midi_track_with_an_instrument_plays_the_audition() {
    let tracks = vec![midi_track(true)];

    assert_eq!(
        audition_target(Some(tracks[0].id), &tracks),
        Some(tracks[0].id)
    );
}

#[test]
fn the_preview_voice_plays_when_the_selection_cannot() {
    let audio = ProjectTrack::new(TrackId::new(), "Drums".into(), 0);
    let bare = midi_track(false);
    let tracks = vec![audio, bare];

    assert_eq!(audition_target(Some(tracks[0].id), &tracks), None);
    assert_eq!(audition_target(Some(tracks[1].id), &tracks), None);
    assert_eq!(audition_target(None, &tracks), None);
}
