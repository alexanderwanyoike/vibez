//! The preview a Clip drag shows while it is in flight, and putting the
//! selection back when one is cancelled.

use vibez_core::time::MusicalTime;

use crate::state::{
    ClipDrag, ClipDragKind, ClipDragPreview, ClipDragStep, ClipEdge, EditorSelection,
    ProjectTracksState, TimelineEditorState, UndoGestureId,
};

use super::{ArrangementAction, ArrangementCtx, ArrangementMsg, EngineHandle};

impl TimelineEditorState {
    pub(super) fn drag_clips(
        &mut self,
        project_tracks: &mut ProjectTracksState,
        engine: &mut impl EngineHandle,
        ctx: ArrangementCtx,
        step: ClipDragStep,
        edit: ArrangementMsg,
    ) -> ArrangementAction {
        let selection_before = match self.clip_drag.take() {
            Some(drag) if drag.preview.step.gesture == step.gesture => drag.selection_before,
            _ => EditorSelection {
                clips: self.selected_clips.clone(),
                track: self.selected_track,
                note_clip: self.selected_note_clip,
            },
        };
        let action = self.update(project_tracks, edit, engine, ctx);
        let readout = self.drag_readout(&step, project_tracks, ctx.samples_per_beat);
        self.clip_drag = Some(ClipDrag {
            preview: ClipDragPreview { step, readout },
            selection_before,
        });
        action
    }

    /// Ends the Clip drag in flight and puts back the selection it began
    /// with, returning its gesture for the caller to revert.
    pub fn cancel_clip_drag(&mut self) -> Option<UndoGestureId> {
        let drag = self.clip_drag.take()?;
        self.selected_clips = drag.selection_before.clips;
        self.selected_track = drag.selection_before.track;
        self.selected_note_clip = drag.selection_before.note_clip;
        Some(drag.preview.step.gesture)
    }

    fn drag_readout(
        &self,
        step: &ClipDragStep,
        project_tracks: &ProjectTracksState,
        samples_per_beat: f64,
    ) -> String {
        let Some((anchor, span)) = self.locate_clip(step.anchor).and_then(|anchor| {
            self.clip_span(anchor, samples_per_beat)
                .map(|span| (anchor, span))
        }) else {
            return String::new();
        };
        let start = MusicalTime(span.start_beats).bar_beat_sixteenth();
        match step.kind {
            ClipDragKind::Move if anchor.track_id() == step.origin_track => start,
            ClipDragKind::Move => {
                let lane = project_tracks
                    .tracks
                    .iter()
                    .find(|track| track.id == anchor.track_id())
                    .map_or("", |track| track.name.as_str());
                format!("{start} to {lane}")
            }
            ClipDragKind::Resize(ClipEdge::Start) => format!("Start {start}"),
            ClipDragKind::Resize(ClipEdge::End) => format!(
                "Length {}",
                MusicalTime(span.length_beats).bars_beats_sixteenths()
            ),
        }
    }
}
