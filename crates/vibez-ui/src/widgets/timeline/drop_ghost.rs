//! A Browser drag's Clip drawn where its drop would land, lifted like a
//! dragged Clip so it reads as not landed yet.

use std::sync::Arc;

use iced::widget::canvas;
use iced::{mouse, Color, Rectangle, Renderer, Theme};

use crate::domains::drop_ghost::{DropGhost, GhostMedia, GhostPlace};
use crate::message::Message;
use crate::timeline_geometry::TimelineGeometry;

use super::clip_colors::clip_colors;
use super::clip_frame::{clip_box, draw_clip_back, draw_clip_front};
use super::clips_draw::{draw_notes, draw_waveform, PeakScale};
use super::drag_preview::draw_drag_shadow;
use super::note_summary::{note_clip_summary, NoteSummary};
use super::CLIP_Y;

#[derive(Debug, Clone)]
enum GhostBody {
    Waveform(Arc<Vec<(f32, f32)>>),
    Notes(Arc<NoteSummary>),
}

/// The ghost as a lane draws it.
#[derive(Debug, Clone)]
pub struct LaneGhost {
    start_beats: f64,
    length_beats: f64,
    name: String,
    body: GhostBody,
}

impl LaneGhost {
    /// `ghost` laid out for a lane at `pixels_per_beat`.
    pub fn new(ghost: &DropGhost<'_>, pixels_per_beat: f32) -> Self {
        let start_beats = match ghost.place {
            GhostPlace::Lane { beat, .. } | GhostPlace::NewTrack { beat } => beat,
            // A launcher Clip starts its own timeline.
            GhostPlace::NewTrackSlot { .. } => 0.0,
        };
        let (name, body) = match ghost.media {
            GhostMedia::Audio { clip, peaks, .. } => {
                (clip.name.clone(), GhostBody::Waveform(Arc::clone(peaks)))
            }
            GhostMedia::Midi { clip, .. } => (
                clip.name.clone(),
                GhostBody::Notes(note_clip_summary(clip, pixels_per_beat)),
            ),
        };
        Self {
            start_beats,
            length_beats: ghost.length_beats,
            name,
            body,
        }
    }

    /// Draws the ghost in `track`'s colours on a lane `w` by `h`.
    pub(super) fn draw(
        &self,
        frame: &mut canvas::Frame,
        geometry: TimelineGeometry,
        track: Color,
        w: f32,
        h: f32,
    ) {
        let clip_x = geometry.beat_to_x(self.start_beats);
        let clip_w = geometry.width_for_beats(self.length_beats);
        if clip_x + clip_w < 0.0 || clip_x > w {
            return;
        }
        let body = clip_box(clip_x, CLIP_Y, clip_w, h - 2.0 * CLIP_Y);
        let colors = clip_colors(track, false);
        draw_drag_shadow(frame, body);
        draw_clip_back(frame, body, &colors, None, w);
        match &self.body {
            GhostBody::Waveform(peaks) => draw_waveform(
                frame,
                peaks,
                // The peaks are fitted to the whole Clip, so they stretch to
                // its width whatever a pixel spans.
                PeakScale {
                    frames_per_pixel: 0.0,
                    peak_span_frames: None,
                },
                body,
                clip_w,
                w,
                colors.ink,
            ),
            GhostBody::Notes(summary) => draw_notes(
                frame,
                summary,
                body,
                w,
                geometry.pixels_per_beat(),
                colors.ink,
            ),
        }
        draw_clip_front(frame, body, &colors, &self.name, false, w);
    }
}

/// The lane of a track a drop would add, showing the ghost in it.
pub struct NewTrackLane {
    pub ghost: LaneGhost,
    pub geometry: TimelineGeometry,
}

impl canvas::Program<Message> for NewTrackLane {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        frame.fill_rectangle(iced::Point::ORIGIN, bounds.size(), crate::theme::track_bg());
        self.ghost.draw(
            &mut frame,
            self.geometry,
            crate::theme::accent(),
            bounds.width,
            bounds.height,
        );
        vec![frame.into_geometry()]
    }
}
