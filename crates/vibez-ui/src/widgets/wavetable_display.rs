//! The Wavetable Synth's table plot, as the concept draws it: every frame a
//! thin wave across the full width, stacked down the plot from the first
//! to the last, the frame Position plays drawn bold where it sits in the
//! stack as Warp bends it, and Position marked across with a dot where it
//! meets that wave.

use iced::alignment::Horizontal;
use iced::mouse;
use iced::widget::canvas;
use iced::{Color, Point, Rectangle, Renderer, Size, Theme};
use vibez_instruments::wavetable::Warp;
use vibez_instruments::wavetables::{tables, FRAMES};

use crate::message::Message;
use crate::theme as th;
use crate::widgets::device_controls::LABEL_SIZE;

const LABEL_H: f32 = 14.0;
/// Each wave swings this share of the plot's height either way of its
/// place in the stack.
const SWING: f32 = 0.16;
/// The thinnest the stack lines get apart before some are left out.
const LINE_SPACING: f32 = 3.0;

pub struct WavetablePlot {
    pub table: usize,
    pub position: f32,
    pub warp: f32,
    pub accent: Color,
}

impl canvas::Program<Message> for WavetablePlot {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        frame.fill(
            &canvas::Path::rounded_rectangle(Point::ORIGIN, bounds.size(), 3.0.into()),
            th::bg_dark(),
        );
        let Some(table) = tables().get(self.table) else {
            return vec![frame.into_geometry()];
        };
        let labels = bounds.height >= 80.0;
        let bottom = if labels { LABEL_H } else { 4.0 };
        let plot = Rectangle::new(
            Point::new(6.0, 6.0),
            Size::new(
                (bounds.width - 12.0).max(1.0),
                (bounds.height - bottom - 10.0).max(1.0),
            ),
        );
        let grid = th::with_alpha(th::border(), 0.3);
        for step in 1..4 {
            let x = plot.x + plot.width * step as f32 / 4.0;
            let y = plot.y + plot.height * step as f32 / 4.0;
            frame.stroke(
                &canvas::Path::line(Point::new(x, plot.y), Point::new(x, plot.y + plot.height)),
                canvas::Stroke::default().with_color(grid).with_width(1.0),
            );
            frame.stroke(
                &canvas::Path::line(Point::new(plot.x, y), Point::new(plot.x + plot.width, y)),
                canvas::Stroke::default().with_color(grid).with_width(1.0),
            );
        }
        // The stack runs from the first frame at the top to the last at
        // the bottom, inset so the outer waves keep their swing inside.
        let swing = plot.height * SWING;
        let top = plot.y + swing;
        let span = (plot.height - 2.0 * swing).max(1.0);
        let place = |through: f32| top + span * through;
        let columns = (plot.width as usize / 2).max(16);
        let warp = Warp::new(self.warp);
        let wave = |through: f32, read: &dyn Fn(f32) -> f32| {
            canvas::Path::new(|builder| {
                for column in 0..=columns {
                    let phase = column as f32 / columns as f32;
                    let at = Point::new(
                        plot.x + plot.width * phase,
                        place(through) - swing * read(warp.apply(phase).min(0.9999)),
                    );
                    if column == 0 {
                        builder.move_to(at);
                    } else {
                        builder.line_to(at);
                    }
                }
            })
        };
        let shown = ((span / LINE_SPACING) as usize).clamp(2, FRAMES);
        for line in 0..shown {
            let through = line as f32 / (shown - 1) as f32;
            let samples = table.frame((through * (FRAMES - 1) as f32).round() as usize);
            let read = |phase: f32| samples[(phase * samples.len() as f32) as usize];
            frame.stroke(
                &wave(through, &read),
                canvas::Stroke::default()
                    .with_color(th::with_alpha(self.accent, 0.28))
                    .with_width(1.0),
            );
        }
        let position = self.position.clamp(0.0, 1.0);
        let read = |phase: f32| table.read(position, phase, 0);
        frame.stroke(
            &wave(position, &read),
            canvas::Stroke::default()
                .with_color(self.accent)
                .with_width(2.5),
        );
        let across = plot.x + plot.width * position;
        frame.stroke(
            &canvas::Path::line(
                Point::new(across, plot.y),
                Point::new(across, plot.y + plot.height),
            ),
            canvas::Stroke::default()
                .with_color(self.accent)
                .with_width(1.5),
        );
        let meeting = place(position) - swing * read(warp.apply(position).min(0.9999));
        frame.fill(
            &canvas::Path::circle(Point::new(across, meeting), 4.5),
            self.accent,
        );
        if labels {
            let y = plot.y + plot.height + LABEL_H / 2.0 + 2.0;
            for (text, x, align) in [
                ("0%", plot.x, Horizontal::Left),
                ("Position", plot.x + plot.width / 2.0, Horizontal::Center),
                ("100%", plot.x + plot.width, Horizontal::Right),
            ] {
                frame.fill_text(canvas::Text {
                    content: text.into(),
                    position: Point::new(x, y),
                    color: th::text_muted(),
                    size: LABEL_SIZE.into(),
                    horizontal_alignment: align,
                    vertical_alignment: iced::alignment::Vertical::Center,
                    ..canvas::Text::default()
                });
            }
        }
        vec![frame.into_geometry()]
    }
}
