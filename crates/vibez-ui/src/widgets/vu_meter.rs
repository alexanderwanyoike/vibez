use iced::mouse;
use iced::widget::canvas;
use iced::{Rectangle, Renderer, Theme};

use crate::theme;

pub struct VuMeterWidget {
    pub peak_l: f32,
    pub peak_r: f32,
}

impl Default for VuMeterWidget {
    fn default() -> Self {
        Self {
            peak_l: 0.0,
            peak_r: 0.0,
        }
    }
}

impl VuMeterWidget {
    pub fn new() -> Self {
        Self::default()
    }
}

impl canvas::Program<crate::message::Message> for VuMeterWidget {
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
            theme::BG_SURFACE,
        );

        let bar_width = (w / 2.0 - 3.0).max(4.0);
        let padding = 2.0;
        let max_height = h - padding * 2.0;

        // Draw a single meter bar
        let draw_bar = |frame: &mut canvas::Frame, x: f32, level: f32| {
            let bar_height = (level * max_height).clamp(0.0, max_height);
            let y = h - padding - bar_height;

            // Three-color gradient segments
            let green_threshold = 0.6;
            let yellow_threshold = 0.85;

            if level > 0.0 {
                // Green portion (0..0.6)
                let green_h = (level.min(green_threshold) * max_height).max(0.0);
                frame.fill_rectangle(
                    iced::Point::new(x, h - padding - green_h),
                    iced::Size::new(bar_width, green_h),
                    theme::METER_GREEN,
                );

                // Yellow portion (0.6..0.85)
                if level > green_threshold {
                    let yellow_h =
                        ((level.min(yellow_threshold) - green_threshold) * max_height).max(0.0);
                    let yellow_y = h - padding - green_threshold * max_height - yellow_h;
                    frame.fill_rectangle(
                        iced::Point::new(x, yellow_y),
                        iced::Size::new(bar_width, yellow_h),
                        theme::METER_YELLOW,
                    );
                }

                // Red portion (0.85..1.0)
                if level > yellow_threshold {
                    let red_h = ((level.min(1.0) - yellow_threshold) * max_height).max(0.0);
                    let red_y = h - padding - yellow_threshold * max_height - red_h;
                    frame.fill_rectangle(
                        iced::Point::new(x, red_y),
                        iced::Size::new(bar_width, red_h),
                        theme::METER_RED,
                    );
                }
            }

            // Border
            let border = canvas::Path::rectangle(
                iced::Point::new(x, y),
                iced::Size::new(bar_width, bar_height),
            );
            frame.stroke(
                &border,
                canvas::Stroke::default()
                    .with_color(iced::Color {
                        a: 0.2,
                        ..theme::TEXT
                    })
                    .with_width(0.5),
            );
        };

        let left_x = padding;
        let right_x = w / 2.0 + 1.0;

        draw_bar(&mut frame, left_x, self.peak_l);
        draw_bar(&mut frame, right_x, self.peak_r);

        vec![frame.into_geometry()]
    }
}

/// Horizontal VU meter for arrangement track headers.
/// Two bars stacked vertically (L on top, R below), growing left to right.
pub struct HorizontalVuMeterWidget {
    pub peak_l: f32,
    pub peak_r: f32,
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
            theme::BG_SURFACE,
        );

        let padding = 1.0;
        let bar_height = (h / 2.0 - padding * 1.5).max(2.0);
        let max_width = w - padding * 2.0;

        let draw_bar = |frame: &mut canvas::Frame, y: f32, level: f32| {
            let green_threshold = 0.6;
            let yellow_threshold = 0.85;

            if level > 0.0 {
                // Green portion (0..0.6)
                let green_w = (level.min(green_threshold) * max_width).max(0.0);
                frame.fill_rectangle(
                    iced::Point::new(padding, y),
                    iced::Size::new(green_w, bar_height),
                    theme::METER_GREEN,
                );

                // Yellow portion (0.6..0.85)
                if level > green_threshold {
                    let green_end = green_threshold * max_width;
                    let yellow_w =
                        ((level.min(yellow_threshold) - green_threshold) * max_width).max(0.0);
                    frame.fill_rectangle(
                        iced::Point::new(padding + green_end, y),
                        iced::Size::new(yellow_w, bar_height),
                        theme::METER_YELLOW,
                    );
                }

                // Red portion (0.85..1.0)
                if level > yellow_threshold {
                    let yellow_end = yellow_threshold * max_width;
                    let red_w = ((level.min(1.0) - yellow_threshold) * max_width).max(0.0);
                    frame.fill_rectangle(
                        iced::Point::new(padding + yellow_end, y),
                        iced::Size::new(red_w, bar_height),
                        theme::METER_RED,
                    );
                }
            }
        };

        let top_y = padding;
        let bottom_y = h / 2.0 + padding * 0.5;

        draw_bar(&mut frame, top_y, self.peak_l);
        draw_bar(&mut frame, bottom_y, self.peak_r);

        vec![frame.into_geometry()]
    }
}
