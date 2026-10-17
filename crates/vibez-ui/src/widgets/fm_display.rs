//! The FM Synth's plots: the algorithm as operators and the arrows between
//! them, where a click picks the operator the card shows, and two cycles of
//! the sound at its brightest.

use iced::alignment::{Horizontal, Vertical};
use iced::mouse;
use iced::widget::canvas;
use iced::{Color, Point, Rectangle, Renderer, Size, Theme, Vector};
use vibez_core::id::TrackId;
use vibez_instruments::fm::{ALGORITHMS, OPERATORS};

use crate::domains::fm::OPERATOR_NAMES;
use crate::domains::instrument_card::InstrumentCardMsg;
use crate::message::Message;
use crate::theme as th;
use crate::widgets::device_controls::{LABEL_SIZE, VALUE_SIZE};

/// Where each operator sits, A to D, in columns and rows, index-aligned
/// with the algorithms: chains run left to right into A, and stacks down
/// into the carriers along the bottom.
const PLACES: [[(f32, f32); OPERATORS]; ALGORITHMS.len()] = [
    [(3.0, 0.0), (2.0, 0.0), (1.0, 0.0), (0.0, 0.0)],
    [(2.0, 0.5), (1.0, 0.5), (0.0, 0.0), (0.0, 1.0)],
    [(2.0, 0.5), (1.0, 0.0), (0.0, 0.0), (1.0, 1.0)],
    [(2.0, 0.5), (1.0, 1.0), (1.0, 0.0), (0.0, 0.0)],
    [(1.0, 0.0), (0.0, 0.0), (1.0, 1.0), (0.0, 1.0)],
    [(0.0, 1.0), (1.0, 1.0), (2.0, 1.0), (1.0, 0.0)],
    [(0.0, 1.0), (1.0, 1.0), (2.0, 1.0), (2.0, 0.0)],
    [(0.0, 0.0), (1.0, 0.0), (2.0, 0.0), (3.0, 0.0)],
];
const MAX_RADIUS: f32 = 22.0;
/// Room below the nodes for a feedback loop and its name.
const FEEDBACK_H: f32 = 16.0;

pub struct AlgorithmPlot {
    pub track_id: TrackId,
    pub algorithm: usize,
    pub selected: usize,
    /// The operators feeding back into themselves.
    pub feedback: [bool; OPERATORS],
    pub accent: Color,
}

impl AlgorithmPlot {
    /// Each operator's centre and the nodes' radius in `size`.
    fn nodes(&self, size: Size) -> ([Point; OPERATORS], f32) {
        let places = PLACES[self.algorithm.min(PLACES.len() - 1)];
        let columns = places.iter().fold(0.0f32, |most, (x, _)| most.max(*x)) + 1.0;
        let rows = places.iter().fold(0.0f32, |most, (_, y)| most.max(*y)) + 1.0;
        let feedback = if self.feedback.iter().any(|on| *on) {
            FEEDBACK_H
        } else {
            0.0
        };
        let area = Rectangle::new(
            Point::new(8.0, 6.0),
            Size::new(
                (size.width - 16.0).max(1.0),
                (size.height - 12.0 - feedback).max(1.0),
            ),
        );
        let cell = Size::new(area.width / columns, area.height / rows);
        let radius = (cell.width.min(cell.height) * 0.36).clamp(6.0, MAX_RADIUS);
        // Nodes keep to the middle when the cells are far wider than tall.
        let span = (cell.height * 2.2).min(cell.width);
        let left = area.x + (area.width - span * columns) / 2.0;
        let centres = places
            .map(|(x, y)| Point::new(left + span * (x + 0.5), area.y + cell.height * (y + 0.5)));
        (centres, radius)
    }
}

impl canvas::Program<Message> for AlgorithmPlot {
    type State = ();

    fn update(
        &self,
        _state: &mut (),
        event: canvas::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> (canvas::event::Status, Option<Message>) {
        let canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) = event else {
            return (canvas::event::Status::Ignored, None);
        };
        let Some(at) = cursor.position_in(bounds) else {
            return (canvas::event::Status::Ignored, None);
        };
        let (centres, radius) = self.nodes(bounds.size());
        match centres
            .iter()
            .position(|centre| centre.distance(at) <= radius + 3.0)
        {
            Some(operator) => (
                canvas::event::Status::Captured,
                Some(Message::InstrumentCard(InstrumentCardMsg::ShowOscillator(
                    self.track_id,
                    operator,
                ))),
            ),
            None => (canvas::event::Status::Ignored, None),
        }
    }

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
        let algorithm = &ALGORITHMS[self.algorithm.min(ALGORITHMS.len() - 1)];
        let (centres, radius) = self.nodes(bounds.size());
        let arrow = canvas::Stroke::default()
            .with_color(th::with_alpha(self.accent, 0.85))
            .with_width(1.5);
        for (target, modulators) in algorithm.modulators.iter().enumerate() {
            for source in 0..OPERATORS {
                if modulators & (1 << source) == 0 {
                    continue;
                }
                let (from, to) = (centres[source], centres[target]);
                let length = from.distance(to).max(1.0);
                let along = Vector::new((to.x - from.x) / length, (to.y - from.y) / length);
                let start = from + along * radius;
                let end = to - along * (radius + 1.0);
                frame.stroke(&canvas::Path::line(start, end), arrow);
                let back = end - along * 6.0;
                let side = Vector::new(-along.y, along.x) * 3.5;
                frame.fill(
                    &canvas::Path::new(|path| {
                        path.move_to(end);
                        path.line_to(back + side);
                        path.line_to(back - side);
                        path.close();
                    }),
                    th::with_alpha(self.accent, 0.85),
                );
            }
        }
        for (operator, centre) in centres.iter().enumerate() {
            let selected = operator == self.selected;
            let ring = if selected { self.accent } else { th::border() };
            let circle = canvas::Path::circle(*centre, radius);
            frame.fill(
                &circle,
                if selected {
                    th::with_alpha(self.accent, 0.22)
                } else {
                    th::bg_dark()
                },
            );
            frame.stroke(
                &circle,
                canvas::Stroke::default()
                    .with_color(ring)
                    .with_width(if selected { 2.5 } else { 1.5 }),
            );
            frame.fill_text(canvas::Text {
                content: OPERATOR_NAMES[operator].into(),
                position: *centre,
                color: th::text(),
                size: VALUE_SIZE.into(),
                horizontal_alignment: Horizontal::Center,
                vertical_alignment: Vertical::Center,
                ..canvas::Text::default()
            });
            if self.feedback[operator] {
                let top = Point::new(centre.x, centre.y + radius);
                let corner = Point::new(centre.x, centre.y + radius + FEEDBACK_H * 0.6);
                let tip = Point::new(centre.x + 9.0, corner.y);
                frame.stroke(
                    &canvas::Path::new(|path| {
                        path.move_to(top);
                        path.line_to(corner);
                        path.line_to(tip);
                    }),
                    arrow,
                );
                frame.fill_text(canvas::Text {
                    content: "Feedback".into(),
                    position: Point::new(tip.x + 3.0, tip.y),
                    color: self.accent,
                    size: LABEL_SIZE.into(),
                    vertical_alignment: Vertical::Center,
                    ..canvas::Text::default()
                });
            }
        }
        vec![frame.into_geometry()]
    }
}

/// Two cycles of the sound at its brightest, as `vibez_instruments::fm`
/// previews it.
pub struct ScopePlot {
    /// Every control's value.
    pub params: Vec<f32>,
    pub accent: Color,
}

impl canvas::Program<Message> for ScopePlot {
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
        let plot = Rectangle::new(
            Point::new(6.0, 6.0),
            Size::new(
                (bounds.width - 12.0).max(1.0),
                (bounds.height - 12.0).max(1.0),
            ),
        );
        let middle = plot.y + plot.height / 2.0;
        let grid = th::with_alpha(th::border(), 0.3);
        for step in 1..4 {
            let x = plot.x + plot.width * step as f32 / 4.0;
            frame.stroke(
                &canvas::Path::line(Point::new(x, plot.y), Point::new(x, plot.y + plot.height)),
                canvas::Stroke::default().with_color(grid).with_width(1.0),
            );
        }
        frame.stroke(
            &canvas::Path::line(
                Point::new(plot.x, middle),
                Point::new(plot.x + plot.width, middle),
            ),
            canvas::Stroke::default().with_color(grid).with_width(1.0),
        );
        let samples = vibez_instruments::fm::preview(&self.params, plot.width.max(2.0) as usize);
        let peak = samples
            .iter()
            .fold(0.0f32, |peak, sample| peak.max(sample.abs()));
        if peak > 1e-6 {
            let scale = plot.height * 0.45 / peak;
            let step = plot.width / (samples.len().max(2) - 1) as f32;
            let wave = canvas::Path::new(|path| {
                for (n, sample) in samples.iter().enumerate() {
                    let at = Point::new(plot.x + step * n as f32, middle - sample * scale);
                    if n == 0 {
                        path.move_to(at);
                    } else {
                        path.line_to(at);
                    }
                }
            });
            frame.stroke(
                &wave,
                canvas::Stroke::default()
                    .with_color(th::with_alpha(self.accent, 0.22))
                    .with_width(5.0),
            );
            frame.stroke(
                &wave,
                canvas::Stroke::default()
                    .with_color(self.accent)
                    .with_width(1.8),
            );
        }
        vec![frame.into_geometry()]
    }
}
