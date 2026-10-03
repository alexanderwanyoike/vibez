//! Shared routing for background edits to Arrange, Sections and launcher Clips.

use std::sync::Arc;

use vibez_engine::commands::EngineCommand;
use vibez_project::TimelineLocation;

use crate::domains::arrangement::ArrangementAction;
use crate::domains::timeline_editor::TimelineEditorAdapter;
use crate::domains::{EngineCommandQueue, EngineHandle};
use crate::state::{ProjectTracksState, TimelineEditorState};

use super::*;

pub(super) enum TimelineResultEngine<'a> {
    Arrange(&'a mut EngineCommandQueue),
    NonResident,
}

impl EngineHandle for TimelineResultEngine<'_> {
    fn send(&mut self, command: EngineCommand) {
        if let Self::Arrange(queue) = self {
            queue.send(command);
        }
    }
}

impl App {
    pub(super) fn with_timeline_editor_at(
        &mut self,
        location: TimelineLocation,
        apply: impl FnOnce(
            &mut TimelineEditorState,
            &mut ProjectTracksState,
            &mut TimelineResultEngine<'_>,
        ) -> ArrangementAction,
    ) -> ArrangementAction {
        match location {
            TimelineLocation::LauncherClip(id) => {
                let mut engine = TimelineResultEngine::NonResident;
                if self.state.perform.clip_editor.selected == Some(id) {
                    let action = apply(
                        &mut self.state.perform.clip_editor.editor,
                        Arc::make_mut(&mut self.state.project_tracks),
                        &mut engine,
                    );
                    self.state.perform.commit_selected_timeline();
                    action
                } else if let Some(clip) =
                    Arc::make_mut(&mut self.state.perform.clips).by_id_mut(id)
                {
                    let mut editor = TimelineEditorState {
                        timeline: Arc::clone(&clip.timeline),
                        ..Default::default()
                    };
                    let action = apply(
                        &mut editor,
                        Arc::make_mut(&mut self.state.project_tracks),
                        &mut engine,
                    );
                    clip.timeline = editor.timeline;
                    action
                } else {
                    ArrangementAction::default()
                }
            }
            TimelineLocation::Arrange => {
                let mut engine = TimelineResultEngine::Arrange(&mut self.cmd_tx);
                apply(
                    self.state.arrangement.resolve_timeline_mut().editor,
                    Arc::make_mut(&mut self.state.project_tracks),
                    &mut engine,
                )
            }
            TimelineLocation::Section(section_id)
                if self.state.perform.selected_section == Some(section_id) =>
            {
                let mut engine = TimelineResultEngine::NonResident;
                let action = apply(
                    self.state.perform.section_editor.editor_mut(),
                    Arc::make_mut(&mut self.state.project_tracks),
                    &mut engine,
                );
                self.state.perform.commit_selected_timeline();
                if action.mark_dirty {
                    self.refresh_playing_section_after_edit(section_id);
                }
                action
            }
            TimelineLocation::Section(section_id) => {
                let action = {
                    let project_tracks = Arc::make_mut(&mut self.state.project_tracks);
                    let Some(section) =
                        Arc::make_mut(&mut self.state.perform.sections).by_id_mut(section_id)
                    else {
                        return ArrangementAction::default();
                    };
                    let mut editor = TimelineEditorState {
                        timeline: Arc::clone(&section.timeline),
                        ..TimelineEditorState::default()
                    };
                    let mut engine = TimelineResultEngine::NonResident;
                    let action = apply(&mut editor, project_tracks, &mut engine);
                    section.timeline = editor.timeline;
                    action
                };
                if action.mark_dirty {
                    self.refresh_playing_section_after_edit(section_id);
                }
                action
            }
        }
    }
}
