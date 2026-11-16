//! The note Clips a dropped MIDI file becomes and the tracks they land on.

use std::collections::HashSet;

use vibez_core::id::{ClipId, TrackId};
use vibez_core::midi::MidiNote;
use vibez_core::midi_file::MidiFile;
use vibez_core::perform::GrooveGrid;
use vibez_core::time::MusicalTime;

use crate::state::UiNoteClip;

/// Where a MIDI file can go, said when it is dropped anywhere else.
pub const MIDI_FILE_TARGETS: &str =
    "MIDI files go on a MIDI track, empty Arrange or a MIDI track's Clip slot";

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
    New {
        name: String,
        drum_rack: bool,
    },
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
        self.file.tracks.len() > 1
    }

    /// Each playing track's name, or the file's numbered by position.
    pub fn track_names(&self) -> Vec<String> {
        self.file
            .tracks
            .iter()
            .enumerate()
            .map(|(index, track)| {
                track
                    .name
                    .clone()
                    .unwrap_or_else(|| format!("{} {}", self.name, index + 1))
            })
            .collect()
    }

    /// What a track per file track adds, as the layout choice says it.
    pub fn track_each_summary(&self) -> String {
        let tracks = self.file.tracks.len();
        match self
            .file
            .tracks
            .iter()
            .filter(|track| track.on_drum_channel)
            .count()
        {
            0 => format!("{tracks} new MIDI tracks"),
            drums if drums == tracks => format!("{tracks} new MIDI tracks, each on a Drum Rack"),
            drums => format!("{tracks} new MIDI tracks, {drums} of them on a Drum Rack"),
        }
    }

    /// The Clips for a drop at `position_beats` on `onto`, or on new tracks
    /// where there is no track to land on.
    pub fn plan(
        &self,
        onto: Option<TrackId>,
        position_beats: f64,
        layout: MidiFileLayout,
    ) -> Vec<PlannedMidiClip> {
        let clip = |name: &str, notes| self.clip(name, notes, position_beats);
        if layout == MidiFileLayout::TrackPerFileTrack && self.offers_layouts() {
            return self
                .file
                .tracks
                .iter()
                .zip(self.track_names())
                .map(|(track, name)| PlannedMidiClip {
                    clip: clip(&name, track.notes.clone()),
                    track: MidiClipTrack::New {
                        name,
                        drum_rack: track.on_drum_channel,
                    },
                })
                .collect();
        }
        let track = match onto {
            Some(track_id) => MidiClipTrack::Existing(track_id),
            None => MidiClipTrack::New {
                name: self.name.clone(),
                drum_rack: self.file.on_drum_channel(),
            },
        };
        vec![PlannedMidiClip {
            track,
            clip: clip(&self.name, self.file.merged_notes()),
        }]
    }

    /// A Clip looping over the whole file in whole bars, as a new pattern
    /// does. Many files end their track on the last note-off, short of the
    /// bar, and would otherwise loop early.
    fn clip(&self, name: &str, notes: Vec<MidiNote>, position_beats: f64) -> UiNoteClip {
        // The slack keeps a length a hair over a bar line, from float
        // error, on that line.
        let length = MusicalTime(self.file.length_beats)
            .whole_bars_holding(1e-6)
            .0;
        UiNoteClip {
            id: ClipId::new(),
            name: name.to_string(),
            position_beats,
            duration_beats: length,
            notes,
            selected_notes: HashSet::new(),
            start_marker_beats: 0.0,
            loop_enabled: true,
            loop_start_beats: 0.0,
            loop_end_beats: length,
            groove_grid: GrooveGrid::Off,
        }
    }
}

/// A dropped file that offers layouts, held until the producer picks one.
#[derive(Debug, Clone)]
pub struct MidiFileLayoutChoice {
    pub import: MidiFileImport,
    pub target: crate::message::BrowserImportTarget,
}

#[cfg(test)]
#[path = "midi_file_import_tests.rs"]
mod tests;
