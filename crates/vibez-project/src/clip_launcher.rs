//! Project-owned Clip slots keep launchable material independent of Sections.

use serde::{Deserialize, Serialize};
use vibez_core::id::{ClipId, TrackId};
use vibez_core::perform::MusicalBoundary;

use crate::TimelineInfo;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PerformLayout {
    #[default]
    Sections,
    Clips,
}

impl PerformLayout {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Sections => "Sections",
            Self::Clips => "Clips",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LauncherClipInfo {
    pub id: ClipId,
    pub track_id: TrackId,
    pub row: u32,
    pub timeline: TimelineInfo,
}

/// A Project Track's Clip launch options. Tracks on the defaults are not written.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LauncherTrackInfo {
    pub track_id: TrackId,
    pub launch_quantization: MusicalBoundary,
}
