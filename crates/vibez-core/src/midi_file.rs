//! Standard MIDI Files read into beat-timed notes, one entry per track that plays.

use std::path::Path;

use crate::midi::MidiNote;

/// Extensions a file must carry to be offered as a MIDI file.
pub const MIDI_FILE_EXTENSIONS: &[&str] = &["mid", "midi"];

/// The General MIDI percussion channel (channel 10), zero-based.
const DRUM_CHANNEL: u8 = 9;

/// A MIDI file's notes, timed in beats from its own quarter-note resolution.
#[derive(Debug, Clone, PartialEq)]
pub struct MidiFile {
    /// The tracks that play notes, in file order.
    pub tracks: Vec<MidiFileTrack>,
    /// Where the file ends: its last End of Track, never before a note ends.
    pub length_beats: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MidiFileTrack {
    pub name: Option<String>,
    /// Sorted by start, then pitch.
    pub notes: Vec<MidiNote>,
    /// Every note sits on the General MIDI percussion channel.
    pub on_drum_channel: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MidiFileError {
    /// The bytes are not a Standard MIDI File.
    Malformed(String),
    /// Timed in SMPTE frames rather than beats.
    TimecodeTiming,
    NoNotes,
}

impl std::fmt::Display for MidiFileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Malformed(reason) => write!(f, "not a readable MIDI file ({reason})"),
            Self::TimecodeTiming => f.write_str("SMPTE-timed MIDI files are not supported"),
            Self::NoNotes => f.write_str("the MIDI file has no notes"),
        }
    }
}

impl std::error::Error for MidiFileError {}

pub fn is_midi_file_path(path: &Path) -> bool {
    todo!("{path:?}")
}

impl MidiFile {
    pub fn read(bytes: &[u8]) -> Result<Self, MidiFileError> {
        todo!("{}", bytes.len())
    }

    /// Every track's notes as one sorted list, for a single Clip.
    pub fn merged_notes(&self) -> Vec<MidiNote> {
        todo!()
    }

    /// Every note sits on the General MIDI percussion channel.
    pub fn on_drum_channel(&self) -> bool {
        todo!("{DRUM_CHANNEL}")
    }
}

#[cfg(test)]
#[path = "midi_file_tests.rs"]
mod tests;
