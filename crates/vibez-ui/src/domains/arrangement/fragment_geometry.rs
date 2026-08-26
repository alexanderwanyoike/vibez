//! Pure geometry shared by clip split, trim and join operations.

use vibez_core::automation::AutomationLane;
use vibez_core::id::ClipId;
use vibez_core::midi::MidiNote;
use vibez_core::warp_marker::WarpMarkers;

use crate::state::{UiClip, UiNoteClip};

/// Resolve the source phase at which a fragment must begin so its audible
/// result exactly matches `[local_start, local_start + duration)` of the
/// original clip. Reverse playback traverses clip time from the opposite edge,
/// so its phase is measured from the original clip's right edge.
pub(super) fn audio_fragment_geometry(
    clip: &UiClip,
    local_start: u64,
    duration: u64,
) -> (u64, u64, WarpMarkers) {
    clip.warp_geometry_for_fragment(local_start, duration)
}

/// Build one independently editable view over an audible fragment of an Audio
/// Clip. All split, slice and Track Mute trim operations use this constructor
/// so identity, timing, fades, markers and Warp geometry cannot drift apart.
pub(super) fn audio_fragment(
    clip: &UiClip,
    name: String,
    local_start: u64,
    duration: u64,
) -> UiClip {
    let mut fragment = clip.clone();
    fragment.id = ClipId::new();
    fragment.name = name;
    fragment.position = clip.position.saturating_add(local_start);
    fragment.duration = duration;
    (
        fragment.source_offset,
        fragment.start_marker,
        fragment.warp_markers,
    ) = audio_fragment_geometry(clip, local_start, duration);
    fragment.fades = clip
        .fades
        .for_fragment(clip.duration, local_start, duration);
    fragment
        .transient_markers
        .retain_source_range(fragment.source_offset, fragment.source_end());
    fragment
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
