//! The Saturator's plots: output level against input level with Custom's
//! three nodes to drag, and the harmonics a sine comes out with.

use iced::mouse;
use iced::widget::canvas;
use iced::{Color, Point, Rectangle, Renderer, Size, Theme};
use vibez_core::id::{EffectId, TrackId};
use vibez_dsp::saturator::{harmonics_db, param, Transfer, CUSTOM_INPUT_DB, SATURATOR_PARAMS};

use crate::domains::saturator::{drag_node, points_of, NODES};
use crate::message::Message;
use crate::state::UndoGestureId;
use crate::theme as th;
use crate::widgets::device_controls::LABEL_SIZE;

const RANGE_DB: f32 = 24.0;
const LABEL_W: f32 = 26.0;
const LABEL_H: f32 = 14.0;
const NODE_R: f32 = 4.0;
/// The spectrum's sine, and how many of its harmonics it draws.
const FUNDAMENTAL_HZ: f32 = 100.0;
const HARMONICS: usize = 64;
const SPECTRUM_FLOOR_DB: f32 = -96.0;
const LOW_HZ: f32 = 20.0;
const HIGH_HZ: f32 = 20_000.0;

fn grid() -> Color {
    th::with_alpha(th::border(), 0.35)
}

use iced::alignment::Horizontal;

fn label(frame: &mut canvas::Frame, text: String, at: Point, align: Horizontal) {
    frame.fill_text(canvas::Text {
        content: text,
        position: at,
        color: th::text_muted(),
        size: LABEL_SIZE.into(),
        horizontal_alignment: align,
        vertical_alignment: iced::alignment::Vertical::Center,
        ..canvas::Text::default()
    });
}

/// Where the `step`th of `count` names along the bottom axis goes: the end
/// ones run inward from the corners, clear of the level names.
fn along(x: f32, step: usize, count: usize) -> (f32, Horizontal) {
    if step == 0 {
        (x + 2.0, Horizontal::Left)
    } else if step + 1 == count {
        (x, Horizontal::Right)
    } else {
        (x, Horizontal::Center)
    }
}

fn panel(frame: &mut canvas::Frame, size: Size) {
    frame.fill(
        &canvas::Path::rounded_rectangle(Point::ORIGIN, size, 3.0.into()),
        th::bg_dark(),
    );
}

/// The area inside the axis labels, and whether there is room for them.
fn plot_area(size: Size) -> (Rectangle, bool) {
    let labels = size.height >= 60.0;
    let left = if labels { LABEL_W } else { 4.0 };
    let bottom = if labels { LABEL_H } else { 4.0 };
    (
        Rectangle::new(
            Point::new(left, 6.0),
            Size::new(
                (size.width - left - 8.0).max(1.0),
                (size.height - bottom - 8.0).max(1.0),
            ),
        ),
        labels,
    )
}

fn to_db(amplitude: f32) -> f32 {
    20.0 * amplitude.abs().max(1e-6).log10()
}

fn value(params: &[f32], index: usize) -> f32 {
    params
        .get(index)
        .copied()
        .unwrap_or(SATURATOR_PARAMS[index].default)
}

/// Output level against input level, both in dB.
pub struct TransferPlot<'a> {
    pub track_id: TrackId,
    pub effect_id: EffectId,
    pub params: &'a [f32],
    /// The measured input peak, linear; zero when nothing is playing.
    pub input_peak: f32,
    pub accent: Color,
}

#[derive(Debug, Default)]
pub struct TransferState {
    /// The node held, the cursor height it was taken at, and its point then.
    drag: Option<(usize, f32, f32)>,
    undo_gesture: Option<UndoGestureId>,
    hover: Option<usize>,
}

impl TransferPlot<'_> {
    fn x(plot: Rectangle, db: f32) -> f32 {
        plot.x + (db.clamp(-RANGE_DB, RANGE_DB) + RANGE_DB) / (2.0 * RANGE_DB) * plot.width
    }

    fn y(plot: Rectangle, db: f32) -> f32 {
        plot.y + (RANGE_DB - db.clamp(-RANGE_DB, RANGE_DB)) / (2.0 * RANGE_DB) * plot.height
    }

    /// Where each node sits: at the input level that reaches its point
    /// once driven, on the curve as heard.
    fn nodes(&self, plot: Rectangle) -> Vec<(usize, Point)> {
        let transfer = Transfer::new(self.params);
        let drive = value(self.params, param::DRIVE_DB);
        (0..NODES.len())
            .filter_map(|node| {
                let input = CUSTOM_INPUT_DB[node] - drive;
                (input >= -RANGE_DB).then(|| {
                    let output = to_db(transfer.at(10f32.powf(input / 20.0)));
                    (
                        node,
                        Point::new(Self::x(plot, input), Self::y(plot, output)),
                    )
                })
            })
            .collect()
    }

    fn node_at(&self, plot: Rectangle, at: Point) -> Option<usize> {
        self.nodes(plot)
            .into_iter()
            .map(|(node, point)| (node, point.distance(at)))
            .filter(|(_, distance)| *distance <= NODE_R + 4.0)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(node, _)| node)
    }
}

impl canvas::Program<Message> for TransferPlot<'_> {
    type State = TransferState;

    fn update(
        &self,
        state: &mut TransferState,
        event: canvas::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> (canvas::event::Status, Option<Message>) {
        let (plot, _) = plot_area(bounds.size());
        let local = cursor.position_in(bounds);
        match event {
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                if let Some(node) = local.and_then(|at| self.node_at(plot, at)) {
                    let at = local.map_or(0.0, |at| at.y);
                    state.drag = Some((node, at, points_of(self.params)[node]));
                    state.undo_gesture = Some(UndoGestureId::new());
                    return (canvas::event::Status::Captured, None);
                }
            }
            canvas::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                if state.drag.take().is_some() {
                    state.undo_gesture = None;
                    return (canvas::event::Status::Captured, None);
                }
            }
            canvas::Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                if let (Some((node, from, start)), Some(at)) = (state.drag, cursor.position()) {
                    let per_db = plot.height / (2.0 * RANGE_DB);
                    let db = start + (from - (at.y - bounds.y)) / per_db;
                    let edits = drag_node(self.params, node, db);
                    let gesture = *state.undo_gesture.get_or_insert_with(UndoGestureId::new);
                    return (
                        canvas::event::Status::Captured,
                        Some(
                            Message::set_effect_params(self.track_id, self.effect_id, edits)
                                .in_undo_gesture(gesture),
                        ),
                    );
                }
                state.hover = local.and_then(|at| self.node_at(plot, at));
            }
            _ => {}
        }
        (canvas::event::Status::Ignored, None)
    }

    fn mouse_interaction(
        &self,
        state: &TransferState,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if state.drag.is_some() || state.hover.is_some() {
            mouse::Interaction::ResizingVertically
        } else {
            mouse::Interaction::default()
        }
    }

    fn draw(
        &self,
        state: &TransferState,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        panel(&mut frame, bounds.size());
        let (plot, labels) = plot_area(bounds.size());
        let (x, y) = (|db| Self::x(plot, db), |db| Self::y(plot, db));
        // A short plot names every other level, so the names keep apart.
        let every = if plot.height >= 100.0 { 1 } else { 2 };
        for (step, db) in [-24.0f32, -12.0, 0.0, 12.0, 24.0].into_iter().enumerate() {
            for line in [
                canvas::Path::line(
                    Point::new(plot.x, y(db)),
                    Point::new(plot.x + plot.width, y(db)),
                ),
                canvas::Path::line(
                    Point::new(x(db), plot.y),
                    Point::new(x(db), plot.y + plot.height),
                ),
            ] {
                frame.stroke(
                    &line,
                    canvas::Stroke::default().with_color(grid()).with_width(1.0),
                );
            }
            if labels && step % every == 0 {
                label(
                    &mut frame,
                    format!("{db:.0}"),
                    Point::new(LABEL_W - 4.0, y(db)),
                    Horizontal::Right,
                );
            }
            // A narrow plot names every other input level too.
            if labels && step % if plot.width >= 200.0 { 1 } else { 2 } == 0 {
                let (at, align) = along(x(db), step, 5);
                label(
                    &mut frame,
                    format!("{db:.0}"),
                    Point::new(at, plot.y + plot.height + LABEL_H / 2.0 + 1.0),
                    align,
                );
            }
        }
        frame.stroke(
            &canvas::Path::line(
                Point::new(x(-RANGE_DB), y(-RANGE_DB)),
                Point::new(x(RANGE_DB), y(RANGE_DB)),
            ),
            canvas::Stroke::default()
                .with_color(th::text_muted())
                .with_width(1.0),
        );
        let transfer = Transfer::new(self.params);
        let output_db = |input_db: f32| to_db(transfer.at(10f32.powf(input_db / 20.0)));
        let columns = (plot.width / 2.0).max(16.0) as usize;
        let curve = canvas::Path::new(|builder| {
            for column in 0..=columns {
                let input_db = -RANGE_DB + 2.0 * RANGE_DB * column as f32 / columns as f32;
                let point = Point::new(x(input_db), y(output_db(input_db)));
                if column == 0 {
                    builder.move_to(point);
                } else {
                    builder.line_to(point);
                }
            }
        });
        frame.stroke(
            &curve,
            canvas::Stroke::default()
                .with_color(self.accent)
                .with_width(2.0),
        );
        for (node, point) in self.nodes(plot) {
            let held = state.drag.is_some_and(|(held, ..)| held == node);
            let radius = if held || state.hover == Some(node) {
                NODE_R + 1.5
            } else {
                NODE_R
            };
            frame.fill(&canvas::Path::circle(point, radius), self.accent);
        }
        if self.input_peak > 0.0 {
            let input_db = to_db(self.input_peak);
            if input_db > -RANGE_DB {
                frame.stroke(
                    &canvas::Path::circle(Point::new(x(input_db), y(output_db(input_db))), 3.0),
                    canvas::Stroke::default()
                        .with_color(th::text())
                        .with_width(1.5),
                );
            }
        }
        vec![frame.into_geometry()]
    }
}

/// The harmonics a sine at -6 dBFS comes out with, against the sine alone.
pub struct SpectrumPlot<'a> {
    pub params: &'a [f32],
    pub accent: Color,
}

impl canvas::Program<Message> for SpectrumPlot<'_> {
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
        panel(&mut frame, bounds.size());
        let (plot, labels) = plot_area(bounds.size());
        let span = (HIGH_HZ / LOW_HZ).ln();
        let x = |hz: f32| plot.x + plot.width * ((hz / LOW_HZ).ln() / span).clamp(0.0, 1.0);
        let y =
            |db: f32| plot.y + plot.height * (db.clamp(SPECTRUM_FLOOR_DB, 0.0) / SPECTRUM_FLOOR_DB);
        let every = if plot.height >= 100.0 { 1 } else { 2 };
        for (step, db) in [0.0f32, -24.0, -48.0, -72.0, -96.0].into_iter().enumerate() {
            frame.stroke(
                &canvas::Path::line(
                    Point::new(plot.x, y(db)),
                    Point::new(plot.x + plot.width, y(db)),
                ),
                canvas::Stroke::default().with_color(grid()).with_width(1.0),
            );
            if labels && step % every == 0 {
                label(
                    &mut frame,
                    format!("{db:.0}"),
                    Point::new(LABEL_W - 4.0, y(db)),
                    Horizontal::Right,
                );
            }
        }
        for (step, (hz, name)) in [
            (20.0, "20"),
            (100.0, "100"),
            (1_000.0, "1k"),
            (10_000.0, "10k"),
            (20_000.0, "20k"),
        ]
        .into_iter()
        .enumerate()
        {
            frame.stroke(
                &canvas::Path::line(
                    Point::new(x(hz), plot.y),
                    Point::new(x(hz), plot.y + plot.height),
                ),
                canvas::Stroke::default().with_color(grid()).with_width(1.0),
            );
            // 10k crowds 20k on a narrow plot.
            if labels && (step != 3 || plot.width >= 360.0) {
                let (at, align) = along(x(hz), step, 5);
                label(
                    &mut frame,
                    name.into(),
                    Point::new(at, plot.y + plot.height + LABEL_H / 2.0 + 1.0),
                    align,
                );
            }
        }
        let bottom = plot.y + plot.height;
        let peak = |frame: &mut canvas::Frame, hz: f32, db: f32, color: Color, width: f32| {
            let (at, top) = (x(hz), y(db));
            frame.fill(
                &canvas::Path::new(|builder| {
                    builder.move_to(Point::new(at - width, bottom));
                    builder.line_to(Point::new(at, top));
                    builder.line_to(Point::new(at + width, bottom));
                    builder.close();
                }),
                color,
            );
        };
        for (harmonic, db) in harmonics_db(self.params, HARMONICS).into_iter().enumerate() {
            let hz = FUNDAMENTAL_HZ * (harmonic + 1) as f32;
            if db > SPECTRUM_FLOOR_DB && hz <= HIGH_HZ {
                peak(&mut frame, hz, db, th::with_alpha(self.accent, 0.85), 2.5);
            }
        }
        // The input is a lone line at the fundamental, drawn over the
        // processed peak so both read.
        frame.stroke(
            &canvas::Path::line(
                Point::new(x(FUNDAMENTAL_HZ), y(0.0)),
                Point::new(x(FUNDAMENTAL_HZ), bottom),
            ),
            canvas::Stroke::default()
                .with_color(th::text())
                .with_width(1.0),
        );
        vec![frame.into_geometry()]
    }
}
