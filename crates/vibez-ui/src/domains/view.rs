//! View domain: how the project is being looked at. Workspace and
//! detail-panel tabs, timeline zoom/scroll, snap grid, the
//! right-click context menu, cursor/window tracking, and inline
//! renames. Nothing in here is part of the project itself.

use vibez_core::id::{ClipId, TrackId};

use crate::state::{
    ContextMenuTarget, DetailPanelTab, SnapGrid, TimelineEditorState, ViewState, Workspace,
};
use crate::timeline_geometry::TimelineViewport;

/// Messages the view domain handles.
#[derive(Debug, Clone)]
pub enum ViewMsg {
    SwitchWorkspace(Workspace),
    /// Shows this bank of buses on the mixer's send rows.
    SetMixerSendBank(usize),
    SwitchDetailTab(DetailPanelTab),
    ZoomIn,
    ZoomOut,
    SetZoom(f32),
    ZoomAround {
        factor: f32,
        anchor_x: f32,
    },
    ZoomToFit,
    /// Frame the time selection, or the selected clips, in the lanes
    /// (Ableton's Z).
    ZoomToSelection,
    /// Return to the view from before the last zoom to selection
    /// (Ableton's X).
    ZoomBack,
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
    TrackHeaderHovered {
        track_id: TrackId,
        hovered: bool,
    },
}

impl ViewMsg {
    /// Zooms or scrolls `viewport`, a timeline `viewport_width` wide over
    /// `total_beats`, the one way Arrange and the Section editor navigate.
    /// False, leaving it alone, for any other message.
    pub fn navigate(
        &self,
        viewport: &mut TimelineViewport,
        total_beats: f64,
        viewport_width: f32,
    ) -> bool {
        let centre = viewport_width / 2.0;
        match *self {
            ViewMsg::ZoomIn => viewport.zoom_around(
                TimelineViewport::ZOOM_STEP,
                centre,
                total_beats,
                viewport_width,
            ),
            ViewMsg::ZoomOut => viewport.zoom_around(
                1.0 / TimelineViewport::ZOOM_STEP,
                centre,
                total_beats,
                viewport_width,
            ),
            ViewMsg::SetZoom(level) => {
                let factor = TimelineViewport::clamp_zoom(level) / viewport.zoom_level;
                viewport.zoom_around(factor, centre, total_beats, viewport_width);
            }
            ViewMsg::ZoomAround { factor, anchor_x } => {
                viewport.zoom_around(factor, anchor_x, total_beats, viewport_width);
            }
            ViewMsg::ScrollArrangement(delta) => {
                viewport.scroll_by(delta, total_beats, viewport_width);
            }
            _ => return false,
        }
        true
    }
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
    /// Padded arrangement length in beats (scroll clamp).
    pub total_beats: f64,
    /// Where the song's last clip ends, in beats (zoom-to-fit).
    pub song_beats: f64,
    /// Width of the Arrange clip lanes, which fit and zoom to selection
    /// frame into.
    pub lane_width: f32,
    /// Converts audio clip positions for zoom to selection.
    pub samples_per_beat: f64,
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
            ViewMsg::SetMixerSendBank(bank) => self.mixer_send_bank = bank,
            ViewMsg::ZoomIn
            | ViewMsg::ZoomOut
            | ViewMsg::SetZoom(_)
            | ViewMsg::ZoomAround { .. }
            | ViewMsg::ScrollArrangement(_) => {
                msg.navigate(&mut self.viewport, ctx.total_beats, ctx.lane_width);
            }
            ViewMsg::ZoomToFit => {
                let song_end = if ctx.song_beats > 0.0 {
                    ctx.song_beats
                } else {
                    ctx.total_beats
                };
                if song_end > 0.0 {
                    self.viewport.frame(0.0, song_end, ctx.lane_width);
                }
            }
            ViewMsg::ZoomToSelection => {
                if let Some((start, end)) = timeline.selection_span_beats(ctx.samples_per_beat) {
                    self.zoom_before_selection = Some(self.viewport);
                    self.viewport.frame(start, end, ctx.lane_width);
                }
            }
            ViewMsg::ZoomBack => {
                if let Some(previous) = self.zoom_before_selection.take() {
                    self.viewport = previous;
                }
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
            ViewMsg::TrackHeaderHovered { track_id, hovered } => {
                if hovered {
                    self.hovered_track_header = Some(track_id);
                } else if self.hovered_track_header == Some(track_id) {
                    self.hovered_track_header = None;
                }
            }
        }
        action
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timeline_geometry::TimelineGeometry;

    #[test]
    fn zoom_clamps_both_directions() {
        let mut v = ViewState::default();
        v.update(
            ViewMsg::SetZoom(99.0),
            &TimelineEditorState::default(),
            ViewCtx::default(),
        );
        assert_eq!(v.viewport.zoom_level, 16.0);
        v.update(
            ViewMsg::SetZoom(0.0),
            &TimelineEditorState::default(),
            ViewCtx::default(),
        );
        assert_eq!(v.viewport.zoom_level, 0.01);
    }

    #[test]
    fn zoom_in_keeps_the_lane_centre_on_the_same_beat() {
        let mut view = ViewState {
            viewport: TimelineViewport::new(1.0, 100.0),
            window_width: 1_000.0,
            ..ViewState::default()
        };
        let centre_x: f32 = 400.0;
        let before = view.viewport.scroll_offset_beats
            + centre_x as f64
                / TimelineGeometry::from_zoom(view.viewport.zoom_level, 0.0).pixels_per_beat()
                    as f64;

        view.update(
            ViewMsg::ZoomIn,
            &TimelineEditorState::default(),
            ViewCtx {
                total_beats: 1_000.0,
                lane_width: 800.0,
                ..ViewCtx::default()
            },
        );

        let after = view.viewport.scroll_offset_beats
            + centre_x as f64
                / TimelineGeometry::from_zoom(view.viewport.zoom_level, 0.0).pixels_per_beat()
                    as f64;
        assert!((after - before).abs() < 1.0e-9);
    }

    #[test]
    fn fit_shows_the_whole_song_inside_the_lane_area_not_the_window() {
        let mut view = ViewState {
            viewport: TimelineViewport::new(3.0, 40.0),
            window_width: 1920.0,
            ..ViewState::default()
        };
        let lane_width = 1697.0;
        let song_beats = 1120.0;

        view.update(
            ViewMsg::ZoomToFit,
            &TimelineEditorState::default(),
            ViewCtx {
                total_beats: 1400.0,
                song_beats,
                lane_width,
                ..ViewCtx::default()
            },
        );

        let song_end_x = view.viewport.geometry().beat_to_x(song_beats);
        assert_eq!(view.viewport.scroll_offset_beats, 0.0);
        assert!(
            song_end_x <= lane_width - 2.0 * crate::timeline_geometry::FRAME_MARGIN_PIXELS + 0.01,
            "the last bar ends inside the lanes, at {song_end_x}"
        );
        assert!(
            song_end_x > lane_width - 4.0 * crate::timeline_geometry::FRAME_MARGIN_PIXELS,
            "the song fills the lanes rather than the padded timeline, ends at {song_end_x}"
        );
    }

    const LANE_WIDTH: f32 = 1600.0;
    const SAMPLES_PER_BEAT: f64 = 100.0;

    fn selection_ctx() -> ViewCtx {
        ViewCtx {
            total_beats: 400.0,
            song_beats: 320.0,
            lane_width: LANE_WIDTH,
            samples_per_beat: SAMPLES_PER_BEAT,
        }
    }

    fn zoomed_out_view() -> ViewState {
        ViewState {
            viewport: TimelineViewport::new(0.25, 8.0),
            ..ViewState::default()
        }
    }

    fn assert_framed(view: &ViewState, start: f64, end: f64) {
        let margin = crate::timeline_geometry::FRAME_MARGIN_PIXELS;
        let geometry = view.viewport.geometry();
        let (start_x, end_x) = (geometry.beat_to_x(start), geometry.beat_to_x(end));
        assert!(
            (start_x - margin).abs() < 0.01,
            "selection starts at {start_x}"
        );
        assert!(
            (end_x - (LANE_WIDTH - margin)).abs() < 0.01,
            "selection ends at {end_x}"
        );
    }

    fn audio_clip(position: u64, duration: u64) -> crate::state::UiClip {
        crate::state::UiClip {
            id: ClipId::new(),
            name: "Audio".into(),
            audio: std::sync::Arc::new(vibez_core::audio_buffer::DecodedAudio {
                channels: vec![vec![0.0; 8]],
                sample_rate: 48_000,
            }),
            source: None,
            position,
            source_offset: 0,
            start_marker: 0,
            duration,
            loop_enabled: false,
            loop_start: 0,
            loop_end: 0,
            gain_db: Default::default(),
            fades: Default::default(),
            playback_direction: Default::default(),
            transient_markers: Default::default(),
            warp_markers: Default::default(),
            transpose: Default::default(),
            original_bpm: None,
            warped: false,
            warped_to_bpm: None,
            original_audio: None,
        }
    }

    fn note_clip(position_beats: f64, duration_beats: f64) -> crate::state::UiNoteClip {
        crate::state::UiNoteClip {
            id: ClipId::new(),
            name: "MIDI".into(),
            position_beats,
            duration_beats,
            notes: Vec::new(),
            selected_notes: Default::default(),
            start_marker_beats: 0.0,
            loop_enabled: false,
            loop_start_beats: 0.0,
            loop_end_beats: 0.0,
            groove_grid: Default::default(),
        }
    }

    #[test]
    fn zoom_to_selection_frames_the_time_selection_and_zoom_back_restores_the_view() {
        let mut view = zoomed_out_view();
        let timeline = TimelineEditorState {
            time_selection_active: true,
            selection_start_beats: 64.0,
            selection_end_beats: 80.0,
            ..TimelineEditorState::default()
        };

        view.update(ViewMsg::ZoomToSelection, &timeline, selection_ctx());
        assert_framed(&view, 64.0, 80.0);

        view.update(ViewMsg::ZoomBack, &timeline, selection_ctx());
        assert_eq!(view.viewport.zoom_level, 0.25);
        assert_eq!(view.viewport.scroll_offset_beats, 8.0);
    }

    #[test]
    fn zoom_to_selection_frames_every_selected_clip_without_a_time_range() {
        let mut view = zoomed_out_view();
        let (audio_track, midi_track) = (TrackId::new(), TrackId::new());
        let audio = audio_clip(3_200, 800);
        let notes = note_clip(44.0, 4.0);
        let unselected = note_clip(200.0, 4.0);
        let mut content = crate::state::TimelineContent::default();
        content.ensure(audio_track).clips.push(audio.clone());
        content
            .ensure(midi_track)
            .note_clips
            .extend([notes.clone(), unselected]);
        let timeline = TimelineEditorState {
            timeline: std::sync::Arc::new(content),
            selected_clips: [
                crate::state::ArrangementSelection::AudioClip {
                    track_id: audio_track,
                    clip_id: audio.id,
                },
                crate::state::ArrangementSelection::NoteClip {
                    track_id: midi_track,
                    clip_id: notes.id,
                },
            ]
            .into_iter()
            .collect(),
            ..TimelineEditorState::default()
        };

        view.update(ViewMsg::ZoomToSelection, &timeline, selection_ctx());

        assert_framed(&view, 32.0, 48.0);
    }

    #[test]
    fn zoom_to_selection_without_a_selection_leaves_the_view_alone() {
        let mut view = zoomed_out_view();
        let timeline = TimelineEditorState::default();

        view.update(ViewMsg::ZoomToSelection, &timeline, selection_ctx());
        view.update(ViewMsg::ZoomBack, &timeline, selection_ctx());

        assert_eq!(view.viewport.zoom_level, 0.25);
        assert_eq!(view.viewport.scroll_offset_beats, 8.0);
    }

    #[test]
    fn zoom_back_stays_inside_a_song_that_has_since_shrunk() {
        let mut view = ViewState {
            viewport: TimelineViewport::new(2.0, 300.0),
            ..ViewState::default()
        };
        let timeline = TimelineEditorState {
            time_selection_active: true,
            selection_start_beats: 64.0,
            selection_end_beats: 80.0,
            ..TimelineEditorState::default()
        };
        view.update(ViewMsg::ZoomToSelection, &timeline, selection_ctx());
        let shrunk = ViewCtx {
            total_beats: 100.0,
            ..selection_ctx()
        };

        view.update(ViewMsg::ZoomBack, &timeline, shrunk);

        assert_eq!(view.viewport.zoom_level, 2.0);
        assert_eq!(
            view.viewport.scroll_offset_beats,
            view.viewport.max_scroll(100.0, LANE_WIDTH)
        );
    }

    #[test]
    fn zoom_back_remembers_one_level_only() {
        let mut view = zoomed_out_view();
        let timeline = TimelineEditorState {
            time_selection_active: true,
            selection_start_beats: 64.0,
            selection_end_beats: 80.0,
            ..TimelineEditorState::default()
        };

        view.update(ViewMsg::ZoomToSelection, &timeline, selection_ctx());
        view.update(ViewMsg::ZoomBack, &timeline, selection_ctx());
        view.update(ViewMsg::ZoomIn, &timeline, selection_ctx());
        let zoomed_in = (view.viewport.zoom_level, view.viewport.scroll_offset_beats);
        view.update(ViewMsg::ZoomBack, &timeline, selection_ctx());

        assert_eq!(
            (view.viewport.zoom_level, view.viewport.scroll_offset_beats),
            zoomed_in
        );
    }

    #[test]
    fn all_workspaces_are_reachable_without_resetting_view_state() {
        let mut view = ViewState {
            viewport: TimelineViewport::new(2.5, 12.0),
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
            assert_eq!(view.viewport.zoom_level, 2.5);
            assert_eq!(view.viewport.scroll_offset_beats, 12.0);
        }
    }

    #[test]
    fn scroll_clamps_to_content() {
        let mut v = ViewState::default();
        let ctx = ViewCtx {
            total_beats: 32.0,
            ..ViewCtx::default()
        };
        v.update(
            ViewMsg::ScrollArrangement(100.0),
            &TimelineEditorState::default(),
            ctx,
        );
        assert_eq!(v.viewport.scroll_offset_beats, 32.0);
        v.update(
            ViewMsg::ScrollArrangement(-100.0),
            &TimelineEditorState::default(),
            ctx,
        );
        assert_eq!(v.viewport.scroll_offset_beats, 0.0);
    }

    #[test]
    fn scroll_stops_at_the_last_full_view_of_the_lanes() {
        let mut v = ViewState::default();
        let ctx = ViewCtx {
            total_beats: 320.0,
            lane_width: 800.0,
            ..ViewCtx::default()
        };
        let visible = v.viewport.geometry().visible_beats(800.0);

        v.update(
            ViewMsg::ScrollArrangement(1_000.0),
            &TimelineEditorState::default(),
            ctx,
        );

        assert_eq!(v.viewport.scroll_offset_beats, 320.0 - visible);
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
    fn a_track_header_stays_hovered_whichever_order_enter_and_leave_arrive() {
        let mut view = ViewState::default();
        let timeline = TimelineEditorState::default();
        let (upper, lower) = (TrackId::new(), TrackId::new());
        let hover = |view: &mut ViewState, track_id, hovered| {
            view.update(
                ViewMsg::TrackHeaderHovered { track_id, hovered },
                &timeline,
                ViewCtx::default(),
            );
        };

        hover(&mut view, lower, true);
        hover(&mut view, upper, true);
        hover(&mut view, lower, false);
        assert_eq!(view.hovered_track_header, Some(upper));

        hover(&mut view, upper, false);
        assert_eq!(view.hovered_track_header, None);
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
