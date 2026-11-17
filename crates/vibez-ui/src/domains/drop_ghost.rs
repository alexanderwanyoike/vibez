//! The Clip a Browser drag would make, drawn where a release would land it
//! before the drop, as Ableton does.

use std::sync::Arc;

use vibez_core::audio_buffer::DecodedAudio;
use vibez_core::id::{ClipId, TrackId};
use vibez_core::track::MediaSourceRef;

use crate::domains::browser::DragMedia;
use crate::domains::midi_file_import::{MidiClipTrack, MidiFileImport, MidiFileLayout};
use crate::domains::perform::LauncherClip;
use crate::state::{
    ArrangementTimeline, AuditionMode, BrowserDropTarget, BrowserState, UiClip, UiNoteClip,
};
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
        let planned = import
            .plan(None, 0.0, MidiFileLayout::OneClip)
            .into_iter()
            .next()
            .expect("a one-Clip plan holds one Clip");
        let drum_rack = matches!(
            planned.track,
            MidiClipTrack::New {
                drum_rack: true,
                ..
            }
        );
        Self::Midi {
            clip: planned.clip,
            drum_rack,
        }
    }

    /// How many beats the Clip a drop makes lasts.
    pub fn length_beats(&self, mode: AuditionMode, project_bpm: f64) -> Option<f64> {
        match self {
            Self::Audio { clip, loop_fit, .. } => {
                let rate = clip.audio.sample_rate;
                if rate == 0 {
                    return None;
                }
                mode.imported_beats(clip.duration as f64 / rate as f64, *loop_fit, project_bpm)
            }
            Self::Midi { clip, .. } => Some(clip.duration_beats),
        }
    }

    /// The track a drop on empty space makes for this media.
    pub fn new_track(&self) -> NewTrack {
        match self {
            Self::Audio { .. } => NewTrack::Audio,
            Self::Midi { drum_rack, .. } => NewTrack::Midi {
                drum_rack: *drum_rack,
            },
        }
    }
}

/// The whole of `audio` as one Clip named `name`, for a ghost to draw.
pub fn audio_clip(name: String, source: MediaSourceRef, audio: Arc<DecodedAudio>) -> UiClip {
    let duration = audio.num_frames() as u64;
    UiClip {
        id: ClipId::new(),
        name,
        audio,
        source,
        position: 0,
        source_offset: 0,
        start_marker: 0,
        duration,
        loop_enabled: false,
        loop_start: 0,
        loop_end: duration,
        gain_db: Default::default(),
        fades: Default::default(),
        playback_direction: Default::default(),
        transient_markers: Default::default(),
        warp_markers: Default::default(),
        transpose: Default::default(),
        original_bpm: None,
        warped: false,
        warped_to_bpm: None,
        original_audio: None,
    }
}

/// The kind of track a drop on empty space adds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NewTrack {
    Audio,
    Midi { drum_rack: bool },
}

impl NewTrack {
    /// The track a drop of `media` adds, before its file says whether the
    /// notes are drums.
    pub fn of(media: DragMedia) -> Self {
        match media {
            DragMedia::Audio => Self::Audio,
            DragMedia::Midi => Self::Midi { drum_rack: false },
        }
    }

    /// What the new track's header says while the ghost waits in it.
    pub fn label(self) -> &'static str {
        match self {
            Self::Audio => "New audio track",
            Self::Midi { drum_rack: false } => "New MIDI track",
            Self::Midi { drum_rack: true } => "New MIDI track · Drum Rack",
        }
    }
}

/// Where the ghost waits.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum GhostPlace {
    /// On an existing track's lane, from `beat`.
    Lane { track_id: TrackId, beat: f64 },
    /// On the lane of a track the drop adds, from `beat`.
    NewTrack { beat: f64 },
    /// In `row`'s Clip slot of a track the drop adds beside the clip
    /// launcher's last column.
    NewTrackSlot { row: u32 },
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

/// What a drag has read, and the clip launcher Clip it shows as, built once
/// so the slot's thumbnail is drawn once.
#[derive(Debug, Clone)]
pub struct GhostCache {
    pub read: DragGhostRead,
    slot: LauncherClip,
}

impl GhostCache {
    fn new(read: DragGhostRead) -> Self {
        let track_id = TrackId::new();
        let mut timeline = ArrangementTimeline::default();
        let lane = timeline.ensure(track_id);
        match &read.media {
            // A launcher's audio Clip loops over the whole file.
            GhostMedia::Audio { clip, .. } => lane.clips.push(UiClip {
                loop_enabled: true,
                ..(**clip).clone()
            }),
            GhostMedia::Midi { clip, .. } => lane.note_clips.push(clip.clone()),
        }
        let slot = LauncherClip {
            id: ClipId::new(),
            track_id,
            row: 0,
            timeline: Arc::new(timeline),
            launch: Default::default(),
        };
        Self { read, slot }
    }
}

impl BrowserState {
    /// The ghost of the drag in flight where a release would land it; none
    /// until the file has been read, and none where the target turns the
    /// drag away.
    pub fn drop_ghost(&self, project_bpm: f64) -> Option<DropGhost<'_>> {
        let read = &self.current_ghost_cache()?.read;
        let place = match *self.drag_target.as_ref()? {
            BrowserDropTarget::ArrangementLane {
                track_id,
                beat,
                compatible: true,
            } => GhostPlace::Lane { track_id, beat },
            BrowserDropTarget::EmptyArrangement { beat } => GhostPlace::NewTrack { beat },
            BrowserDropTarget::NewTrackSlot { row } => GhostPlace::NewTrackSlot { row },
            _ => return None,
        };
        Some(DropGhost {
            place,
            length_beats: read.media.length_beats(self.audition_mode, project_bpm)?,
            media: &read.media,
        })
    }

    /// The kind of track a drop on empty space adds: what the file read
    /// says, or what its media says until then.
    pub fn new_track_kind(&self) -> Option<NewTrack> {
        self.current_ghost_cache()
            .map(|ghost| ghost.read.media.new_track())
            .or_else(|| self.drag_media().map(NewTrack::of))
    }

    /// The ghost waiting on the lane of a track the drop adds.
    pub fn new_track_drop_ghost(&self, project_bpm: f64) -> Option<DropGhost<'_>> {
        self.drop_ghost(project_bpm)
            .filter(|ghost| matches!(ghost.place, GhostPlace::NewTrack { .. }))
    }

    /// The ghost waiting on `track_id`'s lane.
    pub fn lane_drop_ghost(&self, track_id: TrackId, project_bpm: f64) -> Option<DropGhost<'_>> {
        self.drop_ghost(project_bpm).filter(|ghost| {
            matches!(ghost.place, GhostPlace::Lane { track_id: lane, .. } if lane == track_id)
        })
    }

    /// The launcher Clip the ghost shows as in a new column's slot.
    pub fn drop_ghost_slot(&self) -> Option<&LauncherClip> {
        let ghost = self.current_ghost_cache()?;
        matches!(
            self.drag_target,
            Some(BrowserDropTarget::NewTrackSlot { .. })
        )
        .then_some(&ghost.slot)
    }

    fn current_ghost_cache(&self) -> Option<&GhostCache> {
        self.ghost_cache
            .as_ref()
            .filter(|ghost| self.drag_source.as_ref() == Some(&ghost.read.source))
    }

    /// Keeps what was read of `source` for the ghost while it is still the
    /// drag in flight; a read that finishes after its drag ended is dropped.
    pub(super) fn install_ghost_cache(&mut self, read: DragGhostRead) {
        if self.drag_source.as_ref() == Some(&read.source) {
            self.ghost_cache = Some(GhostCache::new(read));
        }
    }
}

#[cfg(test)]
#[path = "drop_ghost_tests.rs"]
mod tests;
