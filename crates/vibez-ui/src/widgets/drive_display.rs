//! Plots the DSP's signed static transfer and genuinely aligned left-channel sample pairs.

use crate::{message::Message, theme as th};
use iced::{mouse, widget::canvas, Color, Point, Rectangle, Renderer, Theme};
use vibez_core::effect::WaveShaperMeter;

pub struct TransferCurve<'a> {
    pub meter: Option<&'a WaveShaperMeter>,
    pub accent: Color,
}

pub struct PairedWaveform<'a> {
    pub meter: Option<&'a WaveShaperMeter>,
    pub accent: Color,
}

fn plot(bounds: Rectangle) -> Rectangle {
    Rectangle {
        x: 32.0,
        y: 14.0,
        width: (bounds.width - 40.0).max(1.0),
        height: (bounds.height - 32.0).max(1.0),
    }
}

fn amplitude(value: f32) -> String {
    if value.abs() >= 1000.0 {
        format!("{value:.1e}")
    } else {
        format!("{value:.1}")
    }
}

fn grid(frame: &mut canvas::Frame, bounds: Rectangle, limit: f32) -> Rectangle {
    frame.fill_rectangle(Point::ORIGIN, bounds.size(), th::bg_dark());
    frame.stroke(
        &canvas::Path::rectangle(Point::ORIGIN, bounds.size()),
        canvas::Stroke::default().with_color(th::border()),
    );
    let area = plot(bounds);
    for index in 0..=4 {
        let fraction = index as f32 / 4.0;
        let x = area.x + area.width * fraction;
        let y = area.y + area.height * fraction;
        for (from, to) in [
            (Point::new(x, area.y), Point::new(x, area.y + area.height)),
            (Point::new(area.x, y), Point::new(area.x + area.width, y)),
        ] {
            frame.stroke(
                &canvas::Path::line(from, to),
                canvas::Stroke::default().with_color(th::divider()),
            );
        }
        frame.fill_text(canvas::Text {
            content: amplitude(limit * (1.0 - 2.0 * fraction)),
            position: Point::new(area.x - 4.0, y),
            color: th::text_dim(),
            size: 8.0.into(),
            horizontal_alignment: iced::alignment::Horizontal::Right,
            vertical_alignment: if index == 0 {
                iced::alignment::Vertical::Top
            } else if index == 4 {
                iced::alignment::Vertical::Bottom
            } else {
                iced::alignment::Vertical::Center
            },
            ..Default::default()
        });
    }
    area
}

fn unavailable(frame: &mut canvas::Frame, area: Rectangle) {
    frame.fill_text(canvas::Text {
        content: "Unavailable".into(),
        position: area.center(),
        color: th::text_dim(),
        size: 9.0.into(),
        horizontal_alignment: iced::alignment::Horizontal::Center,
        vertical_alignment: iced::alignment::Vertical::Center,
        ..Default::default()
    });
}

impl canvas::Program<Message> for TransferCurve<'_> {
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
        let limit = self.meter.map_or(1.0, |meter| {
            meter
                .transfer
                .iter()
                .map(|value| value.abs())
                .fold(1.0_f32, f32::max)
        });
        let area = grid(&mut frame, bounds, limit);
        let x = |value: f32| area.x + (value + 1.0) * 0.5 * area.width;
        let y = |value: f32| area.y + (1.0 - value / limit) * 0.5 * area.height;
        frame.fill_text(canvas::Text {
            content: "Output amplitude / Input →".into(),
            position: Point::new(area.x, 2.0),
            color: th::text_dim(),
            size: 8.0.into(),
            ..Default::default()
        });
        for value in [-1.0, -0.5, 0.0, 0.5, 1.0] {
            frame.fill_text(canvas::Text {
                content: amplitude(value),
                position: Point::new(x(value), bounds.height - 2.0),
                color: th::text_dim(),
                size: 8.0.into(),
                horizontal_alignment: iced::alignment::Horizontal::Center,
                vertical_alignment: iced::alignment::Vertical::Bottom,
                ..Default::default()
            });
        }
        if let Some(meter) = self.meter {
            frame.stroke(
                &canvas::Path::line(Point::new(x(-1.0), y(-1.0)), Point::new(x(1.0), y(1.0))),
                canvas::Stroke::default().with_color(th::text_muted()),
            );
            let curve = canvas::Path::new(|path| {
                for (index, value) in meter.transfer.iter().enumerate() {
                    let point = Point::new(x(-1.0 + 2.0 * index as f32 / 64.0), y(*value));
                    if index == 0 {
                        path.move_to(point);
                    } else {
                        path.line_to(point);
                    }
                }
            });
            frame.stroke(
                &curve,
                canvas::Stroke::default()
                    .with_color(self.accent)
                    .with_width(2.0),
            );
        } else {
            unavailable(&mut frame, area);
        }
        vec![frame.into_geometry()]
    }
}

impl canvas::Program<Message> for PairedWaveform<'_> {
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
        let limit = self.meter.map_or(1.0, |meter| {
            let count = usize::from(meter.count);
            meter.input[..count]
                .iter()
                .chain(&meter.output[..count])
                .map(|value| value.abs())
                .fold(1.0_f32, f32::max)
        });
        let area = grid(&mut frame, bounds, limit);
        frame.fill_text(canvas::Text {
            content: "Input / Output · amplitude".into(),
            position: Point::new(area.x, 2.0),
            color: th::text_dim(),
            size: 8.0.into(),
            ..Default::default()
        });
        if let Some(meter) = self.meter {
            let count = usize::from(meter.count);
            let denominator = count.saturating_sub(1).max(1) as f32;
            let x = |index: usize| area.x + index as f32 / denominator * area.width;
            let y = |value: f32| area.y + (1.0 - value / limit) * 0.5 * area.height;
            for (values, color, width) in [
                (&meter.input, th::text_dim(), 1.0),
                (&meter.output, self.accent, 1.5),
            ] {
                if count == 1 {
                    frame.fill(
                        &canvas::Path::circle(Point::new(x(0), y(values[0])), 1.5),
                        color,
                    );
                } else {
                    let line = canvas::Path::new(|path| {
                        for (index, value) in values[..count].iter().enumerate() {
                            let point = Point::new(x(index), y(*value));
                            if index == 0 {
                                path.move_to(point);
                            } else {
                                path.line_to(point);
                            }
                        }
                    });
                    frame.stroke(
                        &line,
                        canvas::Stroke::default()
                            .with_color(color)
                            .with_width(width),
                    );
                }
            }
            for (index, alignment) in [
                (0, iced::alignment::Horizontal::Left),
                (count.saturating_sub(1), iced::alignment::Horizontal::Right),
            ] {
                frame.fill_text(canvas::Text {
                    content: meter
                        .first_output_sample
                        .saturating_add(index as u64)
                        .to_string(),
                    position: Point::new(x(index), bounds.height - 2.0),
                    color: th::text_dim(),
                    size: 8.0.into(),
                    horizontal_alignment: alignment,
                    vertical_alignment: iced::alignment::Vertical::Bottom,
                    ..Default::default()
                });
            }
        } else {
            unavailable(&mut frame, area);
        }
        vec![frame.into_geometry()]
    }
}
