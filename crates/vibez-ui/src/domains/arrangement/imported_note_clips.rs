//! Imported note Clips joining their timeline lanes.

use std::sync::Arc;

use vibez_core::id::TrackId;

use super::ArrangementAction;
use crate::domains::EngineHandle;
use crate::state::{TimelineEditorState, UiNoteClip};

impl TimelineEditorState {
    /// Adds each Clip to its lane, its engine insertion sized for the Clips
    /// already there.
    pub fn add_imported_note_clips(
        &mut self,
        engine: &mut impl EngineHandle,
        clips: Vec<(TrackId, UiNoteClip)>,
    ) -> ArrangementAction {
        let timeline = Arc::make_mut(&mut self.timeline);
        for (track_id, clip) in clips {
            let content = timeline.ensure(track_id);
            engine.send(clip.add_command_beside(track_id, content.note_clips.len()));
            content.note_clips.push(clip);
        }
        ArrangementAction {
            mark_dirty: true,
            ..ArrangementAction::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domains::test_support::RecordingEngine;
    use vibez_core::id::ClipId;
    use vibez_engine::commands::EngineCommand;

    fn clip() -> UiNoteClip {
        UiNoteClip {
            id: ClipId::new(),
            name: "Rebeater P3".into(),
            position_beats: 0.0,
            duration_beats: 4.0,
            notes: Vec::new(),
            selected_notes: Default::default(),
            start_marker_beats: 0.0,
            loop_enabled: true,
            loop_start_beats: 0.0,
            loop_end_beats: 4.0,
            groove_grid: Default::default(),
        }
    }

    #[test]
    fn each_clip_joins_its_lane_and_reaches_the_engine_once() {
        let kick = TrackId::new();
        let hats = TrackId::new();
        let mut editor = TimelineEditorState::default();
        Arc::make_mut(&mut editor.timeline)
            .ensure(kick)
            .note_clips
            .push(clip());
        let mut engine = RecordingEngine(Vec::new());

        let action =
            editor.add_imported_note_clips(&mut engine, vec![(kick, clip()), (hats, clip())]);

        assert!(action.mark_dirty);
        assert_eq!(editor.timeline.get(kick).unwrap().note_clips.len(), 2);
        assert_eq!(editor.timeline.get(hats).unwrap().note_clips.len(), 1);
        let lanes: Vec<_> = engine
            .0
            .iter()
            .filter_map(|command| match command {
                EngineCommand::AddNoteClip { track_id, .. } => Some(*track_id),
                _ => None,
            })
            .collect();
        assert_eq!(lanes, vec![kick, hats]);
    }
}
