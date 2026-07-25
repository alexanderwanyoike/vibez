//! Routes View messages and their Browser/Perform resize side effects.

use crate::domains::browser::BrowserMsg;
use crate::domains::perform::PerformEditorFocus;
use crate::domains::timeline_editor::TimelineEditorAdapter;
use crate::domains::view::ViewMsg;
use crate::state::Workspace;
use crate::timeline_geometry::{TimelineGeometry, BASE_PIXELS_PER_BEAT};

use super::*;

impl App {
    fn route_section_timeline_navigation(&mut self, msg: &ViewMsg) -> bool {
        if self.state.view.workspace != Workspace::Perform
            || self.state.perform.editor_focus != PerformEditorFocus::SectionConstruction
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
        match msg {
            ViewMsg::ZoomIn => {
                viewport.zoom_around(1.25, viewport_width / 2.0, total_beats, viewport_width)
            }
            ViewMsg::ZoomOut => viewport.zoom_around(
                1.0 / 1.25,
                viewport_width / 2.0,
                total_beats,
                viewport_width,
            ),
            ViewMsg::SetZoom(level) => {
                let factor = level.clamp(0.01, 16.0) / viewport.zoom_level;
                viewport.zoom_around(factor, viewport_width / 2.0, total_beats, viewport_width);
            }
            ViewMsg::ZoomAround { factor, anchor_x } => {
                viewport.zoom_around(*factor, *anchor_x, total_beats, viewport_width);
            }
            ViewMsg::ZoomToFit => {
                let target_ppb =
                    TimelineGeometry::fitted(total_beats, viewport_width, 0.0).pixels_per_beat();
                viewport.zoom_level = (target_ppb / BASE_PIXELS_PER_BEAT).clamp(0.01, 16.0);
                viewport.scroll_offset_beats = 0.0;
                viewport.clamp(total_beats, viewport_width);
            }
            ViewMsg::ScrollArrangement(delta) => {
                viewport.scroll_by(*delta, total_beats, viewport_width);
            }
            _ => return false,
        }
        true
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
            let ctx = crate::domains::view::ViewCtx {
                total_beats: self.state.total_beats(),
            };
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
            let ctx = crate::domains::view::ViewCtx {
                total_beats: self.state.total_beats(),
            };
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
        let ctx = crate::domains::view::ViewCtx {
            total_beats: self.state.total_beats(),
        };
        let action =
            self.state
                .view
                .update(msg, self.state.arrangement.resolve_timeline().editor, ctx);
        self.apply_view_action(action)
    }
}
