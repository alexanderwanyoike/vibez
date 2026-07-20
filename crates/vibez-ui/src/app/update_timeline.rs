//! Routes shared editor messages to the active Arrange or Section adapter.

use std::sync::Arc;

use crate::domains::arrangement::{ArrangementAction, ArrangementCtx, ArrangementMsg};
use crate::domains::automation::{AutomationAction, AutomationMsg};
use crate::domains::piano_roll::{PianoRollAction, PianoRollCtx, PianoRollMsg};
use crate::domains::timeline_editor::TimelineEditorAdapter;
use crate::state::Workspace;

use super::*;

impl App {
    pub(super) fn route_automation_editor_message(
        &mut self,
        msg: AutomationMsg,
    ) -> AutomationAction {
        let section_content_changed = msg.marks_dirty();
        let editing_section = self.state.view.workspace == Workspace::Perform
            && self.state.perform.selected_section.is_some();
        let action = if editing_section {
            let mut engine = crate::domains::DiscardingEngine;
            self.state.automation_ui.update(
                msg,
                &mut engine,
                Arc::make_mut(&mut self.state.project_tracks),
                self.state.perform.section_editor.editor_mut(),
            )
        } else {
            let mut engine = crate::domains::EngineTx(&mut self.cmd_tx);
            self.state.automation_ui.update(
                msg,
                &mut engine,
                Arc::make_mut(&mut self.state.project_tracks),
                self.state.arrangement.resolve_timeline_mut().editor,
            )
        };
        if editing_section {
            self.state.perform.commit_selected_section_timeline();
            if section_content_changed {
                if let Some(section_id) = self.state.perform.selected_section {
                    self.refresh_playing_section_after_edit(section_id);
                }
            }
        }
        action
    }

    pub(super) fn route_arrangement_editor_message(
        &mut self,
        msg: ArrangementMsg,
        ctx: ArrangementCtx,
    ) -> ArrangementAction {
        let section_content_changed = msg.marks_dirty();
        let editing_section = self.state.view.workspace == Workspace::Perform
            && self.state.perform.selected_section.is_some();
        if editing_section {
            if let ArrangementMsg::SelectTrack(track_id) = &msg {
                self.state.arrangement.selected_track = Some(*track_id);
                self.state
                    .perform
                    .section_editor
                    .editor_mut()
                    .selected_track = Some(*track_id);
                self.state.perform.sync_instrument_target(Some(*track_id));
                return ArrangementAction::default();
            }
        }
        if editing_section && msg.is_timeline_editor_message() {
            let mut engine = crate::domains::DiscardingEngine;
            let action = self.state.perform.section_editor.editor_mut().update(
                Arc::make_mut(&mut self.state.project_tracks),
                msg,
                &mut engine,
                ctx,
            );
            self.state.perform.commit_selected_section_timeline();
            if section_content_changed {
                if let Some(section_id) = self.state.perform.selected_section {
                    self.refresh_playing_section_after_edit(section_id);
                }
            }
            if let Some(track_id) = self.state.perform.section_editor.editor().selected_track {
                self.state.arrangement.selected_track = Some(track_id);
            }
            self.state
                .perform
                .sync_instrument_target(self.state.arrangement.selected_track);
            action
        } else {
            let mut engine = crate::domains::EngineTx(&mut self.cmd_tx);
            let action = self.state.arrangement.update(
                Arc::make_mut(&mut self.state.project_tracks),
                msg,
                &mut engine,
                ctx,
            );
            self.state
                .perform
                .sync_instrument_target(self.state.arrangement.selected_track);
            action
        }
    }

    pub(super) fn route_piano_roll_editor_message(
        &mut self,
        msg: PianoRollMsg,
        ctx: PianoRollCtx,
    ) -> PianoRollAction {
        let section_content_changed = msg.marks_dirty();
        let editing_section = self.state.view.workspace == Workspace::Perform
            && self.state.perform.selected_section.is_some();
        if editing_section {
            if let PianoRollMsg::AddNoteClipToTrack(track_id) = &msg {
                let midi_track = self
                    .state
                    .project_tracks
                    .tracks
                    .iter()
                    .any(|track| track.id == *track_id && track.kind.is_midi());
                if !midi_track {
                    self.state.status_text = "MIDI clips require a MIDI Project Track".into();
                    return PianoRollAction::default();
                }
            }
            let mut engine = crate::domains::DiscardingEngine;
            let action = self.state.piano_roll.update(
                msg,
                &mut engine,
                self.state.perform.section_editor.editor_mut(),
                ctx,
            );
            self.state.perform.commit_selected_section_timeline();
            if section_content_changed {
                if let Some(section_id) = self.state.perform.selected_section {
                    self.refresh_playing_section_after_edit(section_id);
                }
            }
            action
        } else {
            let mut engine = crate::domains::EngineTx(&mut self.cmd_tx);
            self.state.piano_roll.update(
                msg,
                &mut engine,
                self.state.arrangement.resolve_timeline_mut().editor,
                ctx,
            )
        }
    }
}
