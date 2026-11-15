//! The note Clips a dropped MIDI file becomes and the tracks they land on.

use std::collections::HashSet;

use vibez_core::id::{ClipId, TrackId};
use vibez_core::midi_file::MidiFile;

use crate::state::UiNoteClip;

/// How a file with several playing tracks lands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MidiFileLayout {
    /// Every track's notes in one Clip.
    OneClip,
    /// A new MIDI track for each of the file's tracks.
    TrackPerFileTrack,
}

/// The track a planned Clip goes on.
#[derive(Debug, Clone, PartialEq)]
pub enum MidiClipTrack {
    Existing(TrackId),
    /// A new MIDI track, holding a Drum Rack when its notes are General MIDI
    /// drums, whose pads start at the same note 36 as the drum map.
    New { name: String, drum_rack: bool },
}

#[derive(Debug, Clone, PartialEq)]
pub struct PlannedMidiClip {
    pub track: MidiClipTrack,
    pub clip: UiNoteClip,
}

/// A MIDI file read for a drop, named after the file it came from.
#[derive(Debug, Clone, PartialEq)]
pub struct MidiFileImport {
    pub name: String,
    pub file: MidiFile,
}

impl MidiFileImport {
    /// Whether the producer chooses between one Clip and a track per file
    /// track.
    pub fn offers_layouts(&self) -> bool {
        todo!()
    }

    /// The Clips for a drop at `position_beats` on `onto`, or on new tracks
    /// where there is no track to land on.
    pub fn plan(
        &self,
        onto: Option<TrackId>,
        position_beats: f64,
        layout: MidiFileLayout,
    ) -> Vec<PlannedMidiClip> {
        let _ = (onto, position_beats, layout, ClipId::new(), HashSet::<usize>::new());
        todo!()
    }
}

#[cfg(test)]
#[path = "midi_file_import_tests.rs"]
mod tests;
