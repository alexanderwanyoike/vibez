//! Draws the DSP's static transfer and measured wet gain reduction without invented signal.

use crate::{
    message::Message,
    state::{UiEffect, UndoGestureId},
    theme as th,
};
use iced::{mouse, widget::canvas, Color, Point, Rectangle, Renderer, Theme};
use vibez_core::id::TrackId;
use vibez_dsp::compressor::{param, static_transfer_db};

pub struct TransferCurve<'a> {
    pub track: TrackId,
    pub effect: &'a UiEffect,
    pub accent: Color,
}
#[derive(Default)]
pub struct TransferState {
    gesture: Option<UndoGestureId>,
}
fn plot(bounds: Rectangle) -> Rectangle {
    if bounds.height >= 64.0 {
        Rectangle {
            x: 25.0,
            y: 4.0,
            width: (bounds.width - 32.0).max(1.0),
            height: (bounds.height - 21.0).max(1.0),
        }
    } else {
        Rectangle {
            x: 3.0,
            y: 3.0,
            width: (bounds.width - 6.0).max(1.0),
            height: (bounds.height - 6.0).max(1.0),
        }
    }
}
fn backdrop(frame: &mut canvas::Frame, bounds: Rectangle) {
    frame.fill_rectangle(Point::ORIGIN, bounds.size(), th::bg_dark());
    frame.stroke(
        &canvas::Path::rectangle(Point::ORIGIN, bounds.size()),
        canvas::Stroke::default().with_color(th::border()),
    );
}
impl TransferCurve<'_> {
    fn x_value(position: f32, area: Rectangle) -> f32 {
        (-60.0 + (position - area.x) / area.width * 60.0).clamp(-60.0, 0.0)
    }
}
impl canvas::Program<Message> for TransferCurve<'_> {
    type State = TransferState;
    fn draw(
        &self,
        _: &TransferState,
        renderer: &Renderer,
        _: &Theme,
        bounds: Rectangle,
        _: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        backdrop(&mut frame, bounds);
        let area = plot(bounds);
        let x = |db: f32| area.x + (db + 60.0) / 60.0 * area.width;
        let y = |db: f32| area.y + (24.0 - db.clamp(-60.0, 24.0)) / 84.0 * area.height;
        for db in [-60.0, -48.0, -36.0, -24.0, -12.0, 0.0] {
            frame.stroke(
                &canvas::Path::line(
                    Point::new(x(db), area.y),
                    Point::new(x(db), area.y + area.height),
                ),
                canvas::Stroke::default().with_color(th::divider()),
            );
            if bounds.height >= 64.0 {
                frame.fill_text(canvas::Text {
                    content: format!("{db:.0}"),
                    position: Point::new(x(db), bounds.height - 1.0),
                    color: th::text_dim(),
                    size: 8.0.into(),
                    horizontal_alignment: iced::alignment::Horizontal::Center,
                    vertical_alignment: iced::alignment::Vertical::Bottom,
                    ..Default::default()
                });
            }
        }
        for db in [-48.0, -24.0, 0.0, 24.0] {
            frame.stroke(
                &canvas::Path::line(
                    Point::new(area.x, y(db)),
                    Point::new(area.x + area.width, y(db)),
                ),
                canvas::Stroke::default().with_color(th::divider()),
            );
            if bounds.height >= 64.0 {
                frame.fill_text(canvas::Text {
                    content: format!("{db:.0}"),
                    position: Point::new(area.x - 3.0, y(db)),
                    color: th::text_dim(),
                    size: 8.0.into(),
                    horizontal_alignment: iced::alignment::Horizontal::Right,
                    vertical_alignment: iced::alignment::Vertical::Center,
                    ..Default::default()
                });
            }
        }
        frame.stroke(
            &canvas::Path::line(Point::new(x(-60.0), y(-60.0)), Point::new(x(0.0), y(0.0))),
            canvas::Stroke::default().with_color(th::text_muted()),
        );
        let response = |db| {
            if self.effect.bypass {
                db
            } else {
                static_transfer_db(db, &self.effect.params)
            }
        };
        let curve = canvas::Path::new(|path| {
            for index in 0..160 {
                let input = -60.0 + index as f32 / 159.0 * 60.0;
                let point = Point::new(x(input), y(response(input)));
                if index == 0 {
                    path.move_to(point)
                } else {
                    path.line_to(point)
                }
            }
        });
        frame.stroke(
            &curve,
            canvas::Stroke::default()
                .with_color(self.accent)
                .with_width(2.0),
        );
        let threshold = self
            .effect
            .params
            .get(param::THRESHOLD)
            .copied()
            .unwrap_or(self.effect.descriptors[param::THRESHOLD].default);
        frame.fill(
            &canvas::Path::circle(Point::new(x(threshold), y(response(threshold))), 4.0),
            self.accent,
        );
        vec![frame.into_geometry()]
    }
    fn update(
        &self,
        state: &mut TransferState,
        event: canvas::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> (canvas::event::Status, Option<Message>) {
        match event {
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
                if cursor.position_in(bounds).is_some() =>
            {
                state.gesture = Some(UndoGestureId::new());
                return (canvas::event::Status::Captured, None);
            }
            canvas::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
                if state.gesture.take().is_some() =>
            {
                return (canvas::event::Status::Captured, None)
            }
            canvas::Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                if let (Some(gesture), Some(cursor)) = (state.gesture, cursor.position()) {
                    return (
                        canvas::event::Status::Captured,
                        Some(
                            Message::set_effect_param(
                                self.track,
                                self.effect.id,
                                param::THRESHOLD,
                                Self::x_value(cursor.x - bounds.x, plot(bounds)),
                            )
                            .in_undo_gesture(gesture),
                        ),
                    );
                }
            }
            _ => {}
        }
        (canvas::event::Status::Ignored, None)
    }
    fn mouse_interaction(
        &self,
        state: &TransferState,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if state.gesture.is_some() {
            mouse::Interaction::Grabbing
        } else if cursor.position_in(bounds).is_some() {
            mouse::Interaction::Grab
        } else {
            mouse::Interaction::default()
        }
    }
}

pub struct ReductionHistory {
    pub values: Vec<f32>,
    pub accent: Color,
}
impl canvas::Program<Message> for ReductionHistory {
    type State = ();
    fn draw(
        &self,
        _: &(),
        renderer: &Renderer,
        _: &Theme,
        bounds: Rectangle,
        _: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        backdrop(&mut frame, bounds);
        let area = plot(bounds);
        let range = self
            .values
            .iter()
            .copied()
            .fold(24.0, f32::max)
            .ceil()
            .min(120.0);
        let y = |db: f32| area.y + db.clamp(0.0, range) / range * area.height;
        for db in [0.0, range * 0.5, range] {
            frame.stroke(
                &canvas::Path::line(
                    Point::new(area.x, y(db)),
                    Point::new(area.x + area.width, y(db)),
                ),
                canvas::Stroke::default().with_color(th::divider()),
            );
            if bounds.height >= 64.0 {
                frame.fill_text(canvas::Text {
                    content: format!("-{db:.0}"),
                    position: Point::new(area.x - 3.0, y(db)),
                    color: th::text_dim(),
                    size: 8.0.into(),
                    horizontal_alignment: iced::alignment::Horizontal::Right,
                    vertical_alignment: if db == 0.0 {
                        iced::alignment::Vertical::Top
                    } else if db == range {
                        iced::alignment::Vertical::Bottom
                    } else {
                        iced::alignment::Vertical::Center
                    },
                    ..Default::default()
                });
            }
        }
        if self.values.is_empty() {
            frame.fill_text(canvas::Text {
                content: "Awaiting audio".into(),
                position: Point::new(bounds.width * 0.5, bounds.height * 0.5),
                color: th::text_dim(),
                size: 9.0.into(),
                horizontal_alignment: iced::alignment::Horizontal::Center,
                ..Default::default()
            });
        } else {
            let history = canvas::Path::new(|path| {
                for (index, &db) in self.values.iter().enumerate() {
                    let point = Point::new(
                        area.x + index as f32 / (self.values.len().max(2) - 1) as f32 * area.width,
                        y(db),
                    );
                    if index == 0 {
                        path.move_to(point)
                    } else {
                        path.line_to(point)
                    }
                }
            });
            frame.stroke(
                &history,
                canvas::Stroke::default()
                    .with_color(self.accent)
                    .with_width(1.5),
            );
        }
        vec![frame.into_geometry()]
    }
}

pub struct ReductionGauge {
    pub current: [f32; 2],
    pub peak: [f32; 2],
    pub accent: Color,
}
impl canvas::Program<Message> for ReductionGauge {
    type State = ();
    fn draw(
        &self,
        _: &(),
        renderer: &Renderer,
        _: &Theme,
        bounds: Rectangle,
        _: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let h = (bounds.height - 2.0).max(1.0);
        for channel in 0..2 {
            let x = channel as f32 * 9.0;
            frame.fill_rectangle(Point::new(x, 1.0), iced::Size::new(6.0, h), th::bg_dark());
            frame.fill_rectangle(
                Point::new(x, 1.0),
                iced::Size::new(6.0, self.current[channel].clamp(0.0, 24.0) / 24.0 * h),
                self.accent,
            );
            let peak = self.peak[channel].clamp(0.0, 24.0) / 24.0 * h + 1.0;
            frame.stroke(
                &canvas::Path::line(Point::new(x, peak), Point::new(x + 6.0, peak)),
                canvas::Stroke::default().with_color(th::text()),
            );
        }
        for db in [0.0, 6.0, 12.0, 24.0] {
            if bounds.height < 64.0 && db != 0.0 && db != 24.0 {
                continue;
            }
            frame.fill_text(canvas::Text {
                content: format!("-{db:.0}"),
                position: Point::new(20.0, 1.0 + db / 24.0 * h),
                color: th::text_dim(),
                size: 8.0.into(),
                vertical_alignment: if db == 0.0 {
                    iced::alignment::Vertical::Top
                } else if db == 24.0 {
                    iced::alignment::Vertical::Bottom
                } else {
                    iced::alignment::Vertical::Center
                },
                ..Default::default()
            });
        }
        vec![frame.into_geometry()]
    }
}
