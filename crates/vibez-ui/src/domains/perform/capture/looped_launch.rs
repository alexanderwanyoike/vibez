//! One looping Arrange clip per track for each launch of a looping source.

use vibez_core::id::ClipId;

use crate::state::{TrackTimelineContent, UiNoteClip};

use super::{captured_clip_name, captured_visible_notes, CapturedTimelineSource};

/// What one launch of a looping source played on the Arrange timeline.
#[derive(Debug, Clone, Copy)]
pub(super) struct LoopedLaunch {
    /// Where in the source pass the kept part of the launch began.
    pub phase_samples: u64,
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
    let pass_beats = source.length_beats;
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
    let beats = |samples: u64| samples as f64 / launch.samples_per_beat;
    let phase = beats(launch.phase_samples);
    Some(UiNoteClip {
        id: ClipId::new(),
        name,
        position_beats: beats(launch.destination_start_samples),
        duration_beats: beats(launch.played_samples),
        notes,
        selected_notes: Default::default(),
        start_marker_beats: if phase < pass_beats { phase } else { 0.0 },
        loop_enabled: true,
        loop_start_beats: 0.0,
        loop_end_beats: pass_beats,
        groove_grid: first.groove_grid,
    })
}
