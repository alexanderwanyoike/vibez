use iced::mouse;
use iced::widget::canvas;
use iced::{Color, Rectangle, Renderer, Theme};

use crate::theme;

/// Horizontal stereo VU meter for track headers and the control bar's
/// output. Two bars stacked vertically (L on top, R below), growing left
/// to right in the given colour.
pub struct HorizontalVuMeterWidget {
    pub peak_l: f32,
    pub peak_r: f32,
    pub track_color: Color,
}

impl HorizontalVuMeterWidget {
    pub fn new(peak_l: f32, peak_r: f32, track_color: Color) -> Self {
        Self {
            peak_l,
            peak_r,
            track_color,
        }
    }
}

impl canvas::Program<crate::message::Message> for HorizontalVuMeterWidget {
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
        let w = bounds.width;
        let h = bounds.height;

        // Background
        frame.fill_rectangle(
            iced::Point::ORIGIN,
            iced::Size::new(w, h),
            theme::bg_elevated(),
        );

        let padding = 1.0;
        let bar_height = (h / 2.0 - padding * 1.5).max(2.0);
        let max_width = w - padding * 2.0;

        let track_color = self.track_color;

        let draw_bar = |frame: &mut canvas::Frame, y: f32, level: f32| {
            if level > 0.0 {
                let bar_w = (level * max_width).clamp(0.0, max_width);
                // Use track color with brightness modulated by level
                let color = if level > 0.85 {
                    theme::meter_red()
                } else if level > 0.6 {
                    theme::meter_yellow()
                } else {
                    track_color
                };
                frame.fill_rectangle(
                    iced::Point::new(padding, y),
                    iced::Size::new(bar_w, bar_height),
                    color,
                );
            }
        };

        let top_y = padding;
        let bottom_y = h / 2.0 + padding * 0.5;

        draw_bar(&mut frame, top_y, self.peak_l);
        draw_bar(&mut frame, bottom_y, self.peak_r);

        vec![frame.into_geometry()]
    }
}
