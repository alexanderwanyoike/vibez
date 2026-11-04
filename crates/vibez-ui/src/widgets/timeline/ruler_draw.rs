//! How the beat ruler paints its strips, marks and markers.

use iced::widget::canvas;
use iced::{Point, Renderer, Size};

use super::ruler::{RulerWidget, LOOP_STRIP_HEIGHT};
use super::ruler_scale::{ruler_marks, RulerMark};
use crate::theme;
use crate::timeline_geometry::TimelineGeometry;
use crate::typography;

const MID_TICK_HEIGHT: f32 = 7.0;
const MINOR_TICK_HEIGHT: f32 = 4.0;

/// The brace's inset from the top and bottom of the loop strip.
const BRACE_INSET: f32 = 3.0;
const BRACE_RADIUS: f32 = 2.0;

impl RulerWidget {
    pub(super) fn paint(&self, renderer: &Renderer, size: Size) -> canvas::Geometry {
        let mut frame = canvas::Frame::new(renderer, size);
        let width = size.width;
        let top = self.numbers_top();
        frame.fill_rectangle(Point::ORIGIN, size, theme::ruler_bg());
        if self.loop_brace {
            self.paint_loop_strip(&mut frame, width);
        } else {
            self.paint_section_loop(&mut frame, size);
        }
        draw_marks(&mut frame, self.geometry(), width, top, size.height);
        self.paint_time_selection(&mut frame, width, top, size.height);
        self.paint_play_start(&mut frame, width, top);
        self.paint_playhead(&mut frame, width, top, size.height);
        frame.into_geometry()
    }

    fn paint_loop_strip(&self, frame: &mut canvas::Frame, width: f32) {
        frame.fill_rectangle(
            Point::ORIGIN,
            Size::new(width, LOOP_STRIP_HEIGHT),
            theme::ruler_loop_strip(),
        );
        if self.loop_end_beats <= self.loop_start_beats {
            return;
        }
        let geometry = self.geometry();
        let start = geometry.beat_to_x(self.loop_start_beats).round();
        let end = geometry.beat_to_x(self.loop_end_beats).round();
        if end < 0.0 || start > width {
            return;
        }
        let color = if self.loop_enabled {
            theme::accent()
        } else {
            theme::ruler_brace_off()
        };
        frame.fill(
            &canvas::Path::rounded_rectangle(
                Point::new(start, BRACE_INSET),
                Size::new(
                    (end - start).max(2.0),
                    LOOP_STRIP_HEIGHT - 2.0 * BRACE_INSET,
                ),
                BRACE_RADIUS.into(),
            ),
            color,
        );
    }

    fn paint_section_loop(&self, frame: &mut canvas::Frame, size: Size) {
        if !self.loop_enabled || self.loop_end_beats <= self.loop_start_beats {
            return;
        }
        let geometry = self.geometry();
        let start = geometry.beat_to_x(self.loop_start_beats).max(0.0);
        let end = geometry.beat_to_x(self.loop_end_beats).min(size.width);
        if end > start {
            frame.fill_rectangle(
                Point::new(start, 0.0),
                Size::new(end - start, size.height),
                theme::with_alpha(theme::accent(), 0.15),
            );
        }
    }

    fn paint_time_selection(&self, frame: &mut canvas::Frame, width: f32, top: f32, bottom: f32) {
        if !self.time_selection_active || self.selection_end_beats <= self.selection_start_beats {
            return;
        }
        let geometry = self.geometry();
        let start = geometry.beat_to_x(self.selection_start_beats).round();
        let end = geometry.beat_to_x(self.selection_end_beats).round();
        let fill_start = start.max(0.0);
        let fill_end = end.min(width);
        if fill_end > fill_start {
            frame.fill_rectangle(
                Point::new(fill_start, top),
                Size::new(fill_end - fill_start, bottom - top),
                theme::with_alpha(theme::accent(), 0.10),
            );
        }
        for x in [start, end] {
            if (0.0..=width).contains(&x) {
                frame.fill_rectangle(
                    Point::new(x, top),
                    Size::new(1.0, bottom - top),
                    theme::accent(),
                );
            }
        }
    }

    fn paint_play_start(&self, frame: &mut canvas::Frame, width: f32, top: f32) {
        let Some(play_start) = self.play_start_beats else {
            return;
        };
        let x = self.geometry().beat_to_x(play_start);
        if (0.0..=width).contains(&x) {
            let marker = canvas::Path::new(|path| {
                path.move_to(Point::new(x - 5.0, top));
                path.line_to(Point::new(x + 5.0, top));
                path.line_to(Point::new(x, top + 7.0));
                path.close();
            });
            frame.fill(&marker, theme::playhead());
        }
    }

    fn paint_playhead(&self, frame: &mut canvas::Frame, width: f32, top: f32, bottom: f32) {
        let x = self.geometry().beat_to_x(self.playhead_beats);
        if (0.0..=width).contains(&x) {
            frame.stroke(
                &canvas::Path::line(Point::new(x, top), Point::new(x, bottom)),
                canvas::Stroke::default()
                    .with_color(theme::playhead())
                    .with_width(2.0),
            );
        }
    }
}

/// The number strip's labels and ticks between `top` and `bottom`.
fn draw_marks(
    frame: &mut canvas::Frame,
    geometry: TimelineGeometry,
    width: f32,
    top: f32,
    bottom: f32,
) {
    let tick = theme::ruler_tick();
    for mark in ruler_marks(geometry, width) {
        match mark {
            RulerMark::Label { x, text, bar } => {
                frame.fill_rectangle(
                    iced::Point::new(x, top),
                    iced::Size::new(1.0, bottom - top),
                    tick,
                );
                let (font, size, color) = if bar {
                    (typography::PERFORM_LABEL, 11.0, theme::text())
                } else {
                    (typography::PERFORM_DISPLAY, 10.5, theme::text_dim())
                };
                frame.fill_text(canvas::Text {
                    content: text,
                    position: iced::Point::new(x + 4.0, top + 4.0),
                    color,
                    size: iced::Pixels(size),
                    font,
                    ..Default::default()
                });
            }
            RulerMark::Mid { x } => frame.fill_rectangle(
                iced::Point::new(x, bottom - MID_TICK_HEIGHT),
                iced::Size::new(1.0, MID_TICK_HEIGHT),
                tick,
            ),
            RulerMark::Minor { x } => frame.fill_rectangle(
                iced::Point::new(x, bottom - MINOR_TICK_HEIGHT),
                iced::Size::new(1.0, MINOR_TICK_HEIGHT),
                tick,
            ),
        }
    }
    frame.fill_rectangle(
        iced::Point::new(0.0, bottom - 1.0),
        iced::Size::new(width, 1.0),
        theme::border(),
    );
}
