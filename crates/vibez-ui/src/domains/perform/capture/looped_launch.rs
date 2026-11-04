//! One looping Arrange clip per track for each launch of a looping source.

use vibez_core::id::ClipId;
use vibez_core::track::{ClipFades, ClipPlaybackDirection};

use crate::state::{TrackTimelineContent, UiClip, UiNoteClip};

use super::{captured_clip_name, captured_visible_notes, on_tick, CapturedTimelineSource};

/// What one launch of a looping source played on the Arrange timeline.
#[derive(Debug, Clone, Copy)]
pub(super) struct LoopedLaunch {
    /// Where in the source pass the kept part of the launch began.
    pub phase_samples: u64,
    pub pass_samples: u64,
    pub played_samples: u64,
    pub destination_start_samples: u64,
    pub samples_per_beat: f64,
}

/// The track's whole pass as one MIDI clip whose loop is the pass, so the
/// clip repeats the source for as long as it played. `None` when the pass
/// has no MIDI clip, or its clips disagree on the groove they swing by.
pub(super) fn looped_note_clip(
    content: &TrackTimelineContent,
    source: &CapturedTimelineSource,
    launch: LoopedLaunch,
) -> Option<UiNoteClip> {
    let pass_beats = on_tick(source.length_beats);
    let sounding: Vec<_> = content
        .note_clips
        .iter()
        .filter(|clip| {
            clip.position_beats < pass_beats && clip.position_beats + clip.duration_beats > 0.0
        })
        .collect();
    let first = sounding.first()?;
    if sounding
        .iter()
        .any(|clip| clip.groove_grid != first.groove_grid)
    {
        return None;
    }
    let mut notes: Vec<_> = sounding
        .iter()
        .flat_map(|clip| {
            let local_start = (-clip.position_beats).max(0.0);
            let local_end = clip.duration_beats.min(pass_beats - clip.position_beats);
            captured_visible_notes(clip, local_start, local_end)
                .into_iter()
                .map(move |mut note| {
                    note.start_beat += clip.position_beats + local_start;
                    note
                })
        })
        .collect();
    notes.sort_by(|left, right| {
        left.start_beat
            .total_cmp(&right.start_beat)
            .then(left.pitch.cmp(&right.pitch))
    });
    let name = match sounding.as_slice() {
        [only] => captured_clip_name(source, &only.name),
        _ => captured_clip_name(source, &source.name),
    };
    let beats = |samples: u64| on_tick(samples as f64 / launch.samples_per_beat);
    let phase = beats(launch.phase_samples);
    let position_beats = beats(launch.destination_start_samples);
    let end_beats = beats(launch.destination_start_samples + launch.played_samples);
    Some(UiNoteClip {
        id: ClipId::new(),
        name,
        position_beats,
        duration_beats: end_beats - position_beats,
        notes,
        selected_notes: Default::default(),
        start_marker_beats: if phase < pass_beats { phase } else { 0.0 },
        loop_enabled: true,
        loop_start_beats: 0.0,
        loop_end_beats: pass_beats,
        groove_grid: first.groove_grid,
    })
}

/// The track's one audio clip, looping over what it plays in one pass.
/// `None` when no single looped clip can repeat the pass exactly: the pass
/// holds several clips or silence, plays in reverse, fades every pass, or
/// bends time with Warp Markers that only map the clip's own window.
pub(super) fn looped_audio_clip(
    content: &TrackTimelineContent,
    source: &CapturedTimelineSource,
    launch: LoopedLaunch,
) -> Option<UiClip> {
    let pass = launch.pass_samples;
    let mut sounding = content
        .clips
        .iter()
        .filter(|clip| clip.position < pass && clip.duration > 0);
    let clip = sounding.next()?;
    if sounding.next().is_some()
        || clip.position != 0
        || clip.duration < pass
        || clip.playback_direction != ClipPlaybackDirection::Forward
        || clip.fades != ClipFades::default()
        || !clip.warp_markers.is_empty()
    {
        return None;
    }
    let timeline = clip.timeline();
    let (loop_start, loop_end) = match timeline.first_wrap() {
        Some(wrap) if wrap < pass => {
            let cycle = clip.loop_end - clip.loop_start;
            let cycles = (pass as f64 / cycle as f64).round() as u64;
            // A cycle measured in whole frames may differ from the pass by
            // a rounding frame; anything more would drift audibly.
            let whole_cycles = cycles > 0 && pass.abs_diff(cycles * cycle) <= cycles;
            if timeline.start != clip.loop_start || !whole_cycles {
                return None;
            }
            (clip.loop_start, clip.loop_end)
        }
        _ => (clip.start_marker, clip.start_marker + pass),
    };
    if loop_end > clip.loop_content_end() {
        return None;
    }
    Some(UiClip {
        id: ClipId::new(),
        name: captured_clip_name(source, &clip.name),
        position: launch.destination_start_samples,
        start_marker: timeline.source_at(launch.phase_samples),
        duration: launch.played_samples,
        loop_enabled: true,
        loop_start,
        loop_end,
        ..clip.clone()
    })
}
