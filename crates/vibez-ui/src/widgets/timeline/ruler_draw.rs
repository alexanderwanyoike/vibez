//! How the beat ruler paints its strips, marks and markers.

use iced::widget::canvas;
use iced::{Renderer, Size};

use super::ruler::RulerWidget;
use super::ruler_scale::{ruler_marks, RulerMark};
use crate::theme;
use crate::timeline_geometry::TimelineGeometry;
use crate::typography;

const MID_TICK_HEIGHT: f32 = 7.0;
const MINOR_TICK_HEIGHT: f32 = 4.0;

impl RulerWidget {
    pub(super) fn paint(&self, renderer: &Renderer, size: Size) -> canvas::Geometry {
        let mut frame = canvas::Frame::new(renderer, size);
        let w = size.width;
        let h = size.height;

        // Background
        frame.fill_rectangle(
            iced::Point::ORIGIN,
            iced::Size::new(w, h),
            theme::ruler_bg(),
        );

        draw_marks(&mut frame, self.geometry(), w, 0.0, h);

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
                    theme::with_alpha(theme::accent(), 0.15),
                );

                // REPEAT icon centered in the loop region
                let center_x = fill_x + fill_w / 2.0;
                frame.fill_text(canvas::Text {
                    content: crate::icons::REPEAT.to_string(),
                    position: iced::Point::new(center_x - 6.0, (h - 12.0) / 2.0),
                    color: theme::with_alpha(theme::accent(), 0.7),
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
                        .with_color(theme::accent())
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
                        .with_color(theme::accent())
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
                    theme::with_alpha(theme::accent(), 0.10),
                );
            }

            // Thinner bracket lines
            if sel_x1 >= 0.0 && sel_x1 <= w {
                let bracket =
                    canvas::Path::line(iced::Point::new(sel_x1, 0.0), iced::Point::new(sel_x1, h));
                frame.stroke(
                    &bracket,
                    canvas::Stroke::default()
                        .with_color(theme::accent())
                        .with_width(1.0),
                );
            }
            if sel_x2 >= 0.0 && sel_x2 <= w {
                let bracket =
                    canvas::Path::line(iced::Point::new(sel_x2, 0.0), iced::Point::new(sel_x2, h));
                frame.stroke(
                    &bracket,
                    canvas::Stroke::default()
                        .with_color(theme::accent())
                        .with_width(1.0),
                );
            }
        }

        if let Some(play_start) = self.play_start_beats {
            let x = self.beat_to_x(play_start, w);
            if (0.0..=w).contains(&x) {
                let marker = canvas::Path::new(|path| {
                    path.move_to(iced::Point::new(x - 5.0, 0.0));
                    path.line_to(iced::Point::new(x + 5.0, 0.0));
                    path.line_to(iced::Point::new(x, 7.0));
                    path.close();
                });
                frame.fill(&marker, theme::playhead());
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
                    .with_color(theme::playhead())
                    .with_width(2.0),
            );
        }

        frame.into_geometry()
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
