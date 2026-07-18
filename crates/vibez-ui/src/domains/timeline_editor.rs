//! Shared Timeline Editor adapter boundary.
//!
//! Adapters resolve their timeline once. Clip, note, automation, selection,
//! and view code consume that resolved target without knowing whether it came
//! from Arrange or, later, a Section.

#[cfg(test)]
use crate::state::TimelineEditorState;
use crate::state::{ArrangementState, ResolvedTimeline, ResolvedTimelineMut};

pub trait TimelineEditorAdapter {
    fn resolve_timeline(&self) -> ResolvedTimeline<'_>;
    fn resolve_timeline_mut(&mut self) -> ResolvedTimelineMut<'_>;
}

impl TimelineEditorAdapter for ArrangementState {
    fn resolve_timeline(&self) -> ResolvedTimeline<'_> {
        ResolvedTimeline {
            editor: &self.editor,
        }
    }

    fn resolve_timeline_mut(&mut self) -> ResolvedTimelineMut<'_> {
        ResolvedTimelineMut {
            editor: &mut self.editor,
        }
    }
}

#[cfg(test)]
pub fn resolved_editor_mut(adapter: &mut impl TimelineEditorAdapter) -> &mut TimelineEditorState {
    adapter.resolve_timeline_mut().editor
}

#[cfg(test)]
pub(crate) mod conformance {
    use std::sync::Arc;

    use vibez_core::automation::AutomationTarget;
    use vibez_core::id::TrackId;
    use vibez_core::midi::TrackKind;

    use super::*;
    use crate::domains::arrangement::{ArrangementCtx, ArrangementMsg};
    use crate::domains::automation::{AutomationMsg, AutomationState};
    use crate::domains::piano_roll::{PianoRollCtx, PianoRollMsg};
    use crate::domains::test_support::RecordingEngine;
    use crate::state::{PianoRollState, ProjectTrack, ProjectTracksState};

    /// Reusable contract: Card 07 calls this same harness for its Section
    /// adapter rather than building a parallel editing implementation.
    pub(crate) fn assert_timeline_editor_conformance<A>(mut adapter: A)
    where
        A: TimelineEditorAdapter,
    {
        let track_id = TrackId::new();
        let mut project_tracks = ProjectTracksState::default();
        project_tracks.tracks.push(ProjectTrack::new_instrument(
            track_id,
            "MIDI 1".into(),
            TrackKind::Midi,
            0,
        ));
        let mut engine = RecordingEngine::default();

        let clip_id = {
            let editor = resolved_editor_mut(&mut adapter);
            Arc::make_mut(&mut editor.timeline).ensure(track_id);
            editor.selected_track = Some(track_id);
            editor.time_selection_active = true;
            editor.selection_start_beats = 4.0;
            editor.selection_end_beats = 8.0;
            editor.update(
                &mut project_tracks,
                ArrangementMsg::CreateNoteClipFromSelection(track_id),
                &mut engine,
                ArrangementCtx::default(),
            );
            editor.selected_note_clip.expect("created note clip").1
        };

        let mut piano_roll = PianoRollState::default();
        piano_roll.update(
            PianoRollMsg::AddNote {
                track_id,
                clip_id,
                pitch: 60,
                start_beat: 0.0,
                duration_beats: 1.0,
            },
            &mut engine,
            resolved_editor_mut(&mut adapter),
            PianoRollCtx::default(),
        );
        resolved_editor_mut(&mut adapter).update(
            &mut project_tracks,
            ArrangementMsg::MoveNoteClipPosition {
                track_id,
                clip_id,
                new_position_beats: 12.0,
            },
            &mut engine,
            ArrangementCtx::default(),
        );

        let mut automation = AutomationState::default();
        automation.update(
            AutomationMsg::AddLane {
                track_id,
                target: AutomationTarget::TrackGain,
            },
            &mut engine,
            &mut project_tracks,
            resolved_editor_mut(&mut adapter),
        );

        let resolved = adapter.resolve_timeline();
        let content = resolved
            .editor
            .timeline
            .get(track_id)
            .expect("resolved track content");
        let clip = content
            .note_clips
            .iter()
            .find(|clip| clip.id == clip_id)
            .expect("shared note clip");
        assert_eq!(clip.position_beats, 12.0);
        assert_eq!(clip.notes.len(), 1);
        assert_eq!(content.automation.len(), 1);
    }

    #[test]
    fn arrange_adapter_satisfies_the_shared_editor_contract() {
        assert_timeline_editor_conformance(ArrangementState::default());
    }
}
