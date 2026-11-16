//! The Browser's preview of a selected MIDI file, showing and playing the Clip a drop would land on the track whose instrument plays it.

use std::sync::Arc;

use vibez_core::id::TrackId;
use vibez_core::time::BEATS_PER_BAR;
use vibez_core::track::MediaSourceRef;
use vibez_engine::midi_audition::MidiAuditionPattern;

use crate::domains::midi_file_import::{MidiFileImport, MidiFileLayout};
use crate::state::ProjectTrack;

/// A file spanning fewer pitches is drawn over an octave, so a bass line or
/// a few drums keep the rows of a small piano roll.
const MIN_STRIP_ROWS: u8 = 12;

/// A selected MIDI file, read for the Audition footer.
#[derive(Debug, Clone)]
pub struct MidiPreview {
    pub source: MediaSourceRef,
    pub pattern: Arc<MidiAuditionPattern>,
}

impl MidiPreview {
    /// Previews the Clip a drop would land, so what the producer hears and
    /// sees is what landing gives them.
    pub fn of(source: MediaSourceRef, import: &MidiFileImport) -> Self {
        let clip = import
            .plan(None, 0.0, MidiFileLayout::OneClip)
            .swap_remove(0)
            .clip;
        Self {
            source,
            pattern: Arc::new(MidiAuditionPattern::new(clip.notes, clip.duration_beats)),
        }
    }

    pub fn strip(&self) -> MidiStrip {
        let notes = self.pattern.notes();
        let length = self.pattern.length_beats();
        let lowest = notes.iter().map(|note| note.pitch).min().unwrap_or(60);
        let highest = notes.iter().map(|note| note.pitch).max().unwrap_or(60);
        let spanned = highest - lowest + 1;
        let rows = spanned.max(MIN_STRIP_ROWS);
        let top_pitch = f32::from(highest) + f32::from((rows - spanned) / 2);
        let across = |beat: f64| (beat / length).clamp(0.0, 1.0) as f32;
        MidiStrip {
            notes: notes
                .iter()
                .map(|note| StripNote {
                    start: across(note.start_beat),
                    end: across(note.end_beat()),
                    depth: (top_pitch - f32::from(note.pitch)) / f32::from(rows),
                    velocity: 0,
                })
                .collect(),
            bar_lines: (1..)
                .map(|bar| f64::from(bar) * BEATS_PER_BAR)
                .take_while(|&beat| beat < length)
                .map(across)
                .collect(),
            rows: crate::note_rows::NoteRows::of([]),
        }
    }
}

/// A note in the footer strip, as fractions of its width and height.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StripNote {
    pub start: f32,
    pub end: f32,
    /// How far down the strip the note sits.
    pub depth: f32,
    pub velocity: u8,
}

/// The strip's notes and bar lines, laid out once per preview.
#[derive(Debug, Clone, PartialEq)]
pub struct MidiStrip {
    pub notes: Vec<StripNote>,
    /// Each bar line after the first bar, as a fraction of the width.
    pub bar_lines: Vec<f32>,
    pub rows: crate::note_rows::NoteRows,
}

/// The track whose instrument plays the audition, which is the selected
/// track when it can play MIDI; without one the preview voice plays it.
pub fn audition_target(selected: Option<TrackId>, tracks: &[ProjectTrack]) -> Option<TrackId> {
    let selected = selected?;
    tracks
        .iter()
        .any(|track| track.id == selected && track.is_playable_midi_target())
        .then_some(selected)
}

/// Who plays the audition, as the Audition footer names it.
pub fn audition_player(selected: Option<TrackId>, tracks: &[ProjectTrack]) -> String {
    let track = audition_target(selected, tracks)
        .and_then(|target| tracks.iter().find(|track| track.id == target));
    match track {
        Some(track) => format!("on {}", track.name),
        None => "on Preview voice".into(),
    }
}

/// Where the playhead sits in the strip, `position` samples into the
/// audition.
pub fn playhead_fraction(
    pattern: &MidiAuditionPattern,
    position: u64,
    samples_per_beat: f64,
    looped: bool,
) -> Option<f32> {
    let length = pattern.length_beats();
    if samples_per_beat <= 0.0 || length <= 0.0 {
        return None;
    }
    let beats = position as f64 / samples_per_beat;
    let played = if looped {
        beats.rem_euclid(length)
    } else {
        beats.min(length)
    };
    Some((played / length) as f32)
}

#[cfg(test)]
#[path = "midi_audition_tests.rs"]
mod tests;
