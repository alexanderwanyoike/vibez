//! View domain: how the project is being looked at. Workspace and
//! detail-panel tabs, timeline zoom/scroll, snap grid, the
//! right-click context menu, cursor/window tracking, and inline
//! renames. Nothing in here is part of the project itself.

use vibez_core::id::{ClipId, TrackId};

use crate::state::{
    ArrangementTimeline, ContextMenuTarget, DetailPanelTab, SnapGrid, ViewState, Workspace,
};

/// Messages the view domain handles.
#[derive(Debug, Clone)]
pub enum ViewMsg {
    SwitchWorkspace(Workspace),
    SwitchDetailTab(DetailPanelTab),
    ZoomIn,
    ZoomOut,
    SetZoom(f32),
    ZoomToFit,
    ScrollArrangement(f64),
    SetSnapGrid(SnapGrid),
    NarrowGrid,
    WidenGrid,
    ToggleTripletGrid,
    ToggleSnapToGrid,
    ToggleAdaptiveGrid,
    CursorMoved(f32, f32),
    WindowResized(f32, f32),
    MouseReleased,
    ShowContextMenu {
        x: f32,
        y: f32,
        target: ContextMenuTarget,
    },
    DismissContextMenu,
    ToggleEditMenu,
    DismissEditMenu,
    StartEditingTrackName {
        track_id: TrackId,
        name: String,
    },
    StartEditingClipName(TrackId, ClipId),
    EditNameText(String),
    FinishEditing,
    CancelEditing,
}

/// A committed inline rename, for the router to replay through the
/// arrangement domain.
#[derive(Debug, Clone, PartialEq)]
pub enum RenameRequest {
    Track(TrackId, String),
    Clip(TrackId, ClipId, String),
}

/// Read-only cross-domain facts for view updates.
#[derive(Debug, Clone, Copy, Default)]
pub struct ViewCtx {
    /// Total arrangement length in beats (zoom-to-fit, scroll clamp).
    pub total_beats: f64,
}

/// Cross-domain effects requested by a view update.
#[derive(Debug, Default, PartialEq)]
pub struct ViewAction {
    /// Right-clicking a clip also selects it (arrangement owns
    /// selection): (track, clip, is_note_clip).
    pub select_clip: Option<(TrackId, ClipId, bool)>,
    /// An inline rename was committed.
    pub rename: Option<RenameRequest>,
    /// A mouse release ends any clip-resize drag.
    pub end_drag_resize: bool,
    /// Cancelling an edit also dismisses the device context menu.
    pub close_device_menu: bool,
}

impl ViewState {
    pub fn update(
        &mut self,
        msg: ViewMsg,
        timeline: &ArrangementTimeline,
        ctx: ViewCtx,
    ) -> ViewAction {
        let mut action = ViewAction::default();
        match msg {
            ViewMsg::SwitchWorkspace(ws) => {
                self.workspace = ws;
            }
            ViewMsg::SwitchDetailTab(tab) => {
                self.detail_panel_tab = tab;
            }
            ViewMsg::ZoomIn => {
                self.zoom_level = (self.zoom_level * 1.25).min(16.0);
            }
            ViewMsg::ZoomOut => {
                self.zoom_level = (self.zoom_level / 1.25).max(0.01);
            }
            ViewMsg::SetZoom(level) => {
                self.zoom_level = level.clamp(0.01, 16.0);
            }
            ViewMsg::ZoomToFit => {
                let content_beats = ctx.total_beats;
                if content_beats > 0.0 {
                    // Conservative estimate of canvas width (window minus track headers)
                    let canvas_width = 1400.0_f32;
                    let target_ppb = canvas_width / content_beats as f32;
                    self.zoom_level = (target_ppb / 20.0).clamp(0.01, 16.0);
                    self.scroll_offset_beats = 0.0;
                }
            }
            ViewMsg::ScrollArrangement(delta) => {
                let total = ctx.total_beats;
                self.scroll_offset_beats = (self.scroll_offset_beats + delta).clamp(0.0, total);
            }
            ViewMsg::SetSnapGrid(grid) => {
                self.snap_grid = grid;
                self.adaptive_grid = false;
                self.adaptive_grid_bias = 0;
            }
            ViewMsg::NarrowGrid => {
                if self.adaptive_grid {
                    self.adaptive_grid_bias = (self.adaptive_grid_bias + 1).min(6);
                } else {
                    self.snap_grid = self.snap_grid.narrower();
                }
            }
            ViewMsg::WidenGrid => {
                if self.adaptive_grid {
                    self.adaptive_grid_bias = (self.adaptive_grid_bias - 1).max(-6);
                } else {
                    self.snap_grid = self.snap_grid.wider();
                }
            }
            ViewMsg::ToggleTripletGrid => {
                self.snap_grid = self.snap_grid.toggle_triplet();
            }
            ViewMsg::ToggleSnapToGrid => {
                self.snap_enabled = !self.snap_enabled;
            }
            ViewMsg::ToggleAdaptiveGrid => {
                self.adaptive_grid = !self.adaptive_grid;
            }
            ViewMsg::CursorMoved(x, y) => {
                self.cursor_x = x;
                self.cursor_y = y;
            }
            ViewMsg::WindowResized(w, h) => {
                self.window_width = w;
                self.window_height = h;
            }
            ViewMsg::MouseReleased => {
                action.end_drag_resize = true;
            }
            ViewMsg::ShowContextMenu { x, y, target } => {
                // For ArrangementEmpty from mouse_area (no cursor coords),
                // use the globally tracked cursor position instead.
                let (menu_x, menu_y) = if matches!(target, ContextMenuTarget::ArrangementEmpty) {
                    (self.cursor_x, self.cursor_y)
                } else {
                    (x, y)
                };
                // Also select the clip if targeting one; the router
                // applies this to the arrangement slice.
                if let ContextMenuTarget::Clip {
                    track_id,
                    clip_id,
                    is_note_clip,
                } = &target
                {
                    action.select_clip = Some((*track_id, *clip_id, *is_note_clip));
                }
                self.context_menu = Some(crate::state::ContextMenu {
                    x: menu_x,
                    y: menu_y,
                    target,
                });
            }
            ViewMsg::DismissContextMenu => {
                self.context_menu = None;
            }
            ViewMsg::ToggleEditMenu => {
                self.edit_menu_open = !self.edit_menu_open;
            }
            ViewMsg::DismissEditMenu => {
                self.edit_menu_open = false;
            }
            ViewMsg::StartEditingTrackName { track_id, name } => {
                self.edit_name_text = name;
                self.editing_track_name = Some(track_id);
                self.editing_clip_name = None;
            }
            ViewMsg::StartEditingClipName(track_id, clip_id) => {
                self.context_menu = None;
                let name = timeline.get(track_id).and_then(|t| {
                    t.clips
                        .iter()
                        .find(|c| c.id == clip_id)
                        .map(|c| c.name.clone())
                        .or_else(|| {
                            t.note_clips
                                .iter()
                                .find(|c| c.id == clip_id)
                                .map(|c| c.name.clone())
                        })
                });
                if let Some(name) = name {
                    self.edit_name_text = name;
                    self.editing_clip_name = Some((track_id, clip_id));
                    self.editing_track_name = None;
                }
            }
            ViewMsg::EditNameText(t) => {
                self.edit_name_text = t;
            }
            ViewMsg::FinishEditing => {
                let new_name = self.edit_name_text.clone();
                if let Some(track_id) = self.editing_track_name.take() {
                    if !new_name.is_empty() {
                        action.rename = Some(RenameRequest::Track(track_id, new_name));
                        return action;
                    }
                }
                if let Some((track_id, clip_id)) = self.editing_clip_name.take() {
                    if !new_name.is_empty() {
                        action.rename = Some(RenameRequest::Clip(track_id, clip_id, new_name));
                        return action;
                    }
                }
            }
            ViewMsg::CancelEditing => {
                self.editing_track_name = None;
                self.editing_clip_name = None;
                self.edit_name_text.clear();
                action.close_device_menu = true;
            }
        }
        action
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zoom_clamps_both_directions() {
        let mut v = ViewState::default();
        v.update(
            ViewMsg::SetZoom(99.0),
            &ArrangementTimeline::default(),
            ViewCtx::default(),
        );
        assert_eq!(v.zoom_level, 16.0);
        v.update(
            ViewMsg::SetZoom(0.0),
            &ArrangementTimeline::default(),
            ViewCtx::default(),
        );
        assert_eq!(v.zoom_level, 0.01);
    }

    #[test]
    fn scroll_clamps_to_content() {
        let mut v = ViewState::default();
        let ctx = ViewCtx { total_beats: 32.0 };
        v.update(
            ViewMsg::ScrollArrangement(100.0),
            &ArrangementTimeline::default(),
            ctx,
        );
        assert_eq!(v.scroll_offset_beats, 32.0);
        v.update(
            ViewMsg::ScrollArrangement(-100.0),
            &ArrangementTimeline::default(),
            ctx,
        );
        assert_eq!(v.scroll_offset_beats, 0.0);
    }

    #[test]
    fn context_menu_on_clip_requests_selection() {
        let mut v = ViewState::default();
        let tid = TrackId::new();
        let cid = ClipId::new();
        let action = v.update(
            ViewMsg::ShowContextMenu {
                x: 10.0,
                y: 20.0,
                target: ContextMenuTarget::Clip {
                    track_id: tid,
                    clip_id: cid,
                    is_note_clip: false,
                },
            },
            &ArrangementTimeline::default(),
            ViewCtx::default(),
        );
        assert!(v.context_menu.is_some());
        assert_eq!(action.select_clip, Some((tid, cid, false)));
    }

    #[test]
    fn finish_editing_track_name_emits_rename() {
        let mut v = ViewState::default();
        let tid = TrackId::new();
        let timeline = ArrangementTimeline::default();
        v.update(
            ViewMsg::StartEditingTrackName {
                track_id: tid,
                name: "Old".to_string(),
            },
            &timeline,
            ViewCtx::default(),
        );
        assert_eq!(v.edit_name_text, "Old");
        v.update(
            ViewMsg::EditNameText("New".to_string()),
            &timeline,
            ViewCtx::default(),
        );
        let action = v.update(ViewMsg::FinishEditing, &timeline, ViewCtx::default());
        assert_eq!(
            action.rename,
            Some(RenameRequest::Track(tid, "New".to_string()))
        );
        assert_eq!(v.editing_track_name, None);
    }

    #[test]
    fn starts_editing_a_channel_name_without_a_regular_track_lookup() {
        let mut v = ViewState::default();
        let bus_id = TrackId::new();

        v.update(
            ViewMsg::StartEditingTrackName {
                track_id: bus_id,
                name: "A Return".to_string(),
            },
            &ArrangementTimeline::default(),
            ViewCtx::default(),
        );

        assert_eq!(v.editing_track_name, Some(bus_id));
        assert_eq!(v.edit_name_text, "A Return");
    }

    #[test]
    fn grid_commands_update_the_shared_editor_grid() {
        let mut v = ViewState::default();
        assert_eq!(v.snap_grid, SnapGrid::EIGHTH);
        assert!(v.snap_enabled);
        assert!(!v.adaptive_grid);

        v.update(
            ViewMsg::NarrowGrid,
            &ArrangementTimeline::default(),
            ViewCtx::default(),
        );
        assert_eq!(v.snap_grid, SnapGrid::SIXTEENTH);
        v.update(
            ViewMsg::WidenGrid,
            &ArrangementTimeline::default(),
            ViewCtx::default(),
        );
        assert_eq!(v.snap_grid, SnapGrid::EIGHTH);
        v.update(
            ViewMsg::ToggleTripletGrid,
            &ArrangementTimeline::default(),
            ViewCtx::default(),
        );
        assert_eq!(v.snap_grid, SnapGrid::EIGHTH.triplet());
        v.update(
            ViewMsg::ToggleSnapToGrid,
            &ArrangementTimeline::default(),
            ViewCtx::default(),
        );
        assert!(!v.snap_enabled);
        v.update(
            ViewMsg::ToggleAdaptiveGrid,
            &ArrangementTimeline::default(),
            ViewCtx::default(),
        );
        assert!(v.adaptive_grid);
        assert_eq!(
            v.grid_config().effective_grid(20.0),
            SnapGrid::QUARTER.triplet()
        );
        v.update(
            ViewMsg::NarrowGrid,
            &ArrangementTimeline::default(),
            ViewCtx::default(),
        );
        assert_eq!(
            v.grid_config().effective_grid(20.0),
            SnapGrid::EIGHTH.triplet()
        );
        v.update(
            ViewMsg::WidenGrid,
            &ArrangementTimeline::default(),
            ViewCtx::default(),
        );
        assert_eq!(
            v.grid_config().effective_grid(20.0),
            SnapGrid::QUARTER.triplet()
        );
    }

    #[test]
    fn cancel_editing_clears_all_edit_state() {
        let mut v = ViewState {
            editing_track_name: Some(TrackId::new()),
            edit_name_text: "x".to_string(),
            ..ViewState::default()
        };
        let action = v.update(
            ViewMsg::CancelEditing,
            &ArrangementTimeline::default(),
            ViewCtx::default(),
        );
        assert_eq!(v.editing_track_name, None);
        assert!(v.edit_name_text.is_empty());
        assert!(action.close_device_menu);
    }
}
