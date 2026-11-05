//! Duplicate places a copy of the selection right after itself, as one block.

use super::{ArrangementAction, ArrangementCtx, EngineHandle};
use crate::state::{ProjectTracksState, TimelineEditorState};

impl TimelineEditorState {
    pub(super) fn op_duplicate_selection(
        &mut self,
        project_tracks: &ProjectTracksState,
        engine: &mut impl EngineHandle,
        ctx: ArrangementCtx,
    ) -> ArrangementAction {
        let spb = ctx.samples_per_beat;
        if spb <= 0.0 {
            return ArrangementAction {
                status: Some("Can't duplicate clips without a valid project tempo".to_string()),
                ..Default::default()
            };
        }
        // A box over several lanes records only the lane it started on, but
        // it selected every clip it touched, so those clips name its lanes.
        let entries = self.collect_selected_clips(project_tracks, spb, |lane| {
            self.time_selection_track
                .is_none_or(|target| lane == target)
                || self
                    .selected_clips
                    .iter()
                    .any(|selection| selection.track_id() == lane)
        });
        let ranged =
            self.time_selection_active && self.selection_end_beats > self.selection_start_beats;
        let span = self.selection_span_beats(spb);
        let Some((start, end)) = span.filter(|_| !entries.is_empty()) else {
            return ArrangementAction::default();
        };

        let selected = self.place_clips(
            entries.into_iter().map(|entry| {
                let track_id = entry.source_track();
                (entry, track_id)
            }),
            start,
            end,
            engine,
            spb,
        );
        if ranged {
            self.selection_start_beats = end;
            self.selection_end_beats = end + (end - start);
        }
        let count = selected.len();
        self.selected_clips = selected;
        ArrangementAction {
            status: Some(if count == 1 {
                "Duplicated clip".to_string()
            } else {
                format!("Duplicated {count} clips")
            }),
            mark_dirty: true,
            ..Default::default()
        }
    }
}
