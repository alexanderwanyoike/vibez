//! Routes View messages and their Browser/Perform resize side effects.

use crate::domains::browser::BrowserMsg;
use crate::domains::timeline_editor::TimelineEditorAdapter;
use crate::domains::view::ViewMsg;

use super::*;

impl App {
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
