//! Per-track clip lane canvas: clip drawing, drag/resize/split
//! interaction, sample drop targets.

use std::collections::HashSet;

use iced::mouse;
use iced::widget::canvas;
use iced::{Color, Rectangle, Renderer, Theme};

use crate::domains::arrangement::ArrangementMsg;
use crate::domains::browser::BrowserMsg;
use crate::domains::transport::TransportMsg;
use crate::domains::view::ViewMsg;
use crate::message::Message;
use crate::state::{
    ArrangementMarqueeRect, ArrangementSelection, ContextMenuTarget, GridConfig, ProjectTrack,
    TrackTimelineContent, UndoGestureId,
};
use crate::timeline_geometry::TimelineGeometry;
use crate::widgets::local_drag::LocalDrag;
use crate::widgets::timeline::marquee;
use vibez_core::id::{ClipId, TrackId};

use super::clip_drag::ClipDragAction;
use super::*;

/// Pointer travel before a press becomes a rubber-band rather than a seek.
const MARQUEE_MIN_PX: f32 = 4.0;

/// Vertical travel before a clip drag may change lane. Without it a
/// horizontal drag near a row boundary would flicker between tracks.
const CROSS_TRACK_MIN_PX: f32 = 20.0;

/// Where an unmodified vertical wheel gesture should be handled.
///
/// Arrange treats it as timeline panning. Section construction lives inside
/// a vertical track scroller, so its lanes must let the parent consume it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum VerticalWheelRouting {
    #[default]
    PanTimeline,
    BubbleToTrackScroller,
}

/// Interaction state for clip canvas.
#[derive(Debug, Default)]
pub struct ClipInteractionState {
    pub drag: Option<ClipDragAction>,
    pub shift_held: bool,
}

/// Canvas for ONE track's clip area (waveforms, borders, names, playhead overlay).
pub struct TrackClipCanvas {
    vertical_wheel_routing: VerticalWheelRouting,
    pub track_id: TrackId,
    pub track_index: usize,
    pub total_tracks: usize,
    pub track_ids: Vec<TrackId>,
    pub track_kinds: Vec<bool>, // is_instrument flags
    /// True vertical layout of every row in this column, so a rubber-band
    /// spans lanes correctly even with automation lanes expanded. Empty
    /// when the surface has not opted into box selection.
    pub row_spans: Vec<TrackRowSpan>,
    /// Live rubber-band, shared by every lane so each draws its own slice
    /// of one continuous box.
    pub marquee: Option<ArrangementMarqueeRect>,
    pub selected_clips: HashSet<ClipId>,
    pub clips: Vec<TimelineClip>,
    pub note_clips: Vec<TimelineNoteClip>,
    /// Transient Section Record visualization. It is deliberately excluded
    /// from hit testing and edit messages.
    pub recording_preview: Option<TimelineNoteClip>,
    /// Transient Audio Track Recording waveform. Like Section Record, it is
    /// display-only until Stop commits one canonical Clip.
    pub audio_recording_preview: Option<TimelineClip>,
    /// Primary edit cursor (or Arrange's shared transport/edit cursor).
    pub playhead_beats: f64,
    /// Section playback truth, rendered independently from the edit cursor.
    pub playback_playhead_beats: Option<f64>,
    pub zoom_level: f32,
    pub grid: GridConfig,
    pub scroll_offset_beats: f64,
    pub total_beats: f64,
    pub sample_rate: u32,
    pub bpm: f64,
    pub selected: bool,
    pub track_color: Color,
    pub is_instrument: bool,
    pub loop_enabled: bool,
    pub loop_start_beats: f64,
    pub loop_end_beats: f64,
    pub time_selection_active: bool,
    pub selection_start_beats: f64,
    pub selection_end_beats: f64,
    /// Track the selection originated on. `None` means arrangement-wide
    /// (selection was drawn on the ruler); `Some` means show it only on
    /// that lane.
    pub time_selection_track: Option<TrackId>,
    /// True while a sample is being drag-dropped from the browser.
    /// Controls whether mouse-up on this lane emits `DropSampleOnArrangement`.
    pub sample_drop_active: bool,
    /// Musical length shown by the placement preview. RAW derives this at the
    /// project tempo; WARP retains the source's confirmed musical length.
    pub sample_drop_duration_beats: Option<f64>,
    pub sample_drop_detail: Option<String>,
    /// The track name this canvas was constructed with. Drawn on the drop
    /// indicator so the user can verify which lane will receive the drop.
    pub track_name: String,
}

impl TrackClipCanvas {
    #[allow(clippy::too_many_arguments)]
    pub fn from_track(
        track: &ProjectTrack,
        content: &TrackTimelineContent,
        playhead_beats: f64,
        zoom_level: f32,
        grid: GridConfig,
        scroll_offset_beats: f64,
        viewport_width: f32,
        total_beats: f64,
        sample_rate: u32,
        selected: bool,
        track_color: Color,
        bpm: f64,
        track_id: TrackId,
        track_index: usize,
        total_tracks: usize,
        track_ids: Vec<TrackId>,
        track_kinds: Vec<bool>,
        selected_clips: HashSet<ClipId>,
        loop_enabled: bool,
        loop_start_beats: f64,
        loop_end_beats: f64,
        time_selection_active: bool,
        selection_start_beats: f64,
        selection_end_beats: f64,
        time_selection_track: Option<TrackId>,
        sample_drop_active: bool,
        sample_drop_duration_beats: Option<f64>,
        sample_drop_detail: Option<String>,
    ) -> Self {
        let geometry = TimelineGeometry::from_zoom(zoom_level, scroll_offset_beats);
        let visible_beats = geometry.visible_beats(viewport_width.max(1.0));
        let prefetch = visible_beats * 0.25;
        let visible_start = (scroll_offset_beats - prefetch).max(0.0);
        let visible_end = scroll_offset_beats + visible_beats + prefetch;
        let samples_per_beat = if bpm > 0.0 {
            sample_rate as f64 * 60.0 / bpm
        } else {
            1.0
        };
        let clips = content
            .clips
            .iter()
            .filter(|clip| {
                let start = clip.position as f64 / samples_per_beat;
                let end = start + clip.duration as f64 / samples_per_beat;
                start < visible_end && end > visible_start
            })
            .map(|c| TimelineClip {
                clip_id: c.id,
                position: c.position,
                duration: c.duration,
                name: c.name.clone(),
                peaks: compute_clip_peaks(c),
                peak_span_frames: None,
                loop_enabled: c.loop_enabled,
                loop_start: c.loop_start,
                loop_end: c.loop_end,
                fade_in_frames: c.fades.fade_in_frames(),
                fade_out_frames: c.fades.fade_out_frames(),
                fade_in_curve: c.fades.fade_in_curve(),
                fade_out_curve: c.fades.fade_out_curve(),
                crossfade_in: c.fades.crossfade_in_from().is_some(),
                crossfade_out: c.fades.crossfade_out_to().is_some(),
                warp_stale: c.warped
                    && c.warped_to_bpm
                        .map(|b| (b - bpm).abs() > 0.01)
                        .unwrap_or(false),
            })
            .collect();
        let note_clips = content
            .note_clips
            .iter()
            .filter(|clip| {
                clip.position_beats < visible_end
                    && clip.position_beats + clip.duration_beats > visible_start
            })
            .map(|c| TimelineNoteClip {
                clip_id: c.id,
                position_beats: c.position_beats,
                duration_beats: c.duration_beats,
                name: c.name.clone(),
                notes: if geometry.pixels_per_beat() >= 4.0 {
                    c.notes
                        .iter()
                        .flat_map(|note| {
                            let first_repeat = c.timeline().first_wrap().unwrap_or(f64::INFINITY);
                            c.note_occurrences(note.start_beat)
                                .into_iter()
                                .map(move |occurrence| {
                                    (
                                        note.pitch,
                                        occurrence,
                                        note.duration_beats,
                                        c.loop_enabled && occurrence + f64::EPSILON >= first_repeat,
                                    )
                                })
                        })
                        .collect()
                } else {
                    Vec::new()
                },
                start_marker_beats: c.start_marker_beats,
                loop_enabled: c.loop_enabled,
                loop_start_beats: c.loop_start_beats,
                loop_end_beats: c.loop_end_beats,
            })
            .collect();
        Self {
            vertical_wheel_routing: VerticalWheelRouting::default(),
            track_id,
            track_index,
            total_tracks,
            track_ids,
            track_kinds,
            row_spans: Vec::new(),
            marquee: None,
            selected_clips,
            clips,
            note_clips,
            recording_preview: None,
            audio_recording_preview: None,
            playhead_beats,
            playback_playhead_beats: None,
            zoom_level,
            grid,
            scroll_offset_beats,
            total_beats,
            sample_rate,
            bpm,
            selected,
            track_color,
            is_instrument: track.kind.is_midi(),
            loop_enabled,
            loop_start_beats,
            loop_end_beats,
            time_selection_active,
            selection_start_beats,
            selection_end_beats,
            time_selection_track,
            sample_drop_active,
            sample_drop_duration_beats,
            sample_drop_detail,
            track_name: track.name.clone(),
        }
    }

    /// Opt this lane into box selection by handing it the column layout
    /// and the live rubber-band. Surfaces that lay their lanes out in one
    /// vertical column (Arrange, Section construction) can both use it.
    pub fn with_marquee(
        mut self,
        row_spans: Vec<TrackRowSpan>,
        marquee: Option<ArrangementMarqueeRect>,
    ) -> Self {
        self.row_spans = row_spans;
        self.marquee = marquee;
        self
    }

    pub fn with_recording_preview(mut self, preview: TimelineNoteClip) -> Self {
        self.recording_preview = Some(preview);
        self
    }

    pub fn with_audio_recording_preview(mut self, preview: TimelineClip) -> Self {
        self.audio_recording_preview = Some(preview);
        self
    }

    pub fn with_playback_playhead(mut self, playback_playhead_beats: Option<f64>) -> Self {
        self.playback_playhead_beats = playback_playhead_beats;
        self
    }

    /// Let the surrounding track list handle ordinary vertical wheel input.
    /// Horizontal wheel input and Shift+wheel remain lane-local timeline
    /// navigation gestures.
    pub fn with_vertical_track_scrolling(mut self) -> Self {
        self.vertical_wheel_routing = VerticalWheelRouting::BubbleToTrackScroller;
        self
    }

    pub(super) fn geometry(&self) -> TimelineGeometry {
        TimelineGeometry::from_zoom(self.zoom_level, self.scroll_offset_beats)
    }

    pub(super) fn pixels_per_beat(&self) -> f32 {
        self.geometry().pixels_per_beat()
    }

    pub(super) fn beat_to_x(&self, beat: f64) -> f32 {
        self.geometry().beat_to_x(beat)
    }

    pub(super) fn x_to_beat(&self, x: f32) -> f64 {
        self.geometry().x_to_beat(x)
    }

    pub(super) fn snapped_beat(&self, beat: f64) -> f64 {
        self.grid.snap_beat(beat, self.pixels_per_beat())
    }

    /// Offset of this lane's row from the top of the column, converting
    /// between lane-local and column coordinates.
    pub(super) fn row_top(&self) -> f32 {
        self.row_spans
            .iter()
            .find(|row| row.track_id == self.track_id)
            .map(|row| row.top)
            .unwrap_or(0.0)
    }

    /// Index of the lane sitting at a column-space y.
    ///
    /// Falls back to uniform row stepping relative to this lane when the
    /// surface supplied no layout, preserving the old behaviour for
    /// canvases that have not opted into box selection.
    pub(super) fn resolve_lane_at(&self, column_y: f32) -> Option<usize> {
        if self.row_spans.is_empty() {
            let offset = ((column_y - self.row_top()) / TRACK_ROW_HEIGHT).floor() as i32;
            let last = self.total_tracks as i32 - 1;
            return (last >= 0).then(|| (self.track_index as i32 + offset).clamp(0, last) as usize);
        }
        self.row_spans
            .iter()
            .position(|row| column_y >= row.top && column_y < row.bottom())
    }

    /// Build the marquee message for a drag from `anchor` to the pointer,
    /// both already in column coordinates.
    fn marquee_message(
        &self,
        anchor_beat: f64,
        anchor_column_y: f32,
        current_x: f32,
        current_column_y: f32,
        additive: bool,
    ) -> Message {
        let current_beat = self.snapped_beat(self.geometry().x_to_beat(current_x));
        let span = marquee::resolve(
            anchor_beat,
            current_beat,
            anchor_column_y,
            current_column_y,
            &self.row_spans,
        );
        Message::Arrangement(ArrangementMsg::MarqueeSelect {
            anchor_track: self.track_id,
            start_beats: span.start_beats,
            end_beats: span.end_beats,
            top_y: span.top_y,
            bottom_y: span.bottom_y,
            track_ids: span.track_ids,
            additive,
        })
    }

    /// Samples per beat.
    pub(super) fn spb(&self) -> f64 {
        if self.bpm > 0.0 {
            self.sample_rate as f64 * 60.0 / self.bpm
        } else {
            1.0
        }
    }

    /// Hit test: find a clip at the given pixel x position.
    /// Returns (clip_id, is_note_clip, near_right_edge, position_beats, duration_beats).
    pub(super) fn hit_test(&self, pos_x: f32) -> Option<(ClipId, bool, bool, f64, f64)> {
        let geometry = self.geometry();
        let spb = self.spb();

        // Check audio clips
        for clip in &self.clips {
            let clip_start_beat = clip.position as f64 / spb;
            let clip_dur_beats = clip.duration as f64 / spb;
            let clip_x = self.beat_to_x(clip_start_beat);
            let clip_w = geometry.width_for_beats(clip_dur_beats);

            if pos_x >= clip_x && pos_x <= clip_x + clip_w {
                let near_right = pos_x > clip_x + clip_w - RESIZE_EDGE_PX;
                return Some((
                    clip.clip_id,
                    false,
                    near_right,
                    clip_start_beat,
                    clip_dur_beats,
                ));
            }
        }

        // Check note clips
        for note_clip in &self.note_clips {
            let clip_x = self.beat_to_x(note_clip.position_beats);
            let clip_w = geometry.width_for_beats(note_clip.duration_beats);

            if pos_x >= clip_x && pos_x <= clip_x + clip_w {
                let near_right = pos_x > clip_x + clip_w - RESIZE_EDGE_PX;
                return Some((
                    note_clip.clip_id,
                    true,
                    near_right,
                    note_clip.position_beats,
                    note_clip.duration_beats,
                ));
            }
        }

        None
    }
}

impl canvas::Program<Message> for TrackClipCanvas {
    type State = ClipInteractionState;

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        self.draw_impl(renderer, bounds, cursor)
    }
    fn mouse_interaction(
        &self,
        state: &Self::State,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if let Some(ref drag) = state.drag {
            return match drag {
                ClipDragAction::MoveClip { .. } => mouse::Interaction::Grabbing,
                ClipDragAction::ResizeClip { .. } => mouse::Interaction::ResizingHorizontally,
                ClipDragAction::FadeClip(_) => mouse::Interaction::ResizingHorizontally,
                ClipDragAction::FadeCurve(_) => mouse::Interaction::ResizingVertically,
                ClipDragAction::RegionSelect { .. } => mouse::Interaction::Crosshair,
                ClipDragAction::PendingSeek { .. } => mouse::Interaction::Pointer,
                ClipDragAction::PanViewport { .. } => mouse::Interaction::Grabbing,
            };
        }

        if let Some(pos) = cursor.position_in(bounds) {
            if self.fade_curve_hit(pos, bounds.height).is_some() {
                return mouse::Interaction::ResizingVertically;
            }
            if self.fade_handle_hit(pos).is_some() {
                return mouse::Interaction::ResizingHorizontally;
            }
            if let Some((_, _, near_right, _, _)) = self.hit_test(pos.x) {
                let in_title_bar = pos.y < CLIP_Y + CLIP_TITLE_HEIGHT;
                if near_right && in_title_bar {
                    return mouse::Interaction::ResizingHorizontally;
                }
                if in_title_bar {
                    return mouse::Interaction::Grab;
                }
                // Body zone — pointer (for seek / region select)
                return mouse::Interaction::Pointer;
            }
            return mouse::Interaction::Pointer;
        }

        mouse::Interaction::default()
    }

    fn update(
        &self,
        state: &mut Self::State,
        event: canvas::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> (canvas::event::Status, Option<Message>) {
        let track_id = self.track_id;

        match event {
            // -- Middle drag: pan the timeline without changing selection --
            canvas::Event::Mouse(iced::mouse::Event::ButtonPressed(
                iced::mouse::Button::Middle,
            )) => {
                if let Some(pos) = cursor.position_in(bounds) {
                    state.drag = Some(ClipDragAction::PanViewport {
                        start_local_x: pos.x,
                        start_scroll_beats: self.scroll_offset_beats,
                    });
                    return (canvas::event::Status::Captured, None);
                }
            }

            // -- Left click: select clip, start drag, or seek --
            // Clip zones (Ableton-style):
            //   Title bar (top ~18px): move / resize (right edge)
            //   Body (below title):    seek / region-select
            canvas::Event::Mouse(iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left)) => {
                if let Some(pos) = cursor.position_in(bounds) {
                    if let Some(drag) = self.fade_curve_hit(pos, bounds.height) {
                        state.drag = Some(ClipDragAction::FadeCurve(drag));
                        return (canvas::event::Status::Captured, None);
                    }
                    if let Some(drag) = self.fade_handle_hit(pos) {
                        state.drag = Some(ClipDragAction::FadeClip(drag));
                        return (canvas::event::Status::Captured, None);
                    }
                    if let Some((clip_id, is_note_clip, near_right, pos_beats, _dur_beats)) =
                        self.hit_test(pos.x)
                    {
                        let in_title_bar = pos.y < CLIP_Y + CLIP_TITLE_HEIGHT;

                        // Build selection message
                        let selection = if is_note_clip {
                            ArrangementSelection::NoteClip { track_id, clip_id }
                        } else {
                            ArrangementSelection::AudioClip { track_id, clip_id }
                        };

                        if near_right && in_title_bar {
                            // Right edge of title bar → resize
                            state.drag = Some(ClipDragAction::ResizeClip {
                                undo_gesture: UndoGestureId::new(),
                                clip_id,
                                is_note_clip,
                                clip_start_beat: pos_beats,
                            });
                        } else if in_title_bar {
                            // Title bar → move clip
                            state.drag = Some(ClipDragAction::MoveClip {
                                undo_gesture: UndoGestureId::new(),
                                clip_id,
                                is_note_clip,
                                start_local_x: pos.x,
                                start_scroll_beats: self.scroll_offset_beats,
                                original_position_beats: pos_beats,
                                start_y: pos.y,
                            });
                        } else {
                            // Body → seek / region-select (like empty space)
                            let beat = self.x_to_beat(pos.x);
                            state.drag = Some(ClipDragAction::PendingSeek {
                                beat,
                                start_x: pos.x,
                                anchor_column_y: self.row_top() + pos.y,
                            });
                        }

                        return (
                            canvas::event::Status::Captured,
                            if near_right
                                && in_title_bar
                                && self.selected_clips.contains(&clip_id)
                                && !state.shift_held
                            {
                                None
                            } else {
                                Some(Message::Arrangement(
                                    ArrangementMsg::SelectArrangementClip {
                                        selection,
                                        shift_held: state.shift_held,
                                    },
                                ))
                            },
                        );
                    }

                    // No clip hit. Start a PendingSeek (may become RegionSelect on drag).
                    // Also surface the track as the selection target so subsequent
                    // browser imports / dropdowns know which lane is "active".
                    if bounds.width > 0.0 {
                        let beat = self.geometry().x_to_beat(pos.x);
                        state.drag = Some(ClipDragAction::PendingSeek {
                            beat,
                            start_x: pos.x,
                            anchor_column_y: self.row_top() + pos.y,
                        });
                        return (
                            canvas::event::Status::Captured,
                            Some(Message::select_track(track_id)),
                        );
                    }
                }
            }

            // -- Right-click: context menu --
            canvas::Event::Mouse(iced::mouse::Event::ButtonPressed(iced::mouse::Button::Right)) => {
                if let Some(pos) = cursor.position_in(bounds) {
                    let screen_x = bounds.x + pos.x;
                    let screen_y = bounds.y + pos.y;

                    // Hit test for clip
                    if let Some((clip_id, is_note_clip, _, _, _)) = self.hit_test(pos.x) {
                        return (
                            canvas::event::Status::Captured,
                            Some(Message::View(ViewMsg::ShowContextMenu {
                                x: screen_x,
                                y: screen_y,
                                target: ContextMenuTarget::Clip {
                                    track_id,
                                    clip_id,
                                    is_note_clip,
                                },
                            })),
                        );
                    }

                    // No clip hit — check if within active time selection
                    if self.time_selection_active
                        && self.selection_end_beats > self.selection_start_beats
                    {
                        let beat = self.geometry().x_to_beat(pos.x);
                        if beat >= self.selection_start_beats && beat <= self.selection_end_beats {
                            return (
                                canvas::event::Status::Captured,
                                Some(Message::View(ViewMsg::ShowContextMenu {
                                    x: screen_x,
                                    y: screen_y,
                                    target: ContextMenuTarget::TimeSelection {
                                        start_beats: self.selection_start_beats,
                                        end_beats: self.selection_end_beats,
                                        track_id: Some(self.track_id),
                                    },
                                })),
                            );
                        }
                    }

                    // No clip, no time selection — show arrangement-empty context menu
                    return (
                        canvas::event::Status::Captured,
                        Some(Message::View(ViewMsg::ShowContextMenu {
                            x: screen_x,
                            y: screen_y,
                            target: ContextMenuTarget::ArrangementEmpty,
                        })),
                    );
                }
            }

            // -- Drag: move, resize, or region select --
            canvas::Event::Mouse(iced::mouse::Event::CursorMoved { .. }) => {
                if let Some(ref drag) = state.drag {
                    if let Some(local) = LocalDrag::unclamped().position(cursor, bounds) {
                        let local_x = local.x;
                        let geometry = self.geometry();

                        match drag {
                            ClipDragAction::PendingSeek {
                                beat: anchor,
                                start_x,
                                anchor_column_y,
                            } => {
                                let column_y = self.row_top() + local.y;
                                let dx = (local_x - start_x).abs();
                                let dy = (column_y - *anchor_column_y).abs();
                                // Vertical travel promotes too: a drag
                                // straight down the lanes is a valid box.
                                if dx > MARQUEE_MIN_PX || dy > MARQUEE_MIN_PX {
                                    let anchor_snapped = self.snapped_beat(*anchor);
                                    let anchor_y = *anchor_column_y;
                                    state.drag = Some(ClipDragAction::RegionSelect {
                                        anchor_beat: anchor_snapped,
                                        anchor_column_y: anchor_y,
                                    });
                                    return (
                                        canvas::event::Status::Captured,
                                        Some(self.marquee_message(
                                            anchor_snapped,
                                            anchor_y,
                                            local_x,
                                            column_y,
                                            state.shift_held,
                                        )),
                                    );
                                }
                                return (canvas::event::Status::Captured, None);
                            }
                            ClipDragAction::RegionSelect {
                                anchor_beat,
                                anchor_column_y,
                            } => {
                                return (
                                    canvas::event::Status::Captured,
                                    Some(self.marquee_message(
                                        *anchor_beat,
                                        *anchor_column_y,
                                        local_x,
                                        self.row_top() + local.y,
                                        state.shift_held,
                                    )),
                                );
                            }
                            ClipDragAction::MoveClip {
                                undo_gesture,
                                clip_id,
                                is_note_clip,
                                start_local_x,
                                start_scroll_beats,
                                original_position_beats,
                                start_y,
                            } => {
                                let delta_px = local_x - start_local_x;
                                let new_pos = crate::timeline_geometry::compensated_drag_beat(
                                    *original_position_beats,
                                    delta_px,
                                    geometry.pixels_per_beat(),
                                    *start_scroll_beats,
                                    self.scroll_offset_beats,
                                )
                                .max(0.0);

                                let snapped = self.snapped_beat(new_pos);

                                // Check for cross-track drag
                                let local_y = local.y;
                                let dy = local_y - start_y;

                                // Resolve against real row geometry when the
                                // surface supplied it; stepping by a fixed
                                // row height mis-targets as soon as a track
                                // above has its automation lanes expanded.
                                let target_idx = self
                                    .resolve_lane_at(self.row_top() + local_y)
                                    .filter(|_| dy.abs() > CROSS_TRACK_MIN_PX);

                                if let Some(target_idx) = target_idx {
                                    if target_idx != self.track_index
                                        && target_idx < self.track_ids.len()
                                    {
                                        let target_track = self.track_ids[target_idx];
                                        let target_is_instrument = self.track_kinds[target_idx];

                                        // Type compatibility: note clips to instrument tracks,
                                        // audio clips to audio tracks
                                        if *is_note_clip == target_is_instrument {
                                            let edit = Message::Arrangement(
                                                ArrangementMsg::MoveClipToTrack {
                                                    source_track: track_id,
                                                    target_track,
                                                    clip_id: *clip_id,
                                                    is_note_clip: *is_note_clip,
                                                },
                                            )
                                            .in_undo_gesture(*undo_gesture);
                                            return (canvas::event::Status::Captured, Some(edit));
                                        }
                                    }
                                }

                                if *is_note_clip {
                                    let edit = Message::Arrangement(
                                        ArrangementMsg::MoveNoteClipPosition {
                                            track_id,
                                            clip_id: *clip_id,
                                            new_position_beats: snapped,
                                        },
                                    )
                                    .in_undo_gesture(*undo_gesture);
                                    return (canvas::event::Status::Captured, Some(edit));
                                } else {
                                    let spb = self.spb();
                                    let new_sample_pos = (snapped * spb) as u64;
                                    let edit =
                                        Message::Arrangement(ArrangementMsg::MoveAudioClip {
                                            track_id,
                                            clip_id: *clip_id,
                                            new_position: new_sample_pos,
                                        })
                                        .in_undo_gesture(*undo_gesture);
                                    return (canvas::event::Status::Captured, Some(edit));
                                }
                            }
                            ClipDragAction::ResizeClip {
                                undo_gesture,
                                clip_id,
                                is_note_clip,
                                clip_start_beat,
                            } => {
                                let current_beat = self.x_to_beat(local_x);
                                let min_duration = if self.grid.snap_enabled {
                                    self.grid.effective_grid(self.pixels_per_beat()).beat_size()
                                } else {
                                    0.01
                                };
                                let new_dur = (current_beat - clip_start_beat).max(min_duration);
                                let snapped = self.snapped_beat(new_dur).max(min_duration);

                                if *is_note_clip {
                                    let edit =
                                        Message::Arrangement(ArrangementMsg::ResizeSelectedClips {
                                            anchor: ArrangementSelection::NoteClip {
                                                track_id,
                                                clip_id: *clip_id,
                                            },
                                            new_duration_beats: snapped,
                                        })
                                        .in_undo_gesture(*undo_gesture);
                                    return (canvas::event::Status::Captured, Some(edit));
                                } else {
                                    let edit =
                                        Message::Arrangement(ArrangementMsg::ResizeSelectedClips {
                                            anchor: ArrangementSelection::AudioClip {
                                                track_id,
                                                clip_id: *clip_id,
                                            },
                                            new_duration_beats: snapped,
                                        })
                                        .in_undo_gesture(*undo_gesture);
                                    return (canvas::event::Status::Captured, Some(edit));
                                }
                            }
                            ClipDragAction::FadeClip(drag) => {
                                let edit = Message::Arrangement(ArrangementMsg::SetAudioClipFade {
                                    track_id,
                                    clip_id: drag.clip_id,
                                    edge: drag.edge,
                                    frames: drag.frames_at_x(local_x),
                                })
                                .in_undo_gesture(drag.undo_gesture);
                                return (canvas::event::Status::Captured, Some(edit));
                            }
                            ClipDragAction::FadeCurve(drag) => {
                                let edit =
                                    Message::Arrangement(ArrangementMsg::SetAudioClipFadeCurve {
                                        track_id,
                                        clip_id: drag.clip_id,
                                        edge: drag.edge,
                                        curve: drag.curve_at_y(local.y),
                                    })
                                    .in_undo_gesture(drag.undo_gesture);
                                return (canvas::event::Status::Captured, Some(edit));
                            }
                            ClipDragAction::PanViewport {
                                start_local_x,
                                start_scroll_beats,
                            } => {
                                let target_scroll = (start_scroll_beats
                                    - geometry.beats_for_width(local_x - start_local_x))
                                .max(0.0);
                                let delta = target_scroll - self.scroll_offset_beats;
                                return (
                                    canvas::event::Status::Captured,
                                    (delta.abs() > f64::EPSILON).then_some(Message::View(
                                        ViewMsg::ScrollArrangement(delta),
                                    )),
                                );
                            }
                        }
                    }
                }
            }

            canvas::Event::Mouse(iced::mouse::Event::ButtonReleased(
                iced::mouse::Button::Middle,
            )) => {
                if matches!(state.drag, Some(ClipDragAction::PanViewport { .. })) {
                    state.drag = None;
                    return (canvas::event::Status::Captured, None);
                }
            }

            // -- Release: end drag or drop sample --
            canvas::Event::Mouse(iced::mouse::Event::ButtonReleased(iced::mouse::Button::Left)) => {
                // Drag-and-drop from the sample browser wins over a local
                // drag: if a sample is being dragged and the cursor is
                // inside this lane on release, emit a drop message.
                if self.sample_drop_active {
                    if let Some(pos) = cursor.position_in(bounds) {
                        if self.is_instrument {
                            state.drag = None;
                            return (
                                canvas::event::Status::Captured,
                                Some(Message::Browser(BrowserMsg::CancelDrag(
                                    "Invalid target: audio cannot be imported to a MIDI/instrument lane"
                                        .into(),
                                ))),
                            );
                        }
                        // Snap the drop position to the nearest beat so it
                        // matches the indicator drawn in `draw`.
                        let beat = self.snapped_beat(self.x_to_beat(pos.x).max(0.0));
                        let spb = self.spb();
                        let position_samples = if spb > 0.0 { (beat * spb) as u64 } else { 0 };
                        state.drag = None;
                        return (
                            canvas::event::Status::Captured,
                            Some(Message::DropSampleOnArrangement {
                                track_id,
                                position_samples,
                            }),
                        );
                    }
                }

                if let Some(ref drag) = state.drag {
                    let msg = match drag {
                        ClipDragAction::PendingSeek { beat, .. } => {
                            // Short click → seek + clear selection
                            Some(Message::Transport(TransportMsg::SeekToBeat(*beat)))
                        }
                        ClipDragAction::RegionSelect { .. } => {
                            // Drops the box; the clip selection it produced
                            // and the time range it set both stay.
                            Some(Message::Arrangement(ArrangementMsg::EndMarqueeSelect))
                        }
                        _ => None,
                    };
                    state.drag = None;
                    return (canvas::event::Status::Captured, msg);
                }
            }

            // -- Scroll: pan / Shift+scroll: zoom --
            canvas::Event::Mouse(iced::mouse::Event::WheelScrolled { delta }) => {
                if cursor.is_over(bounds) {
                    let (dx, dy) = crate::timeline_geometry::wheel_delta_pixels(delta);
                    // Horizontal scroll for panning
                    if dx.abs() > dy.abs() {
                        return (
                            canvas::event::Status::Captured,
                            Some(Message::View(ViewMsg::ScrollArrangement(
                                -self.geometry().beats_for_width(dx),
                            ))),
                        );
                    }
                    // Shift+scroll for zoom
                    if state.shift_held && dy.abs() > 0.0 {
                        let anchor_x = cursor
                            .position_in(bounds)
                            .map(|position| position.x)
                            .unwrap_or(bounds.width / 2.0);
                        return (
                            canvas::event::Status::Captured,
                            Some(Message::View(ViewMsg::ZoomAround {
                                factor: crate::timeline_geometry::zoom_factor_from_pixels(dy),
                                anchor_x,
                            })),
                        );
                    }
                    // Plain scroll for horizontal panning
                    if dy.abs() > 0.0 {
                        if self.vertical_wheel_routing
                            == VerticalWheelRouting::BubbleToTrackScroller
                        {
                            return (canvas::event::Status::Ignored, None);
                        }
                        return (
                            canvas::event::Status::Captured,
                            Some(Message::View(ViewMsg::ScrollArrangement(
                                self.geometry().beats_for_width(dy),
                            ))),
                        );
                    }
                }
            }

            // Delete/Backspace are handled centrally by the global
            // DeleteKeyPressed shortcut (context-aware: selected notes
            // first, then clips). The old canvas binding here raced
            // the piano roll's and won, deleting the clip while a
            // note was selected; it could even delete the whole track.

            // -- Lane-local keyboard shortcuts (Ctrl+D/E/J) --
            // Track creation is global and must not be handled here: this
            // canvas is instantiated once per track, so one Ctrl+T event would
            // otherwise publish one AddTrack message per existing lane.
            canvas::Event::Keyboard(iced::keyboard::Event::KeyPressed {
                key: iced::keyboard::Key::Character(ref c),
                modifiers,
                ..
            }) => {
                if crate::app::command_held(modifiers, crate::app::ON_MACOS) {
                    match c.as_str() {
                        "d" if !self.selected_clips.is_empty() => {
                            return (
                                canvas::event::Status::Captured,
                                Some(Message::Arrangement(ArrangementMsg::DuplicateSelectedClip)),
                            );
                        }
                        "e" => {
                            return (
                                canvas::event::Status::Captured,
                                Some(Message::split_selected_at_playhead()),
                            );
                        }
                        "j" if !self.selected_clips.is_empty() => {
                            return (
                                canvas::event::Status::Captured,
                                Some(Message::join_selected_clips()),
                            );
                        }
                        _ => {}
                    }
                }
            }

            // -- Track shift key state for multi-select --
            canvas::Event::Keyboard(iced::keyboard::Event::ModifiersChanged(modifiers)) => {
                state.shift_held = modifiers.shift();
            }

            _ => {}
        }

        (canvas::event::Status::Ignored, None)
    }
}

// ── Arrangement Minimap ──
