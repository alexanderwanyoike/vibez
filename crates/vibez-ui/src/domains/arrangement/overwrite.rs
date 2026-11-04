//! Clips placed on a lane overwrite the Clips already under them, like
//! Ableton: covered parts are trimmed away, split around or removed.

use std::collections::HashSet;

use vibez_core::id::ClipId;

use crate::state::{TrackTimelineContent, UiClip, UiNoteClip};

/// What is left of each lane Clip that an incoming range touched. A Clip
/// listed with no pieces is fully covered.
#[derive(Debug, Default)]
pub(super) struct LaneOverwrite {
    pub(super) audio: Vec<(ClipId, Vec<UiClip>)>,
    pub(super) notes: Vec<(ClipId, Vec<UiNoteClip>)>,
}

/// The edits that placing `placed` over `start_beats..end_beats` makes to
/// the other Clips on `lane`.
pub(super) fn lane_overwrite(
    _lane: &TrackTimelineContent,
    (_start_beats, _end_beats): (f64, f64),
    _placed: &HashSet<ClipId>,
    _samples_per_beat: f64,
) -> LaneOverwrite {
    LaneOverwrite::default()
}
