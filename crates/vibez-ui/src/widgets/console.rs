//! The mix desk's console parts: LED ladder meters and the dark channel
//! name display.

use iced::mouse;
use iced::widget::canvas;
use iced::{Color, Point, Rectangle, Renderer, Size, Theme};
use vibez_core::level::amplitude_to_db;

use crate::message::Message;
use crate::theme as th;

pub const DISPLAY: Color = Color::from_rgb(0.047, 0.051, 0.059);
pub const DISPLAY_TEXT: Color = Color::from_rgb(0.886, 0.910, 0.937);
pub const LED_RED: Color = Color::from_rgb(1.0, 0.290, 0.239);
pub const LED_YELLOW: Color = Color::from_rgb(1.0, 0.824, 0.239);
pub const LED_GREEN: Color = Color::from_rgb(0.298, 1.0, 0.416);

/// The level each LED lights at, bottom to top, in dBFS. Like a desk it
/// packs the segments near 0 dB, where gain staging happens.
pub const LED_DB: [f32; 23] = [
    -60.0, -50.0, -42.0, -36.0, -30.0, -27.0, -24.0, -21.0, -18.0, -15.0, -12.0, -10.0, -8.0, -6.0,
    -5.0, -4.0, -3.0, -2.0, -1.0, 0.0, 1.0, 2.0, 3.0,
];
/// The first yellow and the first red LED.
const YELLOW_FROM: usize = 13;
const RED_FROM: usize = 19;

/// How many LEDs a linear peak lights.
pub fn lit_leds(peak: f32) -> usize {
    let db = amplitude_to_db(peak);
    LED_DB
        .iter()
        .take_while(|threshold| db >= **threshold)
        .count()
}

/// A stereo LED ladder for a channel's output peak.
pub struct LedMeter {
    pub peak_l: f32,
    pub peak_r: f32,
}

impl canvas::Program<Message> for LedMeter {
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
        let gap = 2.0;
        let column = ((bounds.width - gap) / 2.0).max(2.0);
        let count = LED_DB.len() as f32;
        let led = ((bounds.height - gap * (count - 1.0)) / count).max(1.0);
        for (side, peak) in [self.peak_l, self.peak_r].into_iter().enumerate() {
            let lit = lit_leds(peak);
            let x = side as f32 * (column + gap);
            for index in 0..LED_DB.len() {
                let on = match index {
                    i if i >= RED_FROM => LED_RED,
                    i if i >= YELLOW_FROM => LED_YELLOW,
                    _ => LED_GREEN,
                };
                let colour = if index < lit {
                    on
                } else {
                    th::blend(th::bg_dark(), on, 0.14)
                };
                let y = bounds.height - (index as f32 + 1.0) * led - index as f32 * gap;
                frame.fill_rectangle(Point::new(x, y), Size::new(column, led), colour);
            }
        }
        vec![frame.into_geometry()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vibez_core::level::db_to_amplitude;

    #[test]
    fn leds_light_on_a_db_ladder_up_to_the_peak() {
        assert_eq!(lit_leds(0.0), 0);
        assert_eq!(lit_leds(db_to_amplitude(-61.0)), 0);
        assert_eq!(lit_leds(db_to_amplitude(-60.0)), 1);
        assert_eq!(lit_leds(db_to_amplitude(-6.0)), YELLOW_FROM + 1);
        assert_eq!(lit_leds(1.0), RED_FROM + 1, "0 dBFS lights the first red");
        assert_eq!(lit_leds(db_to_amplitude(12.0)), LED_DB.len());
    }
}
