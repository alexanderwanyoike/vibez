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
    ArrangementSelection, ContextMenuTarget, GridConfig, ProjectTrack, TrackTimelineContent,
    UndoGestureId,
};
use crate::timeline_geometry::TimelineGeometry;
use crate::widgets::local_drag::LocalDrag;
use vibez_core::id::{ClipId, TrackId};

use super::*;

/// Drag action in progress on the clip canvas.
#[derive(Debug, Clone)]
pub enum ClipDragAction {
    MoveClip {
        undo_gesture: UndoGestureId,
        clip_id: ClipId,
        is_note_clip: bool,
        /// Initial local x pixel where the drag started.
        start_local_x: f32,
        original_position_beats: f64,
        start_y: f32,
    },
    ResizeClip {
        undo_gesture: UndoGestureId,
        clip_id: ClipId,
        is_note_clip: bool,
        clip_start_beat: f64,
    },
    PendingSeek {
        beat: f64,
        start_x: f32,
    },
    RegionSelect {
        anchor_beat: f64,
    },
    PanViewport {
        start_local_x: f32,
        start_scroll_beats: f64,
    },
}

/// Interaction state for clip canvas.
#[derive(Debug, Default)]
pub struct ClipInteractionState {
    pub drag: Option<ClipDragAction>,
    pub shift_held: bool,
}

/// Canvas for ONE track's clip area (waveforms, borders, names, playhead overlay).
pub struct TrackClipCanvas {
    pub track_id: TrackId,
    pub track_index: usize,
    pub total_tracks: usize,
    pub track_ids: Vec<TrackId>,
    pub track_kinds: Vec<bool>, // is_instrument flags
    pub selected_clips: HashSet<ClipId>,
    pub clips: Vec<TimelineClip>,
    pub note_clips: Vec<TimelineNoteClip>,
    /// Transient Section Record visualization. It is deliberately excluded
    /// from hit testing and edit messages.
    pub recording_preview: Option<TimelineNoteClip>,
    pub playhead_beats: f64,
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
                loop_enabled: c.loop_enabled,
                loop_start: c.loop_start,
                loop_end: c.loop_end,
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
                        .map(|n| (n.pitch, n.start_beat, n.duration_beats))
                        .collect()
                } else {
                    Vec::new()
                },
                loop_enabled: c.loop_enabled,
                loop_start_beats: c.loop_start_beats,
                loop_end_beats: c.loop_end_beats,
            })
            .collect();
        Self {
            track_id,
            track_index,
            total_tracks,
            track_ids,
            track_kinds,
            selected_clips,
            clips,
            note_clips,
            recording_preview: None,
            playhead_beats,
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

    pub fn with_recording_preview(mut self, preview: TimelineNoteClip) -> Self {
        self.recording_preview = Some(preview);
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
                ClipDragAction::RegionSelect { .. } => mouse::Interaction::Crosshair,
                ClipDragAction::PendingSeek { .. } => mouse::Interaction::Pointer,
                ClipDragAction::PanViewport { .. } => mouse::Interaction::Grabbing,
            };
        }

        if let Some(pos) = cursor.position_in(bounds) {
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
                                original_position_beats: pos_beats,
                                start_y: pos.y,
                            });
                        } else {
                            // Body → seek / region-select (like empty space)
                            let beat = self.x_to_beat(pos.x);
                            state.drag = Some(ClipDragAction::PendingSeek {
                                beat,
                                start_x: pos.x,
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
                            } => {
                                let dx = (local_x - start_x).abs();
                                if dx > 4.0 {
                                    let anchor_snapped = self.snapped_beat(*anchor);
                                    let beat = geometry.x_to_beat(local_x);
                                    let current = self.snapped_beat(beat);
                                    let start = anchor_snapped.min(current);
                                    let end = anchor_snapped.max(current);
                                    state.drag = Some(ClipDragAction::RegionSelect {
                                        anchor_beat: anchor_snapped,
                                    });
                                    if end > start {
                                        return (
                                            canvas::event::Status::Captured,
                                            Some(Message::Arrangement(
                                                ArrangementMsg::SetTimeSelection {
                                                    start_beats: start,
                                                    end_beats: end,
                                                    track_id: Some(track_id),
                                                },
                                            )),
                                        );
                                    }
                                }
                                return (canvas::event::Status::Captured, None);
                            }
                            ClipDragAction::RegionSelect { anchor_beat } => {
                                let beat = geometry.x_to_beat(local_x);
                                let current = self.snapped_beat(beat);
                                let start = anchor_beat.min(current);
                                let end = anchor_beat.max(current);
                                if end > start {
                                    return (
                                        canvas::event::Status::Captured,
                                        Some(Message::Arrangement(
                                            ArrangementMsg::SetTimeSelection {
                                                start_beats: start,
                                                end_beats: end,
                                                track_id: Some(track_id),
                                            },
                                        )),
                                    );
                                }
                                return (canvas::event::Status::Captured, None);
                            }
                            ClipDragAction::MoveClip {
                                undo_gesture,
                                clip_id,
                                is_note_clip,
                                start_local_x,
                                original_position_beats,
                                start_y,
                            } => {
                                let delta_px = local_x - start_local_x;
                                let delta_beats = geometry.beats_for_width(delta_px);
                                let new_pos = (original_position_beats + delta_beats).max(0.0);

                                let snapped = self.snapped_beat(new_pos);

                                // Check for cross-track drag
                                let local_y = local.y;
                                let dy = local_y - start_y;
                                let track_height = 70.0_f32;

                                if dy.abs() > track_height * 0.6 {
                                    let track_offset = (dy / track_height).round() as i32;
                                    let target_idx = (self.track_index as i32 + track_offset)
                                        .clamp(0, self.total_tracks as i32 - 1)
                                        as usize;

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
                            Some(Message::set_time_selection_active(true))
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
                if modifiers.control() {
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

#[cfg(test)]
mod tests {
    use super::*;
    use iced::keyboard::key::{Code, Physical};
    use iced::keyboard::{Event, Key, Location, Modifiers};
    use iced::{Point, Size};
    use vibez_core::midi::MidiNote;
    use vibez_core::perform::GrooveGrid;

    fn track_canvas(content: &TrackTimelineContent, zoom_level: f32) -> TrackClipCanvas {
        let track_id = TrackId::new();
        let track = ProjectTrack::new(track_id, "Track 1".into(), 0);
        TrackClipCanvas::from_track(
            &track,
            content,
            0.0,
            zoom_level,
            GridConfig::new(crate::state::SnapGrid::EIGHTH, true, false, 0),
            0.0,
            800.0,
            16.0,
            44_100,
            true,
            Color::BLACK,
            120.0,
            track_id,
            0,
            1,
            vec![track_id],
            vec![false],
            HashSet::new(),
            false,
            0.0,
            0.0,
            false,
            0.0,
            0.0,
            None,
            false,
            None,
            None,
        )
    }

    fn empty_track_canvas() -> TrackClipCanvas {
        track_canvas(&TrackTimelineContent::default(), 1.0)
    }

    fn note_clip(position_beats: f64) -> crate::state::UiNoteClip {
        crate::state::UiNoteClip {
            id: ClipId::new(),
            name: "Dense capture".into(),
            position_beats,
            duration_beats: 32.0,
            notes: (0..64)
                .map(|index| MidiNote {
                    pitch: 36 + (index % 12) as u8,
                    velocity: 100,
                    start_beat: index as f64 * 0.25,
                    duration_beats: 0.2,
                })
                .collect(),
            selected_notes: HashSet::new(),
            loop_enabled: false,
            loop_start_beats: 0.0,
            loop_end_beats: 0.0,
            groove_grid: GrooveGrid::Off,
        }
    }

    #[test]
    fn zoomed_out_note_clips_skip_individual_note_geometry() {
        let mut content = TrackTimelineContent::default();
        content.note_clips.push(note_clip(0.0));

        let canvas = track_canvas(&content, 0.01);

        assert_eq!(canvas.note_clips.len(), 1);
        assert!(canvas.note_clips[0].notes.is_empty());
    }

    #[test]
    fn track_canvas_materialises_only_the_visible_note_clips() {
        let mut content = TrackTimelineContent::default();
        let visible = note_clip(0.0);
        let visible_id = visible.id;
        content.note_clips.push(visible);
        content.note_clips.push(note_clip(128.0));

        let canvas = track_canvas(&content, 1.0);

        assert_eq!(canvas.note_clips.len(), 1);
        assert_eq!(canvas.note_clips[0].clip_id, visible_id);
        assert_eq!(canvas.note_clips[0].notes.len(), 64);
    }

    #[test]
    fn one_pixel_shift_wheel_event_requests_continuous_cursor_anchored_zoom() {
        let canvas = empty_track_canvas();
        let mut state = ClipInteractionState {
            shift_held: true,
            ..ClipInteractionState::default()
        };
        let bounds = Rectangle::new(Point::ORIGIN, Size::new(800.0, 80.0));
        let (status, message) = <TrackClipCanvas as canvas::Program<Message>>::update(
            &canvas,
            &mut state,
            canvas::Event::Mouse(iced::mouse::Event::WheelScrolled {
                delta: iced::mouse::ScrollDelta::Pixels { x: 0.0, y: 1.0 },
            }),
            bounds,
            mouse::Cursor::Available(Point::new(320.0, 20.0)),
        );

        assert_eq!(status, canvas::event::Status::Captured);
        assert!(matches!(
            message,
            Some(Message::View(ViewMsg::ZoomAround { factor, anchor_x }))
                if factor > 1.0 && factor < 1.01 && anchor_x == 320.0
        ));
    }

    #[test]
    fn middle_drag_pans_continuously_without_seeking_or_selecting() {
        let mut canvas = empty_track_canvas();
        canvas.scroll_offset_beats = 32.0;
        let mut state = ClipInteractionState::default();
        let bounds = Rectangle::new(Point::ORIGIN, Size::new(800.0, 80.0));

        let (press_status, press_message) = <TrackClipCanvas as canvas::Program<Message>>::update(
            &canvas,
            &mut state,
            canvas::Event::Mouse(iced::mouse::Event::ButtonPressed(
                iced::mouse::Button::Middle,
            )),
            bounds,
            mouse::Cursor::Available(Point::new(400.0, 20.0)),
        );
        assert_eq!(press_status, canvas::event::Status::Captured);
        assert!(press_message.is_none());

        let (drag_status, drag_message) = <TrackClipCanvas as canvas::Program<Message>>::update(
            &canvas,
            &mut state,
            canvas::Event::Mouse(iced::mouse::Event::CursorMoved {
                position: Point::new(420.0, 20.0),
            }),
            bounds,
            mouse::Cursor::Available(Point::new(420.0, 20.0)),
        );
        assert_eq!(drag_status, canvas::event::Status::Captured);
        assert!(matches!(
            drag_message,
            Some(Message::View(ViewMsg::ScrollArrangement(delta)))
                if (delta + 1.0).abs() < f64::EPSILON
        ));

        let (release_status, release_message) =
            <TrackClipCanvas as canvas::Program<Message>>::update(
                &canvas,
                &mut state,
                canvas::Event::Mouse(iced::mouse::Event::ButtonReleased(
                    iced::mouse::Button::Middle,
                )),
                bounds,
                mouse::Cursor::Available(Point::new(420.0, 20.0)),
            );
        assert_eq!(release_status, canvas::event::Status::Captured);
        assert!(release_message.is_none());
        assert!(state.drag.is_none());
    }

    fn right_click(
        canvas: &TrackClipCanvas,
        position: Point,
    ) -> (canvas::event::Status, Option<Message>) {
        <TrackClipCanvas as canvas::Program<Message>>::update(
            canvas,
            &mut ClipInteractionState::default(),
            canvas::Event::Mouse(iced::mouse::Event::ButtonPressed(
                iced::mouse::Button::Right,
            )),
            Rectangle::new(Point::ORIGIN, Size::new(800.0, 80.0)),
            mouse::Cursor::Available(position),
        )
    }

    #[test]
    fn physical_right_click_opens_clip_and_empty_arrange_context_menus() {
        let mut canvas = empty_track_canvas();
        let (status, message) = right_click(&canvas, Point::new(300.0, 20.0));
        assert_eq!(status, canvas::event::Status::Captured);
        assert!(matches!(
            message,
            Some(Message::View(ViewMsg::ShowContextMenu {
                target: ContextMenuTarget::ArrangementEmpty,
                ..
            }))
        ));

        let clip_id = ClipId::new();
        canvas.clips.push(TimelineClip {
            clip_id,
            position: 0,
            duration: 44_100,
            name: "Clip".into(),
            peaks: Arc::new(Vec::new()),
            loop_enabled: false,
            loop_start: 0,
            loop_end: 0,
            warp_stale: false,
        });
        let (status, message) = right_click(&canvas, Point::new(10.0, 10.0));
        assert_eq!(status, canvas::event::Status::Captured);
        assert!(matches!(
            message,
            Some(Message::View(ViewMsg::ShowContextMenu {
                target: ContextMenuTarget::Clip {
                    clip_id: id,
                    is_note_clip: false,
                    ..
                },
                ..
            })) if id == clip_id
        ));
    }

    #[test]
    fn per_track_canvas_ignores_global_track_creation_shortcuts() {
        let canvas = empty_track_canvas();
        let bounds = Rectangle::new(Point::ORIGIN, Size::new(800.0, 80.0));

        for modifiers in [Modifiers::CTRL, Modifiers::CTRL | Modifiers::SHIFT] {
            let event = canvas::Event::Keyboard(Event::KeyPressed {
                key: Key::Character("t".into()),
                modified_key: Key::Character("t".into()),
                physical_key: Physical::Code(Code::KeyT),
                location: Location::Standard,
                modifiers,
                text: None,
            });
            let mut state = ClipInteractionState::default();

            let (status, message) = <TrackClipCanvas as canvas::Program<Message>>::update(
                &canvas,
                &mut state,
                event,
                bounds,
                mouse::Cursor::Unavailable,
            );

            assert_eq!(status, canvas::event::Status::Ignored);
            assert!(message.is_none());
        }
    }

    #[test]
    fn recording_preview_is_visible_but_not_hit_testable() {
        let preview_id = ClipId::new();
        let canvas = empty_track_canvas().with_recording_preview(TimelineNoteClip {
            clip_id: preview_id,
            position_beats: 0.0,
            duration_beats: 4.0,
            name: "● RECORDING LIVE".into(),
            notes: vec![(60, 0.0, 0.5)],
            loop_enabled: false,
            loop_start_beats: 0.0,
            loop_end_beats: 0.0,
        });

        assert_eq!(
            canvas.recording_preview.as_ref().unwrap().clip_id,
            preview_id
        );
        assert!(canvas.hit_test(10.0).is_none());
    }
}

// ── Arrangement Minimap ──
