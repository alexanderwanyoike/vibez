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
    #[serde(flatten)]
    pub launch: ClipLaunchOverrides,
}

/// A Clip's own launch options. `None` follows its Project Track.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClipLaunchOverrides {
    #[serde(
        rename = "launch_quantization",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub quantization: Option<MusicalBoundary>,
    #[serde(
        rename = "launch_mode",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub mode: Option<ClipLaunchMode>,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clip_launch_overrides_save_under_their_own_names_and_only_when_set() {
        let set = ClipLaunchOverrides {
            quantization: Some(MusicalBoundary::Eighth),
            mode: Some(ClipLaunchMode::Legato),
        };
        let json = serde_json::to_value(set).unwrap();
        assert_eq!(
            json,
            serde_json::json!({ "launch_quantization": "eighth", "launch_mode": "legato" })
        );
        assert_eq!(
            serde_json::from_value::<ClipLaunchOverrides>(json).unwrap(),
            set
        );
        assert_eq!(
            serde_json::to_value(ClipLaunchOverrides::default()).unwrap(),
            serde_json::json!({})
        );
    }
}
