use iced::widget::canvas;
use iced::{Rectangle, Renderer};

use crate::theme;

/// Draw an uncached playhead overlay only when a position is visible.
pub(super) fn playhead_geometry(
    renderer: &Renderer,
    bounds: Rectangle,
    x: Option<f32>,
) -> Option<canvas::Geometry> {
    let x = x?.clamp(0.0, bounds.width);
    let mut frame = canvas::Frame::new(renderer, bounds.size());
    let line = canvas::Path::line(iced::Point::new(x, 0.0), iced::Point::new(x, bounds.height));
    frame.stroke(
        &line,
        canvas::Stroke::default()
            .with_color(theme::playhead())
            .with_width(1.5),
    );
    Some(frame.into_geometry())
}
