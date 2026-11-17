//! The Clip a Browser drag would make, drawn where a release would land it
//! before the drop, as Ableton does.

use std::sync::Arc;

use vibez_core::audio_buffer::DecodedAudio;
use vibez_core::id::{ClipId, TrackId};
use vibez_core::track::MediaSourceRef;

use crate::domains::midi_file_import::{MidiClipTrack, MidiFileImport, MidiFileLayout};
use crate::state::{AuditionMode, BrowserDropTarget, BrowserState, UiClip, UiNoteClip};
use crate::warp::LoopGridFit;

/// A dragged file as its ghost shows it, read once per drag.
#[derive(Debug, Clone)]
pub enum GhostMedia {
    Audio {
        /// The whole file as one Clip, at the file's own sample rate.
        clip: Box<UiClip>,
        loop_fit: Option<LoopGridFit>,
        /// The waveform across the whole Clip, worked out with the read so
        /// the lane never scans the file while it draws.
        peaks: Arc<Vec<(f32, f32)>>,
    },
    Midi {
        /// The Clip a one-Clip drop makes, at beat zero.
        clip: UiNoteClip,
        drum_rack: bool,
    },
}

impl GhostMedia {
    /// The note Clip a one-Clip drop of `import` makes, and whether a new
    /// track for it holds a Drum Rack.
    pub fn midi(import: &MidiFileImport) -> Self {
        let _ = import;
        todo!()
    }

    /// How many beats the Clip a drop makes lasts.
    pub fn length_beats(&self, mode: AuditionMode, project_bpm: f64) -> Option<f64> {
        let _ = (mode, project_bpm);
        todo!()
    }

    /// The track a drop on empty space makes for this media.
    pub fn new_track(&self) -> NewTrack {
        todo!()
    }
}

/// The whole of `audio` as one Clip named `name`, for a ghost to draw.
pub fn audio_clip(name: String, source: MediaSourceRef, audio: Arc<DecodedAudio>) -> UiClip {
    let _ = (name, source, audio);
    todo!()
}

/// The kind of track a drop on empty space adds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NewTrack {
    Audio,
    Midi { drum_rack: bool },
}

impl NewTrack {
    /// What the new track's header says while the ghost waits in it.
    pub fn label(self) -> &'static str {
        todo!()
    }
}

/// Where the ghost waits.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum GhostPlace {
    /// On an existing track's lane, from `beat`.
    Lane { track_id: TrackId, beat: f64 },
    /// On the lane of a track the drop adds, from `beat`.
    NewTrack { beat: f64 },
}

/// The Clip a release would make and where it would land.
#[derive(Debug, Clone, Copy)]
pub struct DropGhost<'a> {
    pub place: GhostPlace,
    pub length_beats: f64,
    pub media: &'a GhostMedia,
}

/// What a drag has read of its file for the ghost.
#[derive(Debug, Clone)]
pub struct DragGhostRead {
    pub source: MediaSourceRef,
    pub media: GhostMedia,
}

impl BrowserState {
    /// The ghost of the drag in flight where a release would land it; none
    /// until the file has been read, and none where the target turns the
    /// drag away.
    pub fn drop_ghost(&self, project_bpm: f64) -> Option<DropGhost<'_>> {
        let _ = project_bpm;
        None
    }

    /// The ghost waiting on `track_id`'s lane.
    pub fn lane_drop_ghost(&self, track_id: TrackId, project_bpm: f64) -> Option<DropGhost<'_>> {
        self.drop_ghost(project_bpm).filter(|ghost| {
            matches!(ghost.place, GhostPlace::Lane { track_id: lane, .. } if lane == track_id)
        })
    }

    /// Keeps what was read of `source` for the ghost while it is still the
    /// drag in flight; a read that finishes after its drag ended is dropped.
    pub(super) fn install_drag_ghost(&mut self, read: DragGhostRead) {
        let _ = read;
    }
}

#[cfg(test)]
#[path = "drop_ghost_tests.rs"]
mod tests;
