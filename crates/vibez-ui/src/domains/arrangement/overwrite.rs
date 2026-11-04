//! Clips placed on a lane overwrite the Clips already under them, like
//! Ableton: covered parts are trimmed away, split around or removed.

use std::collections::HashSet;

use vibez_core::id::ClipId;

use super::clip_trims::{audio_start_trimmed, note_start_trimmed};
use super::fragment_geometry::audio_fragment;
use crate::state::{TrackTimelineContent, UiClip, UiNoteClip, SHORTEST_LENGTH_BEATS};

const BEAT_EPSILON: f64 = 1e-9;

/// What is left of each lane Clip that an incoming range touched. A Clip
/// listed with no pieces is fully covered.
#[derive(Debug, Default)]
pub(super) struct LaneOverwrite {
    pub(super) audio: Vec<(ClipId, Vec<UiClip>)>,
    pub(super) notes: Vec<(ClipId, Vec<UiNoteClip>)>,
}

/// The edits that placing `placed` over `start_beats..end_beats` makes to
/// the other Clips on `lane`. A piece shorter than the shortest Clip is
/// dropped, since it could be neither seen nor grabbed.
pub(super) fn lane_overwrite(
    lane: &TrackTimelineContent,
    (start_beats, end_beats): (f64, f64),
    placed: &HashSet<ClipId>,
    samples_per_beat: f64,
) -> LaneOverwrite {
    let mut edits = LaneOverwrite::default();
    if samples_per_beat > 0.0 {
        let frames = |beats: f64| (beats * samples_per_beat).round().max(0.0) as u64;
        let (start, end) = (frames(start_beats), frames(end_beats));
        let shortest = frames(SHORTEST_LENGTH_BEATS).max(1);
        for clip in lane.clips.iter().filter(|clip| !placed.contains(&clip.id)) {
            let clip_end = clip.position.saturating_add(clip.duration);
            if clip.position >= end || clip_end <= start {
                continue;
            }
            let mut pieces = Vec::with_capacity(2);
            let head = start.saturating_sub(clip.position);
            if head >= shortest {
                let mut kept = audio_fragment(clip, clip.name.clone(), 0, head);
                kept.id = clip.id;
                pieces.push(kept);
            }
            let tail = clip_end.saturating_sub(end);
            if tail >= shortest {
                if let Some(mut kept) = audio_start_trimmed(clip, (clip.duration - tail) as i64, 0)
                {
                    if !pieces.is_empty() {
                        kept.id = ClipId::new();
                    }
                    pieces.push(kept);
                }
            }
            edits.audio.push((clip.id, pieces));
        }
    }
    for clip in lane
        .note_clips
        .iter()
        .filter(|clip| !placed.contains(&clip.id))
    {
        let clip_end = clip.position_beats + clip.duration_beats;
        if clip.position_beats >= end_beats - BEAT_EPSILON || clip_end <= start_beats + BEAT_EPSILON
        {
            continue;
        }
        let mut pieces = Vec::with_capacity(2);
        let head = start_beats - clip.position_beats;
        if head >= SHORTEST_LENGTH_BEATS - BEAT_EPSILON {
            let mut kept = clip.clone();
            kept.resize(head);
            pieces.push(kept);
        }
        let tail = clip_end - end_beats;
        if tail >= SHORTEST_LENGTH_BEATS - BEAT_EPSILON {
            if let Some(mut kept) = note_start_trimmed(clip, clip.duration_beats - tail) {
                if !pieces.is_empty() {
                    kept.id = ClipId::new();
                }
                pieces.push(kept);
            }
        }
        edits.notes.push((clip.id, pieces));
    }
    edits
}
