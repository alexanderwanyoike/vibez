//! Draws actual left-channel input/output sample pairs, source timing and acquisition masks.

use crate::{message::Message, theme as th};
use iced::{mouse, widget::canvas, Color, Point, Rectangle, Renderer, Theme};
use vibez_core::effect::SampleHoldMeter;

pub struct SampleHoldScope {
    pub meter: Option<SampleHoldMeter>,
    pub accent: Color,
}

fn marked(mask: [u64; 2], index: usize) -> bool {
    mask[index / 64] & (1_u64 << (index % 64)) != 0
}

fn caption(frame: &mut canvas::Frame, value: String, position: Point, color: Color) {
    frame.fill_text(canvas::Text {
        content: value,
        position,
        color,
        size: 8.0.into(),
        ..Default::default()
    });
}

impl canvas::Program<Message> for SampleHoldScope {
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
        frame.fill_rectangle(Point::ORIGIN, bounds.size(), th::bg_dark());
        frame.stroke(
            &canvas::Path::rectangle(Point::ORIGIN, bounds.size()),
            canvas::Stroke::default().with_color(th::border()),
        );
        let area = Rectangle {
            x: 32.0,
            y: 22.0,
            width: (bounds.width - 40.0).max(1.0),
            height: (bounds.height - 44.0).max(1.0),
        };
        let limit = self.meter.map_or(1.0, |meter| {
            let count = usize::from(meter.count);
            meter.input[..count].iter().chain(&meter.output[..count])
                .map(|value| value.abs()).fold(1.0_f32, f32::max)
        });
        for index in 0..=4 {
            let fraction = index as f32 / 4.0;
            let x = area.x + area.width * fraction;
            let y = area.y + area.height * fraction;
            for (from, to) in [
                (Point::new(x, area.y), Point::new(x, area.y + area.height)),
                (Point::new(area.x, y), Point::new(area.x + area.width, y)),
            ] {
                frame.stroke(&canvas::Path::line(from, to), canvas::Stroke::default().with_color(th::divider()));
            }
            if index % 2 == 0 {
                frame.fill_text(canvas::Text {
                    content: {
                        let amplitude = limit * (1.0 - 2.0 * fraction);
                        if amplitude.abs() >= 1000.0 { format!("{amplitude:.1e}") } else { format!("{amplitude:.1}") }
                    },
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
        }
        caption(&mut frame, "L · unavailable".into(), Point::new(5.0, 3.0), th::text_dim());
        caption(&mut frame, "Input".into(), Point::new(bounds.width - 104.0, 3.0), th::text_dim());
        caption(&mut frame, "Output".into(), Point::new(bounds.width - 55.0, 3.0), self.accent);
        if let Some(meter) = self.meter {
            frame.fill_rectangle(Point::new(2.0, 2.0), iced::Size::new(160.0, 14.0), th::bg_dark());
            caption(&mut frame,
                format!("L · {} samples @ {:.1}k", meter.count, meter.sample_rate / 1000.0),
                Point::new(5.0, 3.0), th::text_dim());
            let count = usize::from(meter.count);
            let denominator = count.saturating_sub(1).max(1) as f32;
            let x = |index: usize| area.x + index as f32 / denominator * area.width;
            let y = |value: f32| area.y + (1.0 - value / limit) * 0.5 * area.height;
            for index in 0..count {
                if marked(meter.blending, index) {
                    let half_sample = area.width / denominator * 0.5;
                    let left = (x(index) - half_sample).max(area.x);
                    let right = (x(index) + half_sample).min(area.x + area.width);
                    frame.fill_rectangle(Point::new(left, area.y),
                        iced::Size::new(right - left, area.height), th::with_alpha(self.accent, 0.08));
                }
                for (mask, offset, color) in [
                    (meter.primary_acquisitions, 0.0, th::text_dim()),
                    (meter.secondary_acquisitions, 4.0, self.accent),
                ] {
                    if marked(mask, index) {
                        frame.stroke(&canvas::Path::line(
                            Point::new(x(index), area.y + area.height - offset - 3.0),
                            Point::new(x(index), area.y + area.height - offset)),
                            canvas::Stroke::default().with_color(color));
                    }
                }
            }
            for (values, color, width) in [
                (&meter.input, th::text_dim(), 1.0),
                (&meter.output, self.accent, 1.5),
            ] {
                if count == 1 {
                    frame.fill(&canvas::Path::circle(Point::new(x(0), y(values[0])), 1.5), color);
                } else {
                    let trace = canvas::Path::new(|path| {
                        for (index, value) in values[..count].iter().enumerate() {
                            let point = Point::new(x(index), y(*value));
                            if index == 0 { path.move_to(point); } else { path.line_to(point); }
                        }
                    });
                    frame.stroke(&trace, canvas::Stroke::default().with_color(color).with_width(width));
                }
            }
            caption(&mut frame, "0 ms".into(), Point::new(area.x, bounds.height - 20.0), th::text_dim());
            frame.fill_text(canvas::Text {
                content: format!("{:.3} ms", count.saturating_sub(1) as f64 / f64::from(meter.sample_rate) * 1000.0),
                position: Point::new(area.x + area.width, bounds.height - 20.0),
                color: th::text_dim(), size: 8.0.into(),
                horizontal_alignment: iced::alignment::Horizontal::Right,
                ..Default::default()
            });
            caption(&mut frame, format!("f{} · ticks 1/2 · tint: blend", meter.first_source_frame),
                Point::new(area.x, bounds.height - 10.0), th::text_dim());
        } else {
            frame.fill_text(canvas::Text {
                content: "Unavailable".into(), position: area.center(), color: th::text_dim(),
                size: 9.0.into(), horizontal_alignment: iced::alignment::Horizontal::Center,
                vertical_alignment: iced::alignment::Vertical::Center, ..Default::default()
            });
        }
        vec![frame.into_geometry()]
    }
}
