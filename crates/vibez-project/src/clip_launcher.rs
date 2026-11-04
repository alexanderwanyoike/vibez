//! Project-owned Clip slots keep launchable material independent of Sections.

use serde::{Deserialize, Serialize};
use vibez_core::id::{ClipId, TrackId};
use vibez_core::perform::{ClipLaunchMode, MusicalBoundary};

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
    /// `None` follows the Project Track's launch quantization.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub launch_quantization: Option<MusicalBoundary>,
}

/// A Project Track's Clip launch options. Tracks on the defaults are not written.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LauncherTrackInfo {
    pub track_id: TrackId,
    pub launch_quantization: MusicalBoundary,
    #[serde(default, skip_serializing_if = "is_play")]
    pub launch_mode: ClipLaunchMode,
}

fn is_play(mode: &ClipLaunchMode) -> bool {
    *mode == ClipLaunchMode::Play
}
