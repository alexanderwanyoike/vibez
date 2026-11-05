//! Routes View messages and their Browser/Perform resize side effects.

use crate::domains::browser::BrowserMsg;
use crate::domains::perform::PerformEditorFocus;
use crate::domains::timeline_editor::TimelineEditorAdapter;
use crate::domains::view::{ViewCtx, ViewMsg};
use crate::state::Workspace;

use super::*;

impl App {
    fn view_ctx(&self) -> ViewCtx {
        ViewCtx {
            total_beats: self.state.total_beats(),
            song_beats: self.state.song_beats(),
            lane_width: self.arrange_lane_width(),
            samples_per_beat: self.state.transport.samples_per_beat(),
        }
    }

    fn route_section_timeline_navigation(&mut self, msg: &ViewMsg) -> bool {
        if self.state.view.workspace != Workspace::Perform
            || self.state.perform.editor_focus != PerformEditorFocus::TimelineEditor
        {
            return false;
        }
        let Some(total_beats) = self
            .state
            .perform
            .selected_section
            .and_then(|id| self.state.perform.sections.by_id(id))
            .map(|section| section.length_beats)
        else {
            return false;
        };
        let viewport_width = self.section_timeline_viewport_width();
        let viewport = self.state.perform.section_editor.viewport_mut();
        if let ViewMsg::ZoomToFit = msg {
            viewport.frame(0.0, total_beats, viewport_width);
            return true;
        }
        msg.navigate(viewport, total_beats, viewport_width)
    }

    pub(super) fn route_view_message(&mut self, msg: ViewMsg) -> Task<Message> {
        if matches!(&msg, ViewMsg::ToggleEditMenu) {
            self.state.project.file_menu_open = false;
        }
        let browser_resize = match &msg {
            ViewMsg::CursorMoved(x, _) if self.state.browser.dock_resize_active => {
                Some(BrowserMsg::ResizeDock(
                    self.state
                        .browser
                        .dock_drag_width(*x, self.state.view.window_width),
                ))
            }
            ViewMsg::MouseReleased if self.state.browser.dock_resize_active => {
                Some(BrowserMsg::EndDockResize)
            }
            _ => None,
        };
        if let Some(browser_msg) = browser_resize {
            let action = self.state.browser.update(browser_msg);
            if action.persist_settings {
                self.persist_ui_settings();
            }
        }
        let detail_panel_resize = match &msg {
            ViewMsg::CursorMoved(_, y) if self.state.view.detail_panel_resize_active => Some(
                ViewMsg::ResizeDetailPanel(self.detail_panel_drag_height(*y)),
            ),
            ViewMsg::MouseReleased if self.state.view.detail_panel_resize_active => {
                Some(ViewMsg::EndDetailPanelResize)
            }
            _ => None,
        };
        if let Some(resize_msg) = detail_panel_resize {
            let ctx = self.view_ctx();
            let action = self.state.view.update(
                resize_msg,
                self.state.arrangement.resolve_timeline().editor,
                ctx,
            );
            if action.persist_settings {
                self.persist_ui_settings();
            }
        }
        let perform_surface_resize = match &msg {
            ViewMsg::CursorMoved(x, _) if self.state.view.perform_surface_resize_active => Some(
                ViewMsg::ResizePerformSurface(self.perform_surface_drag_width(*x)),
            ),
            ViewMsg::MouseReleased if self.state.view.perform_surface_resize_active => {
                Some(ViewMsg::EndPerformSurfaceResize)
            }
            _ => None,
        };
        if let Some(resize_msg) = perform_surface_resize {
            let ctx = self.view_ctx();
            let action = self.state.view.update(
                resize_msg,
                self.state.arrangement.resolve_timeline().editor,
                ctx,
            );
            if action.persist_settings {
                self.persist_ui_settings();
            }
        }
        let pending_drag_msg = match &msg {
            ViewMsg::CursorMoved(x, y) if self.state.browser.pending_drag.is_some() => {
                Some(BrowserMsg::PendingDragMoved { x: *x, y: *y })
            }
            ViewMsg::MouseReleased if self.state.browser.pending_drag.is_some() => {
                Some(BrowserMsg::EndDragSample)
            }
            ViewMsg::MouseReleased if self.state.browser.drag_source.is_some() => {
                Some(BrowserMsg::EndDragSample)
            }
            _ => None,
        };
        if let Some(browser_msg) = pending_drag_msg {
            let action = self.state.browser.update(browser_msg);
            if let Some(status) = action.status {
                self.state.status_text = status;
            }
        }
        if self.route_section_timeline_navigation(&msg) {
            return Task::none();
        }
        // Zoom to selection frames the Arrange view, which Perform hides.
        if self.state.view.workspace == Workspace::Perform
            && matches!(msg, ViewMsg::ZoomToSelection | ViewMsg::ZoomBack)
        {
            return Task::none();
        }
        let ctx = self.view_ctx();
        let editor = if self.state.view.workspace == Workspace::Perform
            && (self.state.perform.has_selected_timeline()
                || self.state.perform.layout == vibez_project::PerformLayout::Clips)
        {
            self.state.perform.timeline_editor()
        } else {
            self.state.arrangement.resolve_timeline().editor
        };
        let focus_name = self.state.view.workspace == Workspace::Perform
            && self.state.perform.layout == vibez_project::PerformLayout::Clips
            && matches!(
                msg,
                ViewMsg::StartEditingClipName(..) | ViewMsg::StartEditingTrackName { .. }
            );
        let action = self.state.view.update(msg, editor, ctx);
        let task = self.apply_view_action(action);
        if focus_name {
            let id = iced::widget::text_input::Id::new(
                super::views_clip_launcher::LAUNCHER_NAME_INPUT_ID,
            );
            Task::batch([
                task,
                iced::widget::text_input::focus(id.clone()),
                iced::widget::text_input::select_all(id),
            ])
        } else {
            task
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domains::perform::Section;
    use crate::timeline_geometry::FRAME_MARGIN_PIXELS;

    /// An app showing one Section's timeline in Perform, with the timeline
    /// focused.
    fn app_in_a_section() -> App {
        let mut app = super::super::test_support::app();
        app.state.view.workspace = Workspace::Perform;
        let section = Section::new(0);
        app.state.perform.selected_section = Some(section.id);
        std::sync::Arc::make_mut(&mut app.state.perform.sections).insert(section);
        app.state.perform.editor_focus = PerformEditorFocus::TimelineEditor;
        app
    }

    #[test]
    fn section_fit_frames_the_section_inside_the_lane_margin() {
        let mut app = app_in_a_section();
        let section_beats = app
            .state
            .perform
            .selected_section
            .and_then(|id| app.state.perform.sections.by_id(id))
            .map(|section| section.length_beats)
            .unwrap();

        let _ = app.update(Message::View(ViewMsg::ZoomToFit));

        let width = app.section_timeline_viewport_width();
        let end_x = app
            .state
            .perform
            .section_editor
            .viewport()
            .geometry()
            .beat_to_x(section_beats);
        assert!(
            end_x <= width - 2.0 * FRAME_MARGIN_PIXELS + 0.01,
            "the section ends at {end_x} of {width}"
        );
        assert!(end_x > width - 4.0 * FRAME_MARGIN_PIXELS, "ends at {end_x}");
    }

    #[test]
    fn z_and_x_in_a_section_leave_the_hidden_arrange_view_alone() {
        let mut app = app_in_a_section();
        let editor = app.state.perform.section_editor.editor_mut();
        editor.time_selection_active = true;
        editor.selection_start_beats = 4.0;
        editor.selection_end_beats = 8.0;
        let arrange = app.state.view.viewport;

        let _ = app.update(Message::View(ViewMsg::ZoomToSelection));
        assert_eq!(app.state.view.viewport, arrange);
        let _ = app.update(Message::View(ViewMsg::ZoomBack));
        assert_eq!(app.state.view.viewport, arrange);
    }
}
