//! Routes shared editor messages to the active Arrange or Section adapter.

use std::sync::Arc;

use crate::domains::arrangement::{ArrangementAction, ArrangementCtx, ArrangementMsg};
use crate::domains::automation::{AutomationAction, AutomationMsg};
use crate::domains::perform::PerformEditorFocus;
use crate::domains::piano_roll::{PianoRollAction, PianoRollCtx, PianoRollMsg};
use crate::domains::timeline_editor::TimelineEditorAdapter;
use crate::state::Workspace;

use super::*;

fn clipboard_targets_section(
    workspace: Workspace,
    selected_section: bool,
    focus: PerformEditorFocus,
) -> bool {
    workspace == Workspace::Perform
        && selected_section
        && focus == PerformEditorFocus::SectionConstruction
}

pub(super) fn section_timeline_claims_focus(workspace: Workspace, selected_section: bool) -> bool {
    workspace == Workspace::Perform && selected_section
}

fn focused_timeline_playhead_beats(
    workspace: Workspace,
    selected_section: bool,
    focus: PerformEditorFocus,
    arrange_playhead_beats: f64,
    section_playhead_beats: f64,
) -> f64 {
    if clipboard_targets_section(workspace, selected_section, focus) {
        section_playhead_beats
    } else {
        arrange_playhead_beats
    }
}

fn focused_section_seek_beat(
    workspace: Workspace,
    selected_section: bool,
    focus: PerformEditorFocus,
    msg: &crate::domains::transport::TransportMsg,
) -> Option<f64> {
    let crate::domains::transport::TransportMsg::SeekToBeat(beat) = msg else {
        return None;
    };
    clipboard_targets_section(workspace, selected_section, focus).then(|| beat.max(0.0))
}

impl App {
    pub(super) fn focused_editor_is_section(&self) -> bool {
        clipboard_targets_section(
            self.state.view.workspace,
            self.state.perform.selected_section.is_some(),
            self.state.perform.editor_focus,
        )
    }

    pub(super) fn focused_editor_playhead_beats(&self) -> f64 {
        focused_timeline_playhead_beats(
            self.state.view.workspace,
            self.state.perform.selected_section.is_some(),
            self.state.perform.editor_focus,
            self.state.position_beats(),
            self.state.perform.section_editor.edit_cursor_beats(),
        )
    }

    pub(super) fn place_focused_section_playhead(
        &mut self,
        msg: &crate::domains::transport::TransportMsg,
    ) -> bool {
        let Some(beat) = focused_section_seek_beat(
            self.state.view.workspace,
            self.state.perform.selected_section.is_some(),
            self.state.perform.editor_focus,
            msg,
        ) else {
            return false;
        };
        self.state.perform.section_editor.place_edit_cursor(beat);
        true
    }

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
        let clipboard_message = msg.is_clipboard_message();
        let section_content_changed = msg.marks_dirty();
        let editing_section = if clipboard_message {
            clipboard_targets_section(
                self.state.view.workspace,
                self.state.perform.selected_section.is_some(),
                self.state.perform.editor_focus,
            )
        } else {
            self.state.view.workspace == Workspace::Perform
                && self.state.perform.selected_section.is_some()
        };
        if editing_section {
            if let ArrangementMsg::SelectTrack(track_id) = &msg {
                self.state.arrangement.selected_track = Some(*track_id);
                self.state
                    .perform
                    .section_editor
                    .editor_mut()
                    .selected_track = Some(*track_id);
                self.state.perform.sync_instrument_target_from_selection(
                    Some(*track_id),
                    &self.state.project_tracks.tracks,
                );
                return ArrangementAction::default();
            }
        }
        if clipboard_message {
            let action = if editing_section {
                let mut engine = crate::domains::DiscardingEngine;
                self.state
                    .perform
                    .section_editor
                    .editor_mut()
                    .update_clipboard(
                        &self.state.project_tracks,
                        msg,
                        &mut self.state.clip_clipboard,
                        &mut engine,
                        ctx,
                    )
            } else {
                let mut engine = crate::domains::EngineTx(&mut self.cmd_tx);
                self.state.arrangement.editor.update_clipboard(
                    &self.state.project_tracks,
                    msg,
                    &mut self.state.clip_clipboard,
                    &mut engine,
                    ctx,
                )
            };
            if editing_section {
                self.state.perform.commit_selected_section_timeline();
                if action.mark_dirty {
                    if let Some(section_id) = self.state.perform.selected_section {
                        self.refresh_playing_section_after_edit(section_id);
                    }
                }
                if let Some(track_id) = self.state.perform.section_editor.editor().selected_track {
                    self.state.arrangement.selected_track = Some(track_id);
                }
            }
            self.state.perform.sync_instrument_target_from_selection(
                self.state.arrangement.selected_track,
                &self.state.project_tracks.tracks,
            );
            return action;
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
            self.state.perform.sync_instrument_target_from_selection(
                self.state.arrangement.selected_track,
                &self.state.project_tracks.tracks,
            );
            action
        } else {
            let mut engine = crate::domains::EngineTx(&mut self.cmd_tx);
            let action = self.state.arrangement.update(
                Arc::make_mut(&mut self.state.project_tracks),
                msg,
                &mut engine,
                ctx,
            );
            self.state.perform.sync_instrument_target_from_selection(
                self.state.arrangement.selected_track,
                &self.state.project_tracks.tracks,
            );
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

#[cfg(test)]
mod clipboard_focus_tests {
    use super::*;

    #[test]
    fn clipboard_shortcuts_follow_editor_focus_not_perform_playback() {
        assert!(clipboard_targets_section(
            Workspace::Perform,
            true,
            PerformEditorFocus::SectionConstruction,
        ));
        assert!(!clipboard_targets_section(
            Workspace::Perform,
            true,
            PerformEditorFocus::PadSurface,
        ));
        assert!(!clipboard_targets_section(
            Workspace::Arrange,
            true,
            PerformEditorFocus::SectionConstruction,
        ));
        assert!(!clipboard_targets_section(
            Workspace::Perform,
            false,
            PerformEditorFocus::SectionConstruction,
        ));
    }

    #[test]
    fn paste_playhead_follows_arrange_or_selected_section_editor_focus() {
        assert_eq!(
            focused_timeline_playhead_beats(
                Workspace::Arrange,
                true,
                PerformEditorFocus::SectionConstruction,
                5.0,
                11.0,
            ),
            5.0
        );
        assert_eq!(
            focused_timeline_playhead_beats(
                Workspace::Perform,
                true,
                PerformEditorFocus::SectionConstruction,
                5.0,
                11.0,
            ),
            11.0
        );
        assert_eq!(
            focused_timeline_playhead_beats(
                Workspace::Perform,
                true,
                PerformEditorFocus::PadSurface,
                5.0,
                11.0,
            ),
            5.0,
            "playing a Section must not redirect Arrange-focused paste"
        );
    }

    #[test]
    fn pointer_seek_places_only_the_focused_section_edit_cursor() {
        use crate::domains::transport::TransportMsg;

        assert_eq!(
            focused_section_seek_beat(
                Workspace::Perform,
                true,
                PerformEditorFocus::SectionConstruction,
                &TransportMsg::SeekToBeat(13.0),
            ),
            Some(13.0)
        );
        assert_eq!(
            focused_section_seek_beat(
                Workspace::Perform,
                true,
                PerformEditorFocus::PadSurface,
                &TransportMsg::SeekToBeat(13.0),
            ),
            None
        );
        assert_eq!(
            focused_section_seek_beat(
                Workspace::Perform,
                true,
                PerformEditorFocus::SectionConstruction,
                &TransportMsg::Play,
            ),
            None,
            "hover and unrelated transport messages do not move the cursor"
        );
    }

    #[test]
    fn section_timeline_messages_claim_focus_only_for_a_visible_selected_section() {
        assert!(section_timeline_claims_focus(Workspace::Perform, true));
        assert!(!section_timeline_claims_focus(Workspace::Arrange, true));
        assert!(!section_timeline_claims_focus(Workspace::Perform, false));
    }
}
