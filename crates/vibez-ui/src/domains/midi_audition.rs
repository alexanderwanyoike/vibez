//! What the Browser shows and plays of a selected MIDI file: its notes over the Clip a drop would land, and the track whose instrument plays them.

use std::sync::Arc;

use vibez_core::id::TrackId;
use vibez_core::track::MediaSourceRef;
use vibez_engine::midi_audition::MidiAuditionPattern;

use crate::domains::midi_file_import::MidiFileImport;
use crate::state::ProjectTrack;

/// A selected MIDI file, read for the Audition footer.
#[derive(Debug, Clone)]
pub struct MidiPreview {
    pub source: MediaSourceRef,
    pub pattern: Arc<MidiAuditionPattern>,
}

impl MidiPreview {
    pub fn of(source: MediaSourceRef, _import: &MidiFileImport) -> Self {
        Self {
            source,
            pattern: Arc::new(MidiAuditionPattern::new(Vec::new(), 0.0)),
        }
    }

    pub fn strip(&self) -> MidiStrip {
        MidiStrip::default()
    }
}

/// A note in the footer strip, as fractions of its width and height.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StripNote {
    pub start: f32,
    pub end: f32,
    /// The top of the note's pitch row.
    pub top: f32,
}

/// The strip's notes and bar lines, laid out once per preview.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MidiStrip {
    pub notes: Vec<StripNote>,
    /// Each bar line after the first bar, as a fraction of the width.
    pub bar_lines: Vec<f32>,
    /// One pitch row, as a fraction of the height.
    pub row_height: f32,
}

/// The track whose instrument plays the audition: the selected track when
/// it can play MIDI, otherwise none and the preview voice plays it.
pub fn audition_target(_selected: Option<TrackId>, _tracks: &[ProjectTrack]) -> Option<TrackId> {
    None
}

/// Where the playhead sits in the strip, `position` samples into the
/// audition.
pub fn playhead_fraction(
    _pattern: &MidiAuditionPattern,
    _position: u64,
    _samples_per_beat: f64,
    _looped: bool,
) -> Option<f32> {
    None
}

#[cfg(test)]
#[path = "midi_audition_tests.rs"]
mod tests;
