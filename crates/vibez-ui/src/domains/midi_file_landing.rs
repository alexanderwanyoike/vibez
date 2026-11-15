//! Where a MIDI file drop lands, checked against the project when the file
//! has been read.

use vibez_core::id::{SectionId, TrackId};

use crate::domains::midi_file_import::MIDI_FILE_TARGETS;
use crate::domains::perform::PerformState;
use crate::message::BrowserImportTarget;
use crate::state::ProjectTracksState;

/// Read-only facts a drop target is checked against.
pub struct MidiLandingCtx<'a> {
    pub project_tracks: &'a ProjectTracksState,
    pub perform: &'a PerformState,
    pub samples_per_beat: f64,
    pub playhead_beats: f64,
}

/// Where the Clips of one MIDI file drop go.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MidiLanding {
    Arrange {
        onto: Option<TrackId>,
        beat: f64,
    },
    Section {
        section_id: SectionId,
        onto: Option<TrackId>,
        beat: f64,
    },
    Slot {
        onto: TrackId,
        row: u32,
    },
}

impl MidiLanding {
    /// The landing `target` names, or why it takes no MIDI file; the track,
    /// Section or slot may have gone while the file was read.
    pub fn resolve(target: &BrowserImportTarget, ctx: &MidiLandingCtx<'_>) -> Result<Self, String> {
        let midi_track = |track_id: TrackId| {
            ctx.project_tracks
                .find(track_id)
                .is_some_and(|track| track.kind.is_midi())
        };
        let beat = |position_samples: u64| {
            position_samples as f64 / ctx.samples_per_beat.max(f64::EPSILON)
        };
        match *target {
            BrowserImportTarget::ArrangementClipAt {
                track_id,
                position_samples,
            } if midi_track(track_id) => Ok(Self::Arrange {
                onto: Some(track_id),
                beat: beat(position_samples),
            }),
            BrowserImportTarget::ArrangementClip(preferred) => Ok(Self::Arrange {
                onto: preferred.filter(|track_id| midi_track(*track_id)),
                beat: ctx.playhead_beats,
            }),
            BrowserImportTarget::ArrangementNewTrackAt { position_samples } => Ok(Self::Arrange {
                onto: None,
                beat: beat(position_samples),
            }),
            BrowserImportTarget::SectionClipAt {
                section_id,
                track_id,
                position_samples,
            } if midi_track(track_id) && ctx.perform.sections.by_id(section_id).is_some() => {
                Ok(Self::Section {
                    section_id,
                    onto: Some(track_id),
                    beat: beat(position_samples),
                })
            }
            BrowserImportTarget::LauncherClipAt { track_id, row } if midi_track(track_id) => {
                if !ctx.perform.launcher_slot_is_free(track_id, row) {
                    return Err(crate::domains::perform::LAUNCHER_SLOT_GONE.into());
                }
                Ok(Self::Slot {
                    onto: track_id,
                    row,
                })
            }
            _ => Err(MIDI_FILE_TARGETS.into()),
        }
    }

    /// The track the Clips go on, or `None` for new tracks.
    pub fn onto(self) -> Option<TrackId> {
        match self {
            Self::Arrange { onto, .. } | Self::Section { onto, .. } => onto,
            Self::Slot { onto, .. } => Some(onto),
        }
    }

    /// Where a Clip starts on its timeline; a slot's Clip starts at zero.
    pub fn beat(self) -> f64 {
        match self {
            Self::Arrange { beat, .. } | Self::Section { beat, .. } => beat,
            Self::Slot { .. } => 0.0,
        }
    }

    /// What the status bar says once `file` has landed on `tracks`.
    pub fn status(self, file: &str, tracks: &[&str]) -> String {
        match (self, tracks) {
            (_, [_, _, ..]) => format!("Imported '{file}' as {} new MIDI tracks", tracks.len()),
            (Self::Slot { row, .. }, _) => {
                crate::domains::perform::LauncherClip::added_status(file, row)
            }
            (_, tracks) => format!(
                "Imported '{file}' to {} at beat {:.2}",
                tracks.first().copied().unwrap_or_default(),
                self.beat()
            ),
        }
    }
}

#[cfg(test)]
#[path = "midi_file_landing_tests.rs"]
mod tests;
