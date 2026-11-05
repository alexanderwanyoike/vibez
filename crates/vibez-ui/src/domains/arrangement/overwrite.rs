//! Clips placed on a lane overwrite the Clips already under them, like
//! Ableton: covered parts are trimmed away, split around or removed.

use std::collections::HashSet;

use vibez_core::id::{ClipId, TrackId};
use vibez_engine::commands::EngineCommand;

use super::clip_trims::{audio_start_trimmed, note_start_trimmed};
use super::fragment_geometry::audio_fragment;
use super::{ArrangementAction, ArrangementCtx, EngineHandle};
use crate::state::{
    ArrangementSelection, TimelineEditorState, TrackTimelineContent, UiClip, UiNoteClip,
    SHORTEST_LENGTH_BEATS,
};

const BEAT_EPSILON: f64 = 1e-9;

/// What is left of each lane Clip that an incoming range touched. A Clip
/// listed with no pieces is fully covered.
#[derive(Debug, Default)]
pub(crate) struct LaneOverwrite {
    pub(crate) audio: Vec<(ClipId, Vec<UiClip>)>,
    pub(crate) notes: Vec<(ClipId, Vec<UiNoteClip>)>,
}

/// The edits that placing `placed` over `start_beats..end_beats` makes to
/// the other Clips on `lane`. A piece shorter than the shortest Clip is
/// dropped, since it could be neither seen nor grabbed.
pub(crate) fn lane_overwrite(
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

impl TimelineEditorState {
    pub(super) fn op_drop_clips(
        &mut self,
        engine: &mut impl EngineHandle,
        ctx: ArrangementCtx,
        anchor: ClipId,
    ) -> ArrangementAction {
        let Some(anchor) = self.locate_clip(anchor) else {
            return ArrangementAction::default();
        };
        let dropped = self.gesture_targets(anchor);
        ArrangementAction {
            mark_dirty: self.overwrite_under(engine, &dropped, ctx.samples_per_beat),
            ..Default::default()
        }
    }

    /// Each of `placed` overwrites the other Clips under it on its lane.
    /// Returns whether any Clip was overwritten.
    pub(super) fn overwrite_under(
        &mut self,
        engine: &mut impl EngineHandle,
        placed: &[ArrangementSelection],
        samples_per_beat: f64,
    ) -> bool {
        let ranges: Vec<_> = placed
            .iter()
            .filter_map(|selection| {
                let track_id = selection.track_id();
                let span = self.clip_span(*selection, samples_per_beat)?;
                let on_lane: HashSet<ClipId> = placed
                    .iter()
                    .filter(|other| other.track_id() == track_id)
                    .map(|other| other.clip_id())
                    .collect();
                Some((track_id, (span.start_beats, span.end_beats()), on_lane))
            })
            .collect();
        self.overwrite_ranges(engine, ranges, samples_per_beat)
    }

    /// Clears each `(lane, range)` of every Clip but the ones it keeps.
    /// Returns whether any Clip was overwritten.
    fn overwrite_ranges(
        &mut self,
        engine: &mut impl EngineHandle,
        ranges: Vec<(TrackId, (f64, f64), HashSet<ClipId>)>,
        samples_per_beat: f64,
    ) -> bool {
        if samples_per_beat <= 0.0 {
            return false;
        }
        let mut changed = false;
        for (track_id, range, kept) in ranges {
            let Some(lane) = self.find_content(track_id) else {
                continue;
            };
            let edits = lane_overwrite(lane, range, &kept, samples_per_beat);
            let edits_lane = !edits.audio.is_empty() || !edits.notes.is_empty();
            if edits_lane && !changed {
                engine.send(EngineCommand::BeginContentRebuild);
            }
            for (clip_id, pieces) in edits.audio {
                if !pieces.iter().any(|piece| piece.id == clip_id) {
                    self.selected_clips
                        .remove(&ArrangementSelection::AudioClip { track_id, clip_id });
                }
                self.replace_audio_clip(engine, track_id, clip_id, pieces);
                changed = true;
            }
            for (clip_id, pieces) in edits.notes {
                if !pieces.iter().any(|piece| piece.id == clip_id) {
                    self.selected_clips
                        .remove(&ArrangementSelection::NoteClip { track_id, clip_id });
                    if self.selected_note_clip == Some((track_id, clip_id)) {
                        self.selected_note_clip = None;
                    }
                }
                self.replace_note_clip(engine, track_id, clip_id, pieces);
                changed = true;
            }
        }
        if changed {
            engine.send(EngineCommand::EndContentRebuild);
        }
        changed
    }
}
