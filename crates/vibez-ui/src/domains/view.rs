//! View domain: how the project is being looked at. Workspace and
//! detail-panel tabs, timeline zoom/scroll, snap grid, the
//! right-click context menu, cursor/window tracking, and inline
//! renames. Nothing in here is part of the project itself.

use vibez_core::id::{ClipId, TrackId};

use crate::state::{
    ContextMenuTarget, DetailPanelTab, SnapGrid, TimelineEditorState, ViewState, Workspace,
};
use crate::timeline_geometry::{TimelineGeometry, BASE_PIXELS_PER_BEAT};

/// Messages the view domain handles.
#[derive(Debug, Clone)]
pub enum ViewMsg {
    SwitchWorkspace(Workspace),
    SwitchDetailTab(DetailPanelTab),
    ZoomIn,
    ZoomOut,
    SetZoom(f32),
    ZoomAround {
        factor: f32,
        anchor_x: f32,
    },
    ZoomToFit,
    ScrollArrangement(f64),
    SetSnapGrid(SnapGrid),
    NarrowGrid,
    WidenGrid,
    ToggleTripletGrid,
    ToggleSnapToGrid,
    ToggleAdaptiveGrid,
    BeginDetailPanelResize,
    ResizeDetailPanel(f32),
    EndDetailPanelResize,
    BeginPerformSurfaceResize,
    ResizePerformSurface(f32),
    EndPerformSurfaceResize,
    CursorMoved(f32, f32),
    WindowResized(f32, f32),
    MouseReleased,
    ShowContextMenu {
        x: f32,
        y: f32,
        target: ContextMenuTarget,
    },
    ToggleEditMenu,
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
    /// A global view preference changed and should be persisted.
    pub persist_settings: bool,
}

impl ViewState {
    pub fn update(
        &mut self,
        msg: ViewMsg,
        timeline: &TimelineEditorState,
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
                self.zoom_around(1.25, self.window_width / 2.0, ctx.total_beats);
            }
            ViewMsg::ZoomOut => {
                self.zoom_around(1.0 / 1.25, self.window_width / 2.0, ctx.total_beats);
            }
            ViewMsg::SetZoom(level) => {
                let factor = level.clamp(0.01, 16.0) / self.zoom_level;
                self.zoom_around(factor, self.window_width / 2.0, ctx.total_beats);
            }
            ViewMsg::ZoomAround { factor, anchor_x } => {
                self.zoom_around(factor, anchor_x, ctx.total_beats);
            }
            ViewMsg::ZoomToFit => {
                let content_beats = ctx.total_beats;
                if content_beats > 0.0 {
                    let canvas_width = self.window_width.max(1.0);
                    let target_ppb = TimelineGeometry::fitted(content_beats, canvas_width, 0.0)
                        .pixels_per_beat();
                    self.zoom_level = (target_ppb / BASE_PIXELS_PER_BEAT).clamp(0.01, 16.0);
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
            ViewMsg::BeginDetailPanelResize => {
                self.detail_panel_resize_active = true;
            }
            ViewMsg::ResizeDetailPanel(height) => {
                if self.detail_panel_resize_active {
                    self.detail_panel_height = height;
                }
            }
            ViewMsg::EndDetailPanelResize => {
                if self.detail_panel_resize_active {
                    self.detail_panel_resize_active = false;
                    action.persist_settings = true;
                }
            }
            ViewMsg::BeginPerformSurfaceResize => {
                self.perform_surface_resize_active = true;
            }
            ViewMsg::ResizePerformSurface(width) => {
                if self.perform_surface_resize_active {
                    self.perform_surface_width = width;
                }
            }
            ViewMsg::EndPerformSurfaceResize => {
                if self.perform_surface_resize_active {
                    self.perform_surface_resize_active = false;
                    action.persist_settings = true;
                }
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
                self.context_menu = Some(crate::state::ContextMenu { x, y, target });
            }
            ViewMsg::ToggleEditMenu => {
                self.edit_menu_open = !self.edit_menu_open;
            }
            ViewMsg::StartEditingTrackName { track_id, name } => {
                self.edit_name_text = name;
                self.editing_track_name = Some(track_id);
                self.editing_clip_name = None;
            }
            ViewMsg::StartEditingClipName(track_id, clip_id) => {
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

    fn zoom_around(&mut self, factor: f32, anchor_x: f32, total_beats: f64) {
        if !factor.is_finite() || factor <= 0.0 {
            return;
        }
        let old_geometry = TimelineGeometry::from_zoom(self.zoom_level, 0.0);
        let anchor_x = anchor_x.max(0.0);
        let anchor_beat = self.scroll_offset_beats + old_geometry.beats_for_width(anchor_x);
        let next_zoom = (self.zoom_level * factor).clamp(0.01, 16.0);
        let next_geometry = TimelineGeometry::from_zoom(next_zoom, 0.0);
        self.zoom_level = next_zoom;
        self.scroll_offset_beats = (anchor_beat - next_geometry.beats_for_width(anchor_x))
            .clamp(0.0, total_beats.max(0.0));
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
            &TimelineEditorState::default(),
            ViewCtx::default(),
        );
        assert_eq!(v.zoom_level, 16.0);
        v.update(
            ViewMsg::SetZoom(0.0),
            &TimelineEditorState::default(),
            ViewCtx::default(),
        );
        assert_eq!(v.zoom_level, 0.01);
    }

    #[test]
    fn zoom_in_keeps_the_viewport_centre_on_the_same_beat() {
        let mut view = ViewState {
            zoom_level: 1.0,
            scroll_offset_beats: 100.0,
            window_width: 800.0,
            ..ViewState::default()
        };
        let centre_x = view.window_width / 2.0;
        let before = view.scroll_offset_beats
            + centre_x as f64
                / TimelineGeometry::from_zoom(view.zoom_level, 0.0).pixels_per_beat() as f64;

        view.update(
            ViewMsg::ZoomIn,
            &TimelineEditorState::default(),
            ViewCtx {
                total_beats: 1_000.0,
            },
        );

        let after = view.scroll_offset_beats
            + centre_x as f64
                / TimelineGeometry::from_zoom(view.zoom_level, 0.0).pixels_per_beat() as f64;
        assert!((after - before).abs() < 1.0e-9);
    }

    #[test]
    fn all_workspaces_are_reachable_without_resetting_view_state() {
        let mut view = ViewState {
            zoom_level: 2.5,
            scroll_offset_beats: 12.0,
            ..ViewState::default()
        };
        let timeline = TimelineEditorState::default();

        for workspace in [Workspace::Perform, Workspace::Mix, Workspace::Arrange] {
            view.update(
                ViewMsg::SwitchWorkspace(workspace),
                &timeline,
                ViewCtx::default(),
            );
            assert_eq!(view.workspace, workspace);
            assert_eq!(view.zoom_level, 2.5);
            assert_eq!(view.scroll_offset_beats, 12.0);
        }
    }

    #[test]
    fn scroll_clamps_to_content() {
        let mut v = ViewState::default();
        let ctx = ViewCtx { total_beats: 32.0 };
        v.update(
            ViewMsg::ScrollArrangement(100.0),
            &TimelineEditorState::default(),
            ctx,
        );
        assert_eq!(v.scroll_offset_beats, 32.0);
        v.update(
            ViewMsg::ScrollArrangement(-100.0),
            &TimelineEditorState::default(),
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
            &TimelineEditorState::default(),
            ViewCtx::default(),
        );
        assert!(v.context_menu.is_some());
        assert_eq!(action.select_clip, Some((tid, cid, false)));
    }

    #[test]
    fn arrangement_empty_context_menu_uses_the_click_position() {
        let mut view = ViewState {
            cursor_x: 900.0,
            cursor_y: 700.0,
            ..ViewState::default()
        };
        view.update(
            ViewMsg::ShowContextMenu {
                x: 240.0,
                y: 180.0,
                target: ContextMenuTarget::ArrangementEmpty,
            },
            &TimelineEditorState::default(),
            ViewCtx::default(),
        );

        let menu = view.context_menu.expect("context menu");
        assert_eq!((menu.x, menu.y), (240.0, 180.0));
    }

    #[test]
    fn passive_view_updates_do_not_dismiss_an_arrange_context_menu() {
        let mut view = ViewState::default();
        let timeline = TimelineEditorState::default();
        view.update(
            ViewMsg::ShowContextMenu {
                x: 240.0,
                y: 180.0,
                target: ContextMenuTarget::ArrangementEmpty,
            },
            &timeline,
            ViewCtx::default(),
        );

        for message in [
            ViewMsg::CursorMoved(10.0, 20.0),
            ViewMsg::WindowResized(1400.0, 900.0),
            ViewMsg::MouseReleased,
        ] {
            view.update(message, &timeline, ViewCtx::default());
            assert!(view.context_menu.is_some());
        }
    }

    #[test]
    fn finish_editing_track_name_emits_rename() {
        let mut v = ViewState::default();
        let tid = TrackId::new();
        let timeline = TimelineEditorState::default();
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
            &TimelineEditorState::default(),
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
            &TimelineEditorState::default(),
            ViewCtx::default(),
        );
        assert_eq!(v.snap_grid, SnapGrid::SIXTEENTH);
        v.update(
            ViewMsg::WidenGrid,
            &TimelineEditorState::default(),
            ViewCtx::default(),
        );
        assert_eq!(v.snap_grid, SnapGrid::EIGHTH);
        v.update(
            ViewMsg::ToggleTripletGrid,
            &TimelineEditorState::default(),
            ViewCtx::default(),
        );
        assert_eq!(v.snap_grid, SnapGrid::EIGHTH.triplet());
        v.update(
            ViewMsg::ToggleSnapToGrid,
            &TimelineEditorState::default(),
            ViewCtx::default(),
        );
        assert!(!v.snap_enabled);
        v.update(
            ViewMsg::ToggleAdaptiveGrid,
            &TimelineEditorState::default(),
            ViewCtx::default(),
        );
        assert!(v.adaptive_grid);
        assert_eq!(
            v.grid_config().effective_grid(20.0),
            SnapGrid::QUARTER.triplet()
        );
        v.update(
            ViewMsg::NarrowGrid,
            &TimelineEditorState::default(),
            ViewCtx::default(),
        );
        assert_eq!(
            v.grid_config().effective_grid(20.0),
            SnapGrid::EIGHTH.triplet()
        );
        v.update(
            ViewMsg::WidenGrid,
            &TimelineEditorState::default(),
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
            &TimelineEditorState::default(),
            ViewCtx::default(),
        );
        assert_eq!(v.editing_track_name, None);
        assert!(v.edit_name_text.is_empty());
        assert!(action.close_device_menu);
    }

    #[test]
    fn perform_surface_resize_commits_one_global_view_preference() {
        let mut view = ViewState::default();
        let timeline = TimelineEditorState::default();

        view.update(
            ViewMsg::ResizePerformSurface(720.0),
            &timeline,
            ViewCtx::default(),
        );
        assert_eq!(
            view.perform_surface_width,
            crate::state::PERFORM_SURFACE_DEFAULT_WIDTH
        );

        view.update(
            ViewMsg::BeginPerformSurfaceResize,
            &timeline,
            ViewCtx::default(),
        );
        view.update(
            ViewMsg::ResizePerformSurface(720.0),
            &timeline,
            ViewCtx::default(),
        );
        let action = view.update(
            ViewMsg::EndPerformSurfaceResize,
            &timeline,
            ViewCtx::default(),
        );

        assert_eq!(view.perform_surface_width, 720.0);
        assert!(!view.perform_surface_resize_active);
        assert!(action.persist_settings);
    }

    #[test]
    fn detail_panel_resize_commits_one_global_view_preference() {
        let mut view = ViewState::default();
        let timeline = TimelineEditorState::default();

        view.update(
            ViewMsg::ResizeDetailPanel(420.0),
            &timeline,
            ViewCtx::default(),
        );
        assert_eq!(
            view.detail_panel_height,
            crate::state::DETAIL_PANEL_DEFAULT_HEIGHT
        );

        view.update(
            ViewMsg::BeginDetailPanelResize,
            &timeline,
            ViewCtx::default(),
        );
        view.update(
            ViewMsg::ResizeDetailPanel(420.0),
            &timeline,
            ViewCtx::default(),
        );
        let action = view.update(ViewMsg::EndDetailPanelResize, &timeline, ViewCtx::default());

        assert_eq!(view.detail_panel_height, 420.0);
        assert!(!view.detail_panel_resize_active);
        assert!(action.persist_settings);
    }
}
