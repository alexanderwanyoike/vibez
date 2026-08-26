//! Pure geometry shared by clip split, trim and join operations.

use vibez_core::automation::AutomationLane;
use vibez_core::midi::MidiNote;
use vibez_core::track::ClipPlaybackDirection;

use crate::state::{UiClip, UiNoteClip};

/// Resolve the source phase at which a fragment must begin so its audible
/// result exactly matches `[local_start, local_start + duration)` of the
/// original clip. Reverse playback traverses clip time from the opposite edge,
/// so its phase is measured from the original clip's right edge.
pub(super) fn audio_fragment_source_start(clip: &UiClip, local_start: u64, duration: u64) -> u64 {
    let phase = match clip.playback_direction {
        ClipPlaybackDirection::Forward => local_start,
        ClipPlaybackDirection::Reverse => clip
            .duration
            .saturating_sub(local_start)
            .saturating_sub(duration),
    };
    clip.timeline().source_at(phase)
}

pub(super) fn visible_notes(clip: &UiNoteClip, local_start: f64, local_end: f64) -> Vec<MidiNote> {
    let mut visible = Vec::new();
    for note in &clip.notes {
        for occurrence in clip.note_occurrences(note.start_beat) {
            let note_end = occurrence + note.duration_beats;
            let kept_start = occurrence.max(local_start);
            let kept_end = note_end.min(local_end);
            if kept_end > kept_start {
                visible.push(MidiNote {
                    start_beat: kept_start - local_start,
                    duration_beats: kept_end - kept_start,
                    ..*note
                });
            }
        }
    }
    visible.sort_by(|a, b| a.start_beat.total_cmp(&b.start_beat));
    visible
}

/// Unmuted portions of `[start, end)` in project beats. Track Mute is a
/// stepped lane and deliberately imposes no state before its first point, so
/// uncaptured material before the first event is retained.
pub(super) fn unmuted_beat_ranges(lane: &AutomationLane, start: f64, end: f64) -> Vec<(f64, f64)> {
    if end <= start {
        return Vec::new();
    }

    let mut boundaries = vec![start];
    boundaries.extend(
        lane.points
            .iter()
            .map(|point| point.beat)
            .filter(|beat| *beat > start && *beat < end),
    );
    boundaries.push(end);

    let mut ranges: Vec<(f64, f64)> = Vec::new();
    for window in boundaries.windows(2) {
        let range_start = window[0];
        let range_end = window[1];
        let muted = lane.value_at(range_start).is_some_and(|value| value >= 0.5);
        if muted || range_end <= range_start {
            continue;
        }
        if let Some((_, previous_end)) = ranges.last_mut() {
            if *previous_end == range_start {
                *previous_end = range_end;
                continue;
            }
        }
        ranges.push((range_start, range_end));
    }
    ranges
}
