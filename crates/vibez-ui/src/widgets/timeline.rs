use std::collections::HashSet;

use iced::mouse;
use iced::widget::canvas;
use iced::{Color, Rectangle, Renderer, Theme};

use crate::domains::arrangement::ArrangementMsg;
use crate::domains::transport::TransportMsg;
use crate::message::Message;
use crate::state::{ArrangementSelection, ContextMenuTarget, UiTrack};
use crate::theme;
use vibez_core::id::{ClipId, TrackId};

// ── Lightweight data types for rendering ──

/// Lightweight copy of clip data for rendering.
pub struct TimelineClip {
    pub clip_id: ClipId,
    pub position: u64,
    pub duration: u64,
    pub name: String,
    /// Pre-computed waveform peaks for mini display (per pixel column).
    pub peaks: Vec<(f32, f32)>,
    pub loop_enabled: bool,
    pub loop_start: u64,
    pub loop_end: u64,
    /// True when this clip is warped but its `warped_to_bpm` no longer
    /// matches the current project BPM. The canvas draws a diagonal
    /// stripe overlay so the user can see at a glance that a re-warp
    /// is needed.
    pub warp_stale: bool,
}

/// Lightweight copy of a note clip for timeline rendering.
pub struct TimelineNoteClip {
    pub clip_id: ClipId,
    pub position_beats: f64,
    pub duration_beats: f64,
    pub name: String,
    pub notes: Vec<(u8, f64, f64)>, // (pitch, start_beat, duration_beats)
    pub loop_enabled: bool,
    pub loop_start_beats: f64,
    pub loop_end_beats: f64,
}

/// Compute waveform peaks for a clip, with loop-aware wrapping.
/// Uses `peak_in_range` on contiguous segments for O(pixels) cost regardless of clip length.
pub fn compute_clip_peaks(clip: &crate::state::UiClip) -> Vec<(f32, f32)> {
    let num_peaks = (clip.duration as usize / 100).clamp(1, 1000);
    let looping = clip.loop_enabled && clip.loop_end > clip.loop_start;
    let loop_start = clip.loop_start as usize;
    let loop_end = clip.loop_end as usize;
    let loop_len = if looping { loop_end - loop_start } else { 0 };
    let channels = clip.audio.num_channels();
    if channels == 0 {
        return vec![(0.0, 0.0); num_peaks];
    }

    let peak_for_range = |src_start: usize, src_end: usize| -> (f32, f32) {
        let mut mn = 0.0f32;
        let mut mx = 0.0f32;
        for ch in 0..channels {
            let (ch_min, ch_max) = clip.audio.peak_in_range(ch, src_start, src_end);
            mn = mn.min(ch_min);
            mx = mx.max(ch_max);
        }
        (mn, mx)
    };

    // Cache full loop region peak for spans >= loop_len
    let full_loop_peak = if looping {
        Some(peak_for_range(loop_start, loop_end))
    } else {
        None
    };

    (0..num_peaks)
        .map(|i| {
            let cf_start = i * clip.duration as usize / num_peaks;
            let cf_end = (i + 1) * clip.duration as usize / num_peaks;
            let span = cf_end.saturating_sub(cf_start).max(1);

            if !looping {
                let src_start = clip.source_offset as usize + cf_start;
                let src_end = clip.source_offset as usize + cf_end;
                peak_for_range(src_start, src_end)
            } else if span >= loop_len {
                full_loop_peak.unwrap()
            } else {
                let raw_start = clip.source_offset as usize + cf_start;
                let raw_end = clip.source_offset as usize + cf_end;
                let src_start = if raw_start >= loop_end {
                    loop_start + (raw_start - loop_start) % loop_len
                } else {
                    raw_start
                };
                let src_end = if raw_end >= loop_end {
                    loop_start + (raw_end - loop_start) % loop_len
                } else {
                    raw_end
                };

                if src_start <= src_end {
                    peak_for_range(src_start, src_end.max(src_start + 1))
                } else {
                    // Wraps around loop boundary
                    let (mn1, mx1) = peak_for_range(src_start, loop_end);
                    let (mn2, mx2) = peak_for_range(loop_start, src_end.max(loop_start + 1));
                    (mn1.min(mn2), mx1.max(mx2))
                }
            }
        })
        .collect()
}

// ── RulerWidget ──

/// Canvas widget that draws a beat-based ruler with bar.beat labels.
pub struct RulerWidget {
    pub playhead_beats: f64,
    pub bpm: f64,
    pub zoom_level: f32,
    pub scroll_offset_beats: f64,
    pub total_beats: f64,
    pub loop_enabled: bool,
    pub loop_start_beats: f64,
    pub loop_end_beats: f64,
    pub time_selection_active: bool,
    pub selection_start_beats: f64,
    pub selection_end_beats: f64,
}

/// Active drag action on the ruler.
#[derive(Debug, Clone)]
enum RulerDragAction {
    PendingSeek { beat: f64, start_x: f32 },
    RegionSelect { anchor_beat: f64 },
}

/// Interaction state for the ruler widget.
#[derive(Debug, Default)]
pub struct RulerInteractionState {
    drag: Option<RulerDragAction>,
    shift_held: bool,
}

impl RulerWidget {
    fn pixels_per_beat(&self) -> f32 {
        20.0 * self.zoom_level
    }

    fn visible_beats(&self, width: f32) -> f64 {
        width as f64 / self.pixels_per_beat() as f64
    }

    fn beat_to_x(&self, beat: f64, _width: f32) -> f32 {
        ((beat - self.scroll_offset_beats) * self.pixels_per_beat() as f64) as f32
    }
}

impl canvas::Program<Message> for RulerWidget {
    type State = RulerInteractionState;

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let w = bounds.width;
        let h = bounds.height;

        // Background
        frame.fill_rectangle(iced::Point::ORIGIN, iced::Size::new(w, h), theme::RULER_BG);

        if self.bpm > 0.0 {
            let beats_per_bar = 4.0_f64;
            let visible = self.visible_beats(w);
            let start_beat = self.scroll_offset_beats.floor().max(0.0) as i64;
            let end_beat = (self.scroll_offset_beats + visible).ceil() as i64 + 1;

            // Adaptive label density based on zoom
            let ppb = self.pixels_per_beat();
            let pixels_per_bar = ppb * beats_per_bar as f32;

            // Adaptive: ensure ~60px minimum between labeled bars
            let bar_step: i64 = if pixels_per_bar >= 60.0 {
                1
            } else if pixels_per_bar >= 30.0 {
                2
            } else if pixels_per_bar >= 15.0 {
                4
            } else if pixels_per_bar >= 8.0 {
                8
            } else if pixels_per_bar >= 4.0 {
                16
            } else {
                32
            };

            for beat_i in start_beat..end_beat {
                let beat = beat_i as f64;
                let x = self.beat_to_x(beat, w);

                if x < -10.0 || x > w + 10.0 {
                    continue;
                }

                let bar_index = (beat / beats_per_bar).floor() as i64;
                let beat_in_bar = (beat % beats_per_bar) as usize;
                let is_bar = beat_in_bar == 0;

                if is_bar {
                    let show_label = (bar_index % bar_step) == 0;
                    let show_tick = show_label || pixels_per_bar >= 10.0;

                    if show_tick {
                        // Bar line (thick)
                        let tick = canvas::Path::line(
                            iced::Point::new(x, h * 0.3),
                            iced::Point::new(x, h),
                        );
                        frame.stroke(
                            &tick,
                            canvas::Stroke::default()
                                .with_color(theme::BORDER)
                                .with_width(1.5),
                        );
                    }

                    if show_label {
                        // Bar label
                        if ppb < 40.0 {
                            // Low zoom: bar numbers only ("1", "2", "3")
                            let label = format!("{}", bar_index + 1);
                            frame.fill_text(canvas::Text {
                                content: label,
                                position: iced::Point::new(x + 4.0, 3.0),
                                color: theme::RULER_TEXT,
                                size: iced::Pixels(12.0),
                                ..Default::default()
                            });
                        } else {
                            // Medium/high zoom: bar.beat ("1.1", "2.1")
                            let label = format!("{}.1", bar_index + 1);
                            frame.fill_text(canvas::Text {
                                content: label,
                                position: iced::Point::new(x + 4.0, 3.0),
                                color: theme::RULER_TEXT,
                                size: iced::Pixels(12.0),
                                ..Default::default()
                            });
                        }
                    }
                } else if ppb >= 40.0 {
                    // Beat ticks only at medium+ zoom
                    let tick =
                        canvas::Path::line(iced::Point::new(x, h * 0.65), iced::Point::new(x, h));
                    frame.stroke(
                        &tick,
                        canvas::Stroke::default()
                            .with_color(theme::DIVIDER)
                            .with_width(0.5),
                    );

                    // Beat labels only at high zoom (≥80 ppb)
                    if ppb >= 80.0 {
                        let label = format!("{}.{}", bar_index + 1, beat_in_bar + 1);
                        frame.fill_text(canvas::Text {
                            content: label,
                            position: iced::Point::new(x + 2.0, 6.0),
                            color: theme::TEXT_MUTED,
                            size: iced::Pixels(9.0),
                            ..Default::default()
                        });
                    }
                }

                // Sub-beat ticks at very high zoom (>120 ppb)
                if ppb > 120.0 {
                    for sub in 1..4 {
                        let sub_beat = beat + sub as f64 * 0.25;
                        let sub_x = self.beat_to_x(sub_beat, w);
                        if sub_x > 0.0 && sub_x < w {
                            let sub_tick = canvas::Path::line(
                                iced::Point::new(sub_x, h * 0.8),
                                iced::Point::new(sub_x, h),
                            );
                            frame.stroke(
                                &sub_tick,
                                canvas::Stroke::default()
                                    .with_color(theme::DIVIDER)
                                    .with_width(0.3),
                            );
                        }
                    }
                }
            }

            // Ruler bottom border
            let bottom_border =
                canvas::Path::line(iced::Point::new(0.0, h - 1.0), iced::Point::new(w, h - 1.0));
            frame.stroke(
                &bottom_border,
                canvas::Stroke::default()
                    .with_color(theme::BORDER)
                    .with_width(1.0),
            );
        }

        // Loop region overlay
        if self.loop_enabled && self.loop_end_beats > self.loop_start_beats {
            let loop_x1 = self.beat_to_x(self.loop_start_beats, w);
            let loop_x2 = self.beat_to_x(self.loop_end_beats, w);

            let fill_x = loop_x1.max(0.0);
            let fill_w = loop_x2.min(w) - fill_x;
            if fill_w > 0.0 {
                frame.fill_rectangle(
                    iced::Point::new(fill_x, 0.0),
                    iced::Size::new(fill_w, h),
                    theme::with_alpha(theme::ACCENT, 0.15),
                );

                // REPEAT icon centered in the loop region
                let center_x = fill_x + fill_w / 2.0;
                frame.fill_text(canvas::Text {
                    content: crate::icons::REPEAT.to_string(),
                    position: iced::Point::new(center_x - 6.0, (h - 12.0) / 2.0),
                    color: theme::with_alpha(theme::ACCENT, 0.7),
                    size: iced::Pixels(12.0),
                    font: crate::icons::ICON_FONT,
                    ..Default::default()
                });
            }

            // Bracket lines at boundaries
            if loop_x1 >= 0.0 && loop_x1 <= w {
                let bracket = canvas::Path::line(
                    iced::Point::new(loop_x1, 0.0),
                    iced::Point::new(loop_x1, h),
                );
                frame.stroke(
                    &bracket,
                    canvas::Stroke::default()
                        .with_color(theme::ACCENT)
                        .with_width(2.0),
                );
            }
            if loop_x2 >= 0.0 && loop_x2 <= w {
                let bracket = canvas::Path::line(
                    iced::Point::new(loop_x2, 0.0),
                    iced::Point::new(loop_x2, h),
                );
                frame.stroke(
                    &bracket,
                    canvas::Stroke::default()
                        .with_color(theme::ACCENT)
                        .with_width(2.0),
                );
            }
        }

        // Selection region overlay (separate from loop)
        if self.time_selection_active && self.selection_end_beats > self.selection_start_beats {
            let sel_x1 = self.beat_to_x(self.selection_start_beats, w);
            let sel_x2 = self.beat_to_x(self.selection_end_beats, w);

            let fill_x = sel_x1.max(0.0);
            let fill_w = sel_x2.min(w) - fill_x;
            if fill_w > 0.0 {
                frame.fill_rectangle(
                    iced::Point::new(fill_x, 0.0),
                    iced::Size::new(fill_w, h),
                    theme::with_alpha(theme::ACCENT, 0.10),
                );
            }

            // Thinner bracket lines
            if sel_x1 >= 0.0 && sel_x1 <= w {
                let bracket =
                    canvas::Path::line(iced::Point::new(sel_x1, 0.0), iced::Point::new(sel_x1, h));
                frame.stroke(
                    &bracket,
                    canvas::Stroke::default()
                        .with_color(theme::ACCENT)
                        .with_width(1.0),
                );
            }
            if sel_x2 >= 0.0 && sel_x2 <= w {
                let bracket =
                    canvas::Path::line(iced::Point::new(sel_x2, 0.0), iced::Point::new(sel_x2, h));
                frame.stroke(
                    &bracket,
                    canvas::Stroke::default()
                        .with_color(theme::ACCENT)
                        .with_width(1.0),
                );
            }
        }

        // Playhead
        let playhead_x = self.beat_to_x(self.playhead_beats, w);
        if playhead_x >= 0.0 && playhead_x <= w {
            let playhead_line = canvas::Path::line(
                iced::Point::new(playhead_x, 0.0),
                iced::Point::new(playhead_x, h),
            );
            frame.stroke(
                &playhead_line,
                canvas::Stroke::default()
                    .with_color(theme::PLAYHEAD)
                    .with_width(2.0),
            );
        }

        vec![frame.into_geometry()]
    }

    fn update(
        &self,
        state: &mut Self::State,
        event: canvas::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> (canvas::event::Status, Option<Message>) {
        match event {
            // Left-click: PendingSeek (may become RegionSelect on drag)
            canvas::Event::Mouse(iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left)) => {
                if let Some(pos) = cursor.position_in(bounds) {
                    let ppb = self.pixels_per_beat();
                    let beat = pos.x as f64 / ppb as f64 + self.scroll_offset_beats;
                    state.drag = Some(RulerDragAction::PendingSeek {
                        beat,
                        start_x: pos.x,
                    });
                    return (canvas::event::Status::Captured, None);
                }
            }
            // Mouse move: transition PendingSeek→RegionSelect, or update region
            canvas::Event::Mouse(iced::mouse::Event::CursorMoved { .. }) => {
                if let Some(ref drag) = state.drag {
                    if let Some(pos) = cursor.position() {
                        let local_x = pos.x - bounds.x;
                        let ppb = self.pixels_per_beat();
                        let beat = local_x as f64 / ppb as f64 + self.scroll_offset_beats;

                        // Auto-scroll when dragging near ruler edges
                        if matches!(drag, RulerDragAction::RegionSelect { .. }) {
                            let edge_zone = 50.0_f32;
                            if local_x > bounds.width - edge_zone {
                                let overshoot = ((local_x - (bounds.width - edge_zone))
                                    / edge_zone)
                                    .clamp(0.0, 3.0);
                                return (
                                    canvas::event::Status::Captured,
                                    Some(Message::ScrollArrangement(overshoot as f64 * 2.0)),
                                );
                            }
                            if local_x < edge_zone && self.scroll_offset_beats > 0.0 {
                                let overshoot = ((edge_zone - local_x) / edge_zone).clamp(0.0, 3.0);
                                return (
                                    canvas::event::Status::Captured,
                                    Some(Message::ScrollArrangement(-(overshoot as f64 * 2.0))),
                                );
                            }
                        }

                        match drag {
                            RulerDragAction::PendingSeek {
                                beat: anchor,
                                start_x,
                            } => {
                                let dx = (local_x - start_x).abs();
                                if dx > 4.0 {
                                    let anchor = anchor.round();
                                    let current = beat.round();
                                    let start = anchor.min(current);
                                    let end = anchor.max(current);
                                    state.drag = Some(RulerDragAction::RegionSelect {
                                        anchor_beat: anchor,
                                    });
                                    if end > start {
                                        return (
                                            canvas::event::Status::Captured,
                                            Some(Message::Arrangement(
                                                ArrangementMsg::SetTimeSelection {
                                                    start_beats: start,
                                                    end_beats: end,
                                                    track_id: None,
                                                },
                                            )),
                                        );
                                    }
                                }
                            }
                            RulerDragAction::RegionSelect { anchor_beat } => {
                                let current = beat.round();
                                let start = anchor_beat.min(current);
                                let end = anchor_beat.max(current);
                                if end > start {
                                    return (
                                        canvas::event::Status::Captured,
                                        Some(Message::Arrangement(
                                            ArrangementMsg::SetTimeSelection {
                                                start_beats: start,
                                                end_beats: end,
                                                track_id: None,
                                            },
                                        )),
                                    );
                                }
                            }
                        }
                    }
                }
            }
            // Mouse release
            canvas::Event::Mouse(iced::mouse::Event::ButtonReleased(iced::mouse::Button::Left)) => {
                if let Some(ref drag) = state.drag {
                    let msg = match drag {
                        RulerDragAction::PendingSeek { beat, .. } => {
                            // Short click → seek + clear selection
                            if self.total_beats > 0.0 {
                                let normalized = (*beat / self.total_beats).clamp(0.0, 1.0);
                                Some(Message::Transport(TransportMsg::Seek(normalized)))
                            } else {
                                None
                            }
                        }
                        RulerDragAction::RegionSelect { .. } => {
                            // Completed region select
                            Some(Message::set_time_selection_active(true))
                        }
                    };
                    state.drag = None;
                    return (canvas::event::Status::Captured, msg);
                }
            }
            // Right-click on ruler: show time selection context menu (or arrangement-empty)
            canvas::Event::Mouse(iced::mouse::Event::ButtonPressed(iced::mouse::Button::Right)) => {
                if let Some(pos) = cursor.position_in(bounds) {
                    let screen_x = bounds.x + pos.x;
                    let screen_y = bounds.y + pos.y;

                    if self.time_selection_active
                        && self.selection_end_beats > self.selection_start_beats
                    {
                        return (
                            canvas::event::Status::Captured,
                            Some(Message::ShowContextMenu {
                                x: screen_x,
                                y: screen_y,
                                target: ContextMenuTarget::TimeSelection {
                                    start_beats: self.selection_start_beats,
                                    end_beats: self.selection_end_beats,
                                    track_id: None,
                                },
                            }),
                        );
                    }

                    return (
                        canvas::event::Status::Captured,
                        Some(Message::ShowContextMenu {
                            x: screen_x,
                            y: screen_y,
                            target: ContextMenuTarget::ArrangementEmpty,
                        }),
                    );
                }
            }
            canvas::Event::Mouse(iced::mouse::Event::WheelScrolled { delta }) => {
                if cursor.is_over(bounds) {
                    let (dx, dy) = match delta {
                        iced::mouse::ScrollDelta::Lines { x, y } => (x, y),
                        iced::mouse::ScrollDelta::Pixels { x, y } => (x / 20.0, y / 20.0),
                    };
                    // Horizontal scroll for panning
                    if dx.abs() > dy.abs() {
                        return (
                            canvas::event::Status::Captured,
                            Some(Message::ScrollArrangement(-dx as f64 * 2.0)),
                        );
                    }
                    // Shift+scroll for zoom
                    if state.shift_held && dy.abs() > 0.0 {
                        if dy > 0.0 {
                            return (canvas::event::Status::Captured, Some(Message::ZoomIn));
                        } else {
                            return (canvas::event::Status::Captured, Some(Message::ZoomOut));
                        }
                    }
                    // Plain scroll for horizontal panning
                    if dy.abs() > 0.0 {
                        return (
                            canvas::event::Status::Captured,
                            Some(Message::ScrollArrangement(dy as f64 * 2.0)),
                        );
                    }
                }
            }
            // Track shift key state for zoom
            canvas::Event::Keyboard(iced::keyboard::Event::ModifiersChanged(modifiers)) => {
                state.shift_held = modifiers.shift();
            }
            _ => {}
        }
        (canvas::event::Status::Ignored, None)
    }

    fn mouse_interaction(
        &self,
        state: &Self::State,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if let Some(ref drag) = state.drag {
            return match drag {
                RulerDragAction::RegionSelect { .. } => mouse::Interaction::Crosshair,
                RulerDragAction::PendingSeek { .. } => mouse::Interaction::Pointer,
            };
        }
        if cursor.is_over(bounds) {
            return mouse::Interaction::Pointer;
        }
        mouse::Interaction::default()
    }
}

// ── TrackClipCanvas ──

/// Pixel threshold for resize handle on right edge of clip.
const RESIZE_EDGE_PX: f32 = 8.0;

/// Height of the clip title bar zone (move/resize). Below this is the body zone (seek/region select).
const CLIP_TITLE_HEIGHT: f32 = 18.0;
/// Top padding of clips within the track canvas.
const CLIP_Y: f32 = 4.0;

/// Drag action in progress on the clip canvas.
#[derive(Debug, Clone)]
pub enum ClipDragAction {
    MoveClip {
        clip_id: ClipId,
        is_note_clip: bool,
        /// Initial local x pixel where the drag started.
        start_local_x: f32,
        original_position_beats: f64,
        start_y: f32,
    },
    ResizeClip {
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
    pub playhead_beats: f64,
    pub zoom_level: f32,
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
    /// The track name this canvas was constructed with. Drawn on the drop
    /// indicator so the user can verify which lane will receive the drop.
    pub track_name: String,
}

impl TrackClipCanvas {
    #[allow(clippy::too_many_arguments)]
    pub fn from_track(
        track: &UiTrack,
        playhead_beats: f64,
        zoom_level: f32,
        scroll_offset_beats: f64,
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
    ) -> Self {
        let clips = track
            .clips
            .iter()
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
        let note_clips = track
            .note_clips
            .iter()
            .map(|c| TimelineNoteClip {
                clip_id: c.id,
                position_beats: c.position_beats,
                duration_beats: c.duration_beats,
                name: c.name.clone(),
                notes: c
                    .notes
                    .iter()
                    .map(|n| (n.pitch, n.start_beat, n.duration_beats))
                    .collect(),
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
            playhead_beats,
            zoom_level,
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
            track_name: track.name.clone(),
        }
    }

    fn pixels_per_beat(&self) -> f32 {
        20.0 * self.zoom_level
    }

    fn beat_to_x(&self, beat: f64) -> f32 {
        ((beat - self.scroll_offset_beats) * self.pixels_per_beat() as f64) as f32
    }

    fn x_to_beat(&self, x: f32) -> f64 {
        x as f64 / self.pixels_per_beat() as f64 + self.scroll_offset_beats
    }

    /// Samples per beat.
    fn spb(&self) -> f64 {
        if self.bpm > 0.0 {
            self.sample_rate as f64 * 60.0 / self.bpm
        } else {
            1.0
        }
    }

    /// Hit test: find a clip at the given pixel x position.
    /// Returns (clip_id, is_note_clip, near_right_edge, position_beats, duration_beats).
    fn hit_test(&self, pos_x: f32) -> Option<(ClipId, bool, bool, f64, f64)> {
        let ppb = self.pixels_per_beat();
        let spb = self.spb();

        // Check audio clips
        for clip in &self.clips {
            let clip_start_beat = clip.position as f64 / spb;
            let clip_dur_beats = clip.duration as f64 / spb;
            let clip_x = self.beat_to_x(clip_start_beat);
            let clip_w = (clip_dur_beats * ppb as f64) as f32;

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
            let clip_w = (note_clip.duration_beats * ppb as f64) as f32;

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
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let w = bounds.width;
        let h = bounds.height;
        let ppb = self.pixels_per_beat();

        // True when a sample drag is active and the cursor is hovering this
        // lane. Used to paint a drop indicator.
        let drop_hover = self.sample_drop_active && cursor.position_in(bounds).is_some();

        // Background
        let bg_color = if drop_hover {
            theme::ACCENT_DIM
        } else if self.selected {
            theme::TRACK_BG_SELECTED
        } else {
            theme::TRACK_BG
        };
        frame.fill_rectangle(iced::Point::ORIGIN, iced::Size::new(w, h), bg_color);

        // Grid lines — adaptive density matching ruler
        if self.bpm > 0.0 {
            let visible = w as f64 / ppb as f64;
            let start = self.scroll_offset_beats.floor().max(0.0) as i64;
            let end = (self.scroll_offset_beats + visible).ceil() as i64 + 1;

            for beat_i in start..end {
                let x = self.beat_to_x(beat_i as f64);
                if x < -1.0 || x > w + 1.0 {
                    continue;
                }
                let is_bar = beat_i % 4 == 0;

                if is_bar {
                    // Bar lines always visible
                    let vline =
                        canvas::Path::line(iced::Point::new(x, 0.0), iced::Point::new(x, h));
                    frame.stroke(
                        &vline,
                        canvas::Stroke::default()
                            .with_color(theme::BORDER)
                            .with_width(1.0),
                    );
                } else if ppb >= 40.0 {
                    // Beat lines only at medium+ zoom
                    let vline =
                        canvas::Path::line(iced::Point::new(x, 0.0), iced::Point::new(x, h));
                    frame.stroke(
                        &vline,
                        canvas::Stroke::default()
                            .with_color(theme::DIVIDER)
                            .with_width(0.5),
                    );
                }

                // Sub-beat lines at high zoom (≥80 ppb)
                if ppb >= 80.0 {
                    for sub in 1..4 {
                        let sub_beat = beat_i as f64 + sub as f64 * 0.25;
                        let sub_x = self.beat_to_x(sub_beat);
                        if sub_x > 0.0 && sub_x < w {
                            let sub_line = canvas::Path::line(
                                iced::Point::new(sub_x, 0.0),
                                iced::Point::new(sub_x, h),
                            );
                            frame.stroke(
                                &sub_line,
                                canvas::Stroke::default()
                                    .with_color(Color {
                                        r: 0.13,
                                        g: 0.13,
                                        b: 0.13,
                                        a: 1.0,
                                    })
                                    .with_width(0.3),
                            );
                        }
                    }
                }
            }
        }

        // Draw audio clips
        if self.bpm > 0.0 {
            let spb = self.sample_rate as f64 * 60.0 / self.bpm;
            let clip_color = theme::with_alpha(self.track_color, 0.5);
            let clip_border_color = theme::darken(self.track_color, 0.7);
            let waveform_color = theme::with_alpha(self.track_color, 0.6);

            for clip in &self.clips {
                let clip_start_beat = clip.position as f64 / spb;
                let clip_dur_beats = clip.duration as f64 / spb;

                let clip_x = self.beat_to_x(clip_start_beat);
                let clip_w = (clip_dur_beats * ppb as f64) as f32;

                // Skip clips entirely outside viewport
                if clip_x + clip_w < 0.0 || clip_x > w {
                    continue;
                }

                let clip_y = 4.0;
                let clip_h = h - 8.0;

                // Clip body
                frame.fill_rectangle(
                    iced::Point::new(clip_x, clip_y),
                    iced::Size::new(clip_w.max(2.0), clip_h),
                    clip_color,
                );

                // Mini waveform using track color (drawn below title bar)
                if !clip.peaks.is_empty() && clip_w > 4.0 {
                    let body_top = clip_y + CLIP_TITLE_HEIGHT;
                    let body_h = clip_h - CLIP_TITLE_HEIGHT;
                    let center_y = body_top + body_h / 2.0;
                    let half_h = body_h / 2.0 - 2.0;
                    let pixels = clip_w as usize;
                    for px in 0..pixels {
                        let screen_x = clip_x + px as f32;
                        if screen_x < 0.0 || screen_x > w {
                            continue;
                        }
                        let peak_idx = px * clip.peaks.len() / pixels.max(1);
                        if peak_idx >= clip.peaks.len() {
                            break;
                        }
                        let (min_val, max_val) = clip.peaks[peak_idx];
                        let y_top = center_y - (max_val * half_h);
                        let y_bottom = center_y - (min_val * half_h);
                        let height = (y_bottom - y_top).max(1.0);
                        frame.fill_rectangle(
                            iced::Point::new(screen_x, y_top),
                            iced::Size::new(1.0, height),
                            waveform_color,
                        );
                    }
                }

                // Loop markers
                if clip.loop_enabled && clip.loop_end > clip.loop_start {
                    let loop_region_samples = (clip.loop_end - clip.loop_start) as f64;
                    let loop_region_beats = loop_region_samples / spb;

                    // Draw repeated sections with lower opacity
                    let repeat_color = theme::with_alpha(self.track_color, 0.25);
                    let first_loop_offset_beats = (clip.loop_end - clip.loop_start) as f64 / spb;
                    let mut repeat_beat = clip_start_beat + first_loop_offset_beats;
                    while repeat_beat < clip_start_beat + clip_dur_beats {
                        let rx = self.beat_to_x(repeat_beat);
                        let rw = (loop_region_beats * ppb as f64) as f32;
                        if rx < w && rx + rw > 0.0 {
                            frame.fill_rectangle(
                                iced::Point::new(rx, clip_y),
                                iced::Size::new(rw.min(clip_x + clip_w - rx).max(0.0), clip_h),
                                repeat_color,
                            );
                        }
                        repeat_beat += loop_region_beats;
                    }

                    // Small "L" icon
                    if clip_w > 20.0 {
                        frame.fill_text(canvas::Text {
                            content: "L".to_string(),
                            position: iced::Point::new(clip_x + clip_w - 14.0, clip_y + 3.0),
                            color: theme::ACCENT,
                            size: iced::Pixels(9.0),
                            ..Default::default()
                        });
                    }
                }

                // Selection highlight
                let is_selected = self.selected_clips.contains(&clip.clip_id);
                let border_color = if is_selected {
                    theme::ACCENT
                } else {
                    clip_border_color
                };
                let border_width = if is_selected { 2.0 } else { 1.0 };

                // Clip border
                let border = canvas::Path::rectangle(
                    iced::Point::new(clip_x, clip_y),
                    iced::Size::new(clip_w.max(2.0), clip_h),
                );
                frame.stroke(
                    &border,
                    canvas::Stroke::default()
                        .with_color(border_color)
                        .with_width(border_width),
                );

                // Title bar separator
                let title_sep_y = clip_y + CLIP_TITLE_HEIGHT;
                let title_line = canvas::Path::line(
                    iced::Point::new(clip_x, title_sep_y),
                    iced::Point::new(clip_x + clip_w.max(2.0), title_sep_y),
                );
                frame.stroke(
                    &title_line,
                    canvas::Stroke::default()
                        .with_color(theme::with_alpha(Color::BLACK, 0.3))
                        .with_width(1.0),
                );

                // Clip name label
                if clip_w > 40.0 {
                    frame.fill_text(canvas::Text {
                        content: clip.name.clone(),
                        position: iced::Point::new(clip_x + 4.0, clip_y + 3.0),
                        color: theme::TEXT,
                        size: iced::Pixels(11.0),
                        ..Default::default()
                    });
                }

                // Diagonal stripe overlay when the clip's warp is
                // stale relative to the current project tempo. Low
                // opacity so the waveform and clip colour remain
                // legible, but strong enough to catch the eye.
                if clip.warp_stale && clip_w > 4.0 {
                    let stripe_color = theme::with_alpha(theme::METER_YELLOW, 0.22);
                    let stripe_spacing = 8.0f32;
                    let stripe_stroke = 1.5f32;
                    let mut offset = -clip_h;
                    while offset < clip_w {
                        let x0 = clip_x + offset;
                        let x1 = clip_x + offset + clip_h;
                        let a_x = x0.clamp(clip_x, clip_x + clip_w);
                        let b_x = x1.clamp(clip_x, clip_x + clip_w);
                        let a_t = if (x1 - x0).abs() > 0.1 {
                            (a_x - x0) / (x1 - x0)
                        } else {
                            0.0
                        };
                        let b_t = if (x1 - x0).abs() > 0.1 {
                            (b_x - x0) / (x1 - x0)
                        } else {
                            0.0
                        };
                        let a_y = clip_y + a_t * clip_h;
                        let b_y = clip_y + b_t * clip_h;
                        if (b_x - a_x).abs() > 0.5 {
                            let line = canvas::Path::line(
                                iced::Point::new(a_x, a_y),
                                iced::Point::new(b_x, b_y),
                            );
                            frame.stroke(
                                &line,
                                canvas::Stroke::default()
                                    .with_color(stripe_color)
                                    .with_width(stripe_stroke),
                            );
                        }
                        offset += stripe_spacing;
                    }
                }
            }
        }

        // Draw note clips (for instrument tracks)
        if self.is_instrument && self.bpm > 0.0 {
            let note_clip_color = theme::with_alpha(self.track_color, 0.4);
            let note_block_color = theme::with_alpha(self.track_color, 0.8);

            for note_clip in &self.note_clips {
                let clip_x = self.beat_to_x(note_clip.position_beats);
                let clip_w = (note_clip.duration_beats * ppb as f64) as f32;

                // Skip clips outside viewport
                if clip_x + clip_w < 0.0 || clip_x > w {
                    continue;
                }

                let clip_y = 4.0;
                let clip_h = h - 8.0;

                // Clip body
                frame.fill_rectangle(
                    iced::Point::new(clip_x, clip_y),
                    iced::Size::new(clip_w.max(2.0), clip_h),
                    note_clip_color,
                );

                // Draw note blocks inside the clip (below title bar)
                if !note_clip.notes.is_empty() && clip_w > 4.0 {
                    let body_top = clip_y + CLIP_TITLE_HEIGHT;
                    let body_h = clip_h - CLIP_TITLE_HEIGHT;
                    let pitches: Vec<u8> = note_clip.notes.iter().map(|n| n.0).collect();
                    let min_pitch = *pitches.iter().min().unwrap_or(&60);
                    let max_pitch = *pitches.iter().max().unwrap_or(&72);
                    let pitch_range = (max_pitch - min_pitch + 1).max(12) as f32;

                    for &(pitch, start_beat, duration_beats) in &note_clip.notes {
                        // start_beat is clip-local (0.0 = clip start)
                        let note_x =
                            clip_x + (start_beat / note_clip.duration_beats * clip_w as f64) as f32;
                        let note_w =
                            (duration_beats / note_clip.duration_beats * clip_w as f64) as f32;
                        let note_y_frac = (max_pitch.saturating_sub(pitch)) as f32 / pitch_range;
                        let note_y = body_top + 2.0 + note_y_frac * (body_h - 4.0);
                        let note_h = ((body_h - 4.0) / pitch_range).clamp(2.0, 6.0);

                        frame.fill_rectangle(
                            iced::Point::new(note_x, note_y),
                            iced::Size::new(note_w.max(2.0), note_h),
                            note_block_color,
                        );
                    }
                }

                // Loop markers for note clips
                if note_clip.loop_enabled && note_clip.loop_end_beats > note_clip.loop_start_beats {
                    let loop_len = note_clip.loop_end_beats - note_clip.loop_start_beats;
                    let repeat_color = theme::with_alpha(self.track_color, 0.2);
                    let ghost_block_color = theme::with_alpha(self.track_color, 0.5);
                    let mut repeat_beat = note_clip.position_beats + note_clip.loop_end_beats;
                    while repeat_beat < note_clip.position_beats + note_clip.duration_beats {
                        let rx = self.beat_to_x(repeat_beat);
                        let rw = (loop_len * ppb as f64) as f32;
                        if rx < w && rx + rw > 0.0 {
                            frame.fill_rectangle(
                                iced::Point::new(rx, clip_y),
                                iced::Size::new(rw.min(clip_x + clip_w - rx).max(0.0), clip_h),
                                repeat_color,
                            );
                        }

                        // Draw ghost note blocks in this repeat (below title bar)
                        if !note_clip.notes.is_empty() && clip_w > 4.0 {
                            let gbody_top = clip_y + CLIP_TITLE_HEIGHT;
                            let gbody_h = clip_h - CLIP_TITLE_HEIGHT;
                            let pitches: Vec<u8> = note_clip.notes.iter().map(|n| n.0).collect();
                            let gmin = *pitches.iter().min().unwrap_or(&60);
                            let gmax = *pitches.iter().max().unwrap_or(&72);
                            let gpitch_range = (gmax - gmin + 1).max(12) as f32;
                            let offset = repeat_beat - note_clip.position_beats;

                            for &(pitch, start_beat, duration_beats) in &note_clip.notes {
                                // Only repeat notes within the loop region
                                if start_beat < note_clip.loop_start_beats
                                    || start_beat >= note_clip.loop_end_beats
                                {
                                    continue;
                                }
                                let gx = clip_x
                                    + ((start_beat + offset) / note_clip.duration_beats
                                        * clip_w as f64)
                                        as f32;
                                let gnw = (duration_beats / note_clip.duration_beats
                                    * clip_w as f64)
                                    as f32;
                                let gy_frac = (gmax.saturating_sub(pitch)) as f32 / gpitch_range;
                                let gy = gbody_top + 2.0 + gy_frac * (gbody_h - 4.0);
                                let gnh = ((gbody_h - 4.0) / gpitch_range).clamp(2.0, 6.0);

                                if gx < clip_x + clip_w && gx + gnw > 0.0 {
                                    frame.fill_rectangle(
                                        iced::Point::new(gx, gy),
                                        iced::Size::new(gnw.max(2.0), gnh),
                                        ghost_block_color,
                                    );
                                }
                            }
                        }

                        repeat_beat += loop_len;
                    }

                    if clip_w > 20.0 {
                        frame.fill_text(canvas::Text {
                            content: "L".to_string(),
                            position: iced::Point::new(clip_x + clip_w - 14.0, clip_y + 3.0),
                            color: theme::ACCENT,
                            size: iced::Pixels(9.0),
                            ..Default::default()
                        });
                    }
                }

                // Selection highlight
                let is_selected = self.selected_clips.contains(&note_clip.clip_id);
                let border_color = if is_selected {
                    theme::ACCENT
                } else {
                    theme::darken(self.track_color, 0.7)
                };
                let border_width = if is_selected { 2.0 } else { 1.0 };

                // Clip border
                let border = canvas::Path::rectangle(
                    iced::Point::new(clip_x, clip_y),
                    iced::Size::new(clip_w.max(2.0), clip_h),
                );
                frame.stroke(
                    &border,
                    canvas::Stroke::default()
                        .with_color(border_color)
                        .with_width(border_width),
                );

                // Title bar separator
                let title_sep_y = clip_y + CLIP_TITLE_HEIGHT;
                let title_line = canvas::Path::line(
                    iced::Point::new(clip_x, title_sep_y),
                    iced::Point::new(clip_x + clip_w.max(2.0), title_sep_y),
                );
                frame.stroke(
                    &title_line,
                    canvas::Stroke::default()
                        .with_color(theme::with_alpha(Color::BLACK, 0.3))
                        .with_width(1.0),
                );

                // Clip name label
                if clip_w > 40.0 {
                    frame.fill_text(canvas::Text {
                        content: note_clip.name.clone(),
                        position: iced::Point::new(clip_x + 4.0, clip_y + 3.0),
                        color: theme::TEXT,
                        size: iced::Pixels(11.0),
                        ..Default::default()
                    });
                }
            }
        }

        // Loop region tint
        if self.loop_enabled && self.loop_end_beats > self.loop_start_beats {
            let loop_x1 = self.beat_to_x(self.loop_start_beats);
            let loop_x2 = self.beat_to_x(self.loop_end_beats);
            let fill_x = loop_x1.max(0.0);
            let fill_w = loop_x2.min(w) - fill_x;
            if fill_w > 0.0 {
                frame.fill_rectangle(
                    iced::Point::new(fill_x, 0.0),
                    iced::Size::new(fill_w, h),
                    theme::with_alpha(theme::ACCENT, 0.06),
                );
            }
        }

        // Selection region tint. Only drawn on the lane the selection
        // originated on; ruler-drawn selections (track_id = None) still
        // show across every lane.
        let show_selection_on_this_lane = self
            .time_selection_track
            .is_none_or(|tid| tid == self.track_id);
        if show_selection_on_this_lane
            && self.time_selection_active
            && self.selection_end_beats > self.selection_start_beats
        {
            let sel_x1 = self.beat_to_x(self.selection_start_beats);
            let sel_x2 = self.beat_to_x(self.selection_end_beats);
            let fill_x = sel_x1.max(0.0);
            let fill_w = sel_x2.min(w) - fill_x;
            if fill_w > 0.0 {
                frame.fill_rectangle(
                    iced::Point::new(fill_x, 0.0),
                    iced::Size::new(fill_w, h),
                    theme::with_alpha(theme::ACCENT, 0.04),
                );
            }
        }

        // Playhead overlay
        let playhead_x = self.beat_to_x(self.playhead_beats);
        if playhead_x >= 0.0 && playhead_x <= w {
            let playhead_line = canvas::Path::line(
                iced::Point::new(playhead_x, 0.0),
                iced::Point::new(playhead_x, h),
            );
            frame.stroke(
                &playhead_line,
                canvas::Stroke::default()
                    .with_color(theme::PLAYHEAD)
                    .with_width(2.0),
            );
        }

        // Bottom separator
        let sep = canvas::Path::line(iced::Point::new(0.0, h - 1.0), iced::Point::new(w, h - 1.0));
        frame.stroke(
            &sep,
            canvas::Stroke::default()
                .with_color(theme::DIVIDER)
                .with_width(1.0),
        );

        // Drop-target indicator: bold accent border + vertical bar at the
        // cursor x + the track name overlaid so the user can verify which
        // lane will receive the drop.
        if drop_hover {
            let outline = canvas::Path::rectangle(
                iced::Point::new(1.0, 1.0),
                iced::Size::new(w - 2.0, h - 2.0),
            );
            frame.stroke(
                &outline,
                canvas::Stroke::default()
                    .with_color(theme::ACCENT)
                    .with_width(2.0),
            );
            if let Some(local) = cursor.position_in(bounds) {
                let beat = self.x_to_beat(local.x).max(0.0);
                let snapped_x = self.beat_to_x(beat.round());
                if snapped_x >= 0.0 && snapped_x <= w {
                    let drop_line = canvas::Path::line(
                        iced::Point::new(snapped_x, 0.0),
                        iced::Point::new(snapped_x, h),
                    );
                    frame.stroke(
                        &drop_line,
                        canvas::Stroke::default()
                            .with_color(theme::ACCENT)
                            .with_width(2.0),
                    );
                }
            }
            // "Dropping to <track>" label inside the lane.
            frame.fill_text(canvas::Text {
                content: format!("Drop on: {}", self.track_name),
                position: iced::Point::new(8.0, 8.0),
                color: theme::ACCENT,
                size: 14.0.into(),
                ..Default::default()
            });
        }

        vec![frame.into_geometry()]
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
                                clip_id,
                                is_note_clip,
                                clip_start_beat: pos_beats,
                            });
                        } else if in_title_bar {
                            // Title bar → move clip
                            state.drag = Some(ClipDragAction::MoveClip {
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
                            Some(Message::Arrangement(
                                ArrangementMsg::SelectArrangementClip {
                                    selection,
                                    shift_held: state.shift_held,
                                },
                            )),
                        );
                    }

                    // No clip hit. Start a PendingSeek (may become RegionSelect on drag).
                    // Also surface the track as the selection target so subsequent
                    // browser imports / dropdowns know which lane is "active".
                    if bounds.width > 0.0 {
                        let ppb = self.pixels_per_beat();
                        let beat = pos.x as f64 / ppb as f64 + self.scroll_offset_beats;
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
                            Some(Message::ShowContextMenu {
                                x: screen_x,
                                y: screen_y,
                                target: ContextMenuTarget::Clip {
                                    track_id,
                                    clip_id,
                                    is_note_clip,
                                },
                            }),
                        );
                    }

                    // No clip hit — check if within active time selection
                    if self.time_selection_active
                        && self.selection_end_beats > self.selection_start_beats
                    {
                        let ppb = self.pixels_per_beat();
                        let beat = pos.x as f64 / ppb as f64 + self.scroll_offset_beats;
                        if beat >= self.selection_start_beats && beat <= self.selection_end_beats {
                            return (
                                canvas::event::Status::Captured,
                                Some(Message::ShowContextMenu {
                                    x: screen_x,
                                    y: screen_y,
                                    target: ContextMenuTarget::TimeSelection {
                                        start_beats: self.selection_start_beats,
                                        end_beats: self.selection_end_beats,
                                        track_id: Some(self.track_id),
                                    },
                                }),
                            );
                        }
                    }

                    // No clip, no time selection — show arrangement-empty context menu
                    return (
                        canvas::event::Status::Captured,
                        Some(Message::ShowContextMenu {
                            x: screen_x,
                            y: screen_y,
                            target: ContextMenuTarget::ArrangementEmpty,
                        }),
                    );
                }
            }

            // -- Drag: move, resize, or region select --
            canvas::Event::Mouse(iced::mouse::Event::CursorMoved { .. }) => {
                // If a sample drag from the browser is in flight and the
                // cursor is over this lane, publish a hover update so the
                // global drop handler can route a release here even if
                // the release lands on a sub-pixel boundary outside
                // `cursor.position_in(bounds)`.
                if self.sample_drop_active {
                    if let Some(local) = cursor.position_in(bounds) {
                        let beat = self.x_to_beat(local.x).max(0.0).round();
                        return (
                            canvas::event::Status::Ignored,
                            Some(Message::DragHoverTrack { track_id, beat }),
                        );
                    }
                }

                if let Some(ref drag) = state.drag {
                    if let Some(pos) = cursor.position() {
                        let local_x = pos.x - bounds.x;
                        let ppb = self.pixels_per_beat();

                        match drag {
                            ClipDragAction::PendingSeek {
                                beat: anchor,
                                start_x,
                            } => {
                                let dx = (local_x - start_x).abs();
                                if dx > 4.0 {
                                    let anchor_snapped = anchor.round();
                                    let beat =
                                        local_x as f64 / ppb as f64 + self.scroll_offset_beats;
                                    let current = beat.round();
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
                                let beat = local_x as f64 / ppb as f64 + self.scroll_offset_beats;
                                let current = beat.round();
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
                                clip_id,
                                is_note_clip,
                                start_local_x,
                                original_position_beats,
                                start_y,
                            } => {
                                let delta_px = local_x - start_local_x;
                                let delta_beats = delta_px as f64 / ppb as f64;
                                let new_pos = (original_position_beats + delta_beats).max(0.0);

                                // Snap to nearest beat
                                let snapped = (new_pos * 4.0).round() / 4.0;

                                // Check for cross-track drag
                                let local_y = pos.y - bounds.y;
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
                                            return (
                                                canvas::event::Status::Captured,
                                                Some(Message::Arrangement(
                                                    ArrangementMsg::MoveClipToTrack {
                                                        source_track: track_id,
                                                        target_track,
                                                        clip_id: *clip_id,
                                                        is_note_clip: *is_note_clip,
                                                    },
                                                )),
                                            );
                                        }
                                    }
                                }

                                if *is_note_clip {
                                    return (
                                        canvas::event::Status::Captured,
                                        Some(Message::Arrangement(
                                            ArrangementMsg::MoveNoteClipPosition {
                                                track_id,
                                                clip_id: *clip_id,
                                                new_position_beats: snapped,
                                            },
                                        )),
                                    );
                                } else {
                                    let spb = self.spb();
                                    let new_sample_pos = (snapped * spb) as u64;
                                    return (
                                        canvas::event::Status::Captured,
                                        Some(Message::Arrangement(ArrangementMsg::MoveAudioClip {
                                            track_id,
                                            clip_id: *clip_id,
                                            new_position: new_sample_pos,
                                        })),
                                    );
                                }
                            }
                            ClipDragAction::ResizeClip {
                                clip_id,
                                is_note_clip,
                                clip_start_beat,
                            } => {
                                let current_beat = self.x_to_beat(local_x);
                                let new_dur = (current_beat - clip_start_beat).max(0.25);
                                // Snap to quarter beat
                                let snapped = (new_dur * 4.0).round() / 4.0;

                                if *is_note_clip {
                                    return (
                                        canvas::event::Status::Captured,
                                        Some(Message::ResizeNoteClipDuration {
                                            track_id,
                                            clip_id: *clip_id,
                                            new_duration_beats: snapped,
                                        }),
                                    );
                                } else {
                                    let spb = self.spb();
                                    let new_dur_samples = (snapped * spb) as u64;
                                    return (
                                        canvas::event::Status::Captured,
                                        Some(Message::Arrangement(
                                            ArrangementMsg::ResizeAudioClip {
                                                track_id,
                                                clip_id: *clip_id,
                                                new_duration: new_dur_samples.max(1),
                                            },
                                        )),
                                    );
                                }
                            }
                        }
                    }
                }
            }

            // -- Release: end drag or drop sample --
            canvas::Event::Mouse(iced::mouse::Event::ButtonReleased(iced::mouse::Button::Left)) => {
                // Drag-and-drop from the sample browser wins over a local
                // drag: if a sample is being dragged and the cursor is
                // inside this lane on release, emit a drop message.
                if self.sample_drop_active {
                    if let Some(pos) = cursor.position_in(bounds) {
                        // Snap the drop position to the nearest beat so it
                        // matches the indicator drawn in `draw`.
                        let beat = self.x_to_beat(pos.x).max(0.0).round();
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
                            if self.total_beats > 0.0 {
                                let normalized = (*beat / self.total_beats).clamp(0.0, 1.0);
                                Some(Message::Transport(TransportMsg::Seek(normalized)))
                            } else {
                                None
                            }
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
                    let (dx, dy) = match delta {
                        iced::mouse::ScrollDelta::Lines { x, y } => (x, y),
                        iced::mouse::ScrollDelta::Pixels { x, y } => (x / 20.0, y / 20.0),
                    };
                    // Horizontal scroll for panning
                    if dx.abs() > dy.abs() {
                        return (
                            canvas::event::Status::Captured,
                            Some(Message::ScrollArrangement(-dx as f64 * 2.0)),
                        );
                    }
                    // Shift+scroll for zoom
                    if state.shift_held && dy.abs() > 0.0 {
                        if dy > 0.0 {
                            return (canvas::event::Status::Captured, Some(Message::ZoomIn));
                        } else {
                            return (canvas::event::Status::Captured, Some(Message::ZoomOut));
                        }
                    }
                    // Plain scroll for horizontal panning
                    if dy.abs() > 0.0 {
                        return (
                            canvas::event::Status::Captured,
                            Some(Message::ScrollArrangement(dy as f64 * 2.0)),
                        );
                    }
                }
            }

            // Delete/Backspace are handled centrally by the global
            // DeleteKeyPressed shortcut (context-aware: selected notes
            // first, then clips). The old canvas binding here raced
            // the piano roll's and won, deleting the clip while a
            // note was selected; it could even delete the whole track.

            // -- Keyboard shortcuts (Ctrl+D/E/J/T) --
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
                                Some(Message::SplitSelectedAtPlayhead),
                            );
                        }
                        "j" if !self.selected_clips.is_empty() => {
                            return (
                                canvas::event::Status::Captured,
                                Some(Message::JoinSelectedClips),
                            );
                        }
                        "t" | "T" => {
                            if modifiers.shift() {
                                return (
                                    canvas::event::Status::Captured,
                                    Some(Message::Arrangement(ArrangementMsg::AddInstrumentTrack)),
                                );
                            } else {
                                return (
                                    canvas::event::Status::Captured,
                                    Some(Message::Arrangement(ArrangementMsg::AddTrack)),
                                );
                            }
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

/// Per-track data for the minimap overview.
pub struct MinimapTrack {
    pub color: Color,
    /// (start_beat, duration_beats) for each clip on this track.
    pub clips: Vec<(f64, f64)>,
}

/// Bird's-eye arrangement overview widget (Ableton-style minimap).
pub struct ArrangementMinimap {
    pub total_beats: f64,
    pub scroll_offset_beats: f64,
    pub zoom_level: f32,
    pub playhead_beats: f64,
    pub bpm: f64,
    pub loop_enabled: bool,
    pub loop_start_beats: f64,
    pub loop_end_beats: f64,
    pub tracks: Vec<MinimapTrack>,
}

impl ArrangementMinimap {
    fn visible_beats(&self, canvas_width: f32) -> f64 {
        let ppb = 20.0 * self.zoom_level;
        canvas_width as f64 / ppb as f64
    }
}

/// Interaction state for the minimap.
#[derive(Debug, Default)]
pub struct MinimapInteractionState {
    drag: Option<MinimapDragAction>,
}

#[derive(Debug)]
enum MinimapDragAction {
    /// Dragging the viewport rectangle (panning).
    PanViewport { start_x: f32, start_scroll: f64 },
    /// Clicked outside viewport — continuously scroll to cursor.
    Seeking,
}

impl canvas::Program<Message> for ArrangementMinimap {
    type State = MinimapInteractionState;

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let w = bounds.width;
        let h = bounds.height;

        // Background
        frame.fill_rectangle(
            iced::Point::ORIGIN,
            iced::Size::new(w, h),
            theme::BG_ELEVATED,
        );

        if self.total_beats <= 0.0 || self.tracks.is_empty() {
            return vec![frame.into_geometry()];
        }

        let ppb_mini = w as f64 / self.total_beats;
        let num_tracks = self.tracks.len();
        let track_h = (h / num_tracks as f32).max(2.0);

        // Draw clips for each track
        for (i, track) in self.tracks.iter().enumerate() {
            let y = i as f32 * track_h;
            let clip_color = theme::with_alpha(track.color, 0.6);
            for &(start, dur) in &track.clips {
                let cx = (start * ppb_mini) as f32;
                let cw = (dur * ppb_mini).max(1.0) as f32;
                if cx + cw < 0.0 || cx > w {
                    continue;
                }
                frame.fill_rectangle(
                    iced::Point::new(cx, y),
                    iced::Size::new(cw, track_h),
                    clip_color,
                );
            }
        }

        // Loop region overlay
        if self.loop_enabled && self.loop_end_beats > self.loop_start_beats {
            let lx1 = (self.loop_start_beats * ppb_mini) as f32;
            let lx2 = (self.loop_end_beats * ppb_mini) as f32;
            let fill_x = lx1.max(0.0);
            let fill_w = lx2.min(w) - fill_x;
            if fill_w > 0.0 {
                frame.fill_rectangle(
                    iced::Point::new(fill_x, 0.0),
                    iced::Size::new(fill_w, h),
                    theme::with_alpha(theme::ACCENT, 0.15),
                );
            }
        }

        // Viewport rectangle
        let visible = self.visible_beats(w);
        let vx_start = (self.scroll_offset_beats * ppb_mini) as f32;
        let vx_end = ((self.scroll_offset_beats + visible) * ppb_mini) as f32;
        let vx = vx_start.max(0.0);
        let vw = vx_end.min(w) - vx;
        if vw > 0.0 {
            // Subtle fill
            frame.fill_rectangle(
                iced::Point::new(vx, 0.0),
                iced::Size::new(vw, h),
                Color {
                    r: 1.0,
                    g: 1.0,
                    b: 1.0,
                    a: 0.05,
                },
            );
            // Orange border
            let rect_path = canvas::Path::new(|b| {
                b.move_to(iced::Point::new(vx, 0.5));
                b.line_to(iced::Point::new(vx + vw, 0.5));
                b.line_to(iced::Point::new(vx + vw, h - 0.5));
                b.line_to(iced::Point::new(vx, h - 0.5));
                b.close();
            });
            frame.stroke(
                &rect_path,
                canvas::Stroke::default()
                    .with_color(theme::ACCENT)
                    .with_width(1.5),
            );
        }

        // Playhead line
        let ph_x = (self.playhead_beats * ppb_mini) as f32;
        if ph_x >= 0.0 && ph_x <= w {
            let playhead =
                canvas::Path::line(iced::Point::new(ph_x, 0.0), iced::Point::new(ph_x, h));
            frame.stroke(
                &playhead,
                canvas::Stroke::default()
                    .with_color(theme::PLAYHEAD)
                    .with_width(1.0),
            );
        }

        // Bottom border
        let border =
            canvas::Path::line(iced::Point::new(0.0, h - 0.5), iced::Point::new(w, h - 0.5));
        frame.stroke(
            &border,
            canvas::Stroke::default()
                .with_color(theme::BORDER)
                .with_width(1.0),
        );

        vec![frame.into_geometry()]
    }

    fn update(
        &self,
        state: &mut Self::State,
        event: canvas::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> (canvas::event::Status, Option<Message>) {
        if self.total_beats <= 0.0 {
            return (canvas::event::Status::Ignored, None);
        }

        let ppb_mini = bounds.width as f64 / self.total_beats;
        let visible = self.visible_beats(bounds.width);

        match event {
            canvas::Event::Mouse(iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left)) => {
                if let Some(pos) = cursor.position_in(bounds) {
                    let click_beat = pos.x as f64 / ppb_mini;

                    // Check if click is inside the viewport rectangle
                    let vx_start = (self.scroll_offset_beats * ppb_mini) as f32;
                    let vx_end = ((self.scroll_offset_beats + visible) * ppb_mini) as f32;

                    if pos.x >= vx_start && pos.x <= vx_end {
                        // Start panning the viewport
                        state.drag = Some(MinimapDragAction::PanViewport {
                            start_x: pos.x,
                            start_scroll: self.scroll_offset_beats,
                        });
                        return (canvas::event::Status::Captured, None);
                    }

                    // Click outside viewport — jump to center that beat
                    let target = (click_beat - visible / 2.0).max(0.0);
                    let delta = target - self.scroll_offset_beats;
                    state.drag = Some(MinimapDragAction::Seeking);
                    return (
                        canvas::event::Status::Captured,
                        Some(Message::ScrollArrangement(delta)),
                    );
                }
            }
            canvas::Event::Mouse(iced::mouse::Event::CursorMoved { .. }) => {
                if let Some(ref drag) = state.drag {
                    if let Some(pos) = cursor.position() {
                        let local_x = pos.x - bounds.x;
                        match drag {
                            MinimapDragAction::PanViewport {
                                start_x,
                                start_scroll,
                            } => {
                                let dx = local_x - start_x;
                                let delta_beats = dx as f64 / ppb_mini;
                                let target = (start_scroll + delta_beats).max(0.0);
                                let delta = target - self.scroll_offset_beats;
                                return (
                                    canvas::event::Status::Captured,
                                    Some(Message::ScrollArrangement(delta)),
                                );
                            }
                            MinimapDragAction::Seeking => {
                                let click_beat = local_x as f64 / ppb_mini;
                                let target = (click_beat - visible / 2.0).max(0.0);
                                let delta = target - self.scroll_offset_beats;
                                return (
                                    canvas::event::Status::Captured,
                                    Some(Message::ScrollArrangement(delta)),
                                );
                            }
                        }
                    }
                }
            }
            canvas::Event::Mouse(iced::mouse::Event::ButtonReleased(iced::mouse::Button::Left))
                if state.drag.is_some() =>
            {
                state.drag = None;
                return (canvas::event::Status::Captured, None);
            }
            _ => {}
        }

        (canvas::event::Status::Ignored, None)
    }

    fn mouse_interaction(
        &self,
        state: &Self::State,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if self.total_beats <= 0.0 {
            return mouse::Interaction::default();
        }

        if let Some(ref drag) = state.drag {
            return match drag {
                MinimapDragAction::PanViewport { .. } => mouse::Interaction::Grabbing,
                MinimapDragAction::Seeking => mouse::Interaction::Pointer,
            };
        }

        if let Some(pos) = cursor.position_in(bounds) {
            let ppb_mini = bounds.width as f64 / self.total_beats;
            let visible = self.visible_beats(bounds.width);
            let vx_start = (self.scroll_offset_beats * ppb_mini) as f32;
            let vx_end = ((self.scroll_offset_beats + visible) * ppb_mini) as f32;

            if pos.x >= vx_start && pos.x <= vx_end {
                return mouse::Interaction::Grab;
            }

            return mouse::Interaction::Pointer;
        }

        mouse::Interaction::default()
    }
}
