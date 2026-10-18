//! Stereo dBFS level meter shared by native devices that display signal level.

use iced::mouse;
use iced::widget::canvas;
use iced::{Point, Rectangle, Renderer, Size, Theme};
use vibez_engine::metering::MeterValues;

use crate::message::Message;
use crate::theme as th;
use vibez_core::level::amplitude_to_db;

/// Scale marks, top to bottom. They are spaced evenly so the musically busy
/// top of the range gets as much room as the quiet bottom.
pub const SCALE_DB: [f32; 6] = [0.0, -6.0, -12.0, -24.0, -36.0, -48.0];
pub const FLOOR_DB: f32 = SCALE_DB[SCALE_DB.len() - 1];
const SHORT_SCALE_DB: [f32; 3] = [0.0, -12.0, -48.0];

/// Width of the two bars and their scale.
pub const WIDTH: f32 = 44.0;

const BAR_W: f32 = 9.0;
const BAR_GAP: f32 = 3.0;
const YELLOW_FROM_DB: f32 = -12.0;
const RED_FROM_DB: f32 = -3.0;

/// Distance from the top of the scale, 0 at 0 dBFS and 1 at its floor.
pub fn scale_position(db: f32) -> f32 {
    let last = SCALE_DB.len() - 1;
    if db >= SCALE_DB[0] {
        return 0.0;
    }
    if db <= SCALE_DB[last] || db.is_nan() {
        return 1.0;
    }
    let segment = SCALE_DB
        .windows(2)
        .position(|pair| db <= pair[0] && db >= pair[1])
        .unwrap_or(last - 1);
    let (upper, lower) = (SCALE_DB[segment], SCALE_DB[segment + 1]);
    (segment as f32 + (upper - db) / (upper - lower)) / last as f32
}

pub fn scale_marks(height: f32) -> &'static [f32] {
    if height < 72.0 {
        &SHORT_SCALE_DB
    } else {
        &SCALE_DB
    }
}

/// One scale number, right-aligned to `anchor`. The end marks sit inside the
/// scale so neither is clipped by the plot edges.
pub fn draw_scale_label(frame: &mut canvas::Frame, db: f32, anchor: Point) {
    frame.fill_text(canvas::Text {
        content: format!("{db:.0}"),
        position: anchor,
        color: th::text_dim(),
        size: 8.0.into(),
        horizontal_alignment: iced::alignment::Horizontal::Right,
        vertical_alignment: if db == SCALE_DB[0] {
            iced::alignment::Vertical::Top
        } else if db == FLOOR_DB {
            iced::alignment::Vertical::Bottom
        } else {
            iced::alignment::Vertical::Center
        },
        ..canvas::Text::default()
    });
}

pub struct LevelMeter {
    pub levels: MeterValues,
}

impl canvas::Program<Message> for LevelMeter {
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
        let height = (bounds.height - 2.0).max(8.0);
        let y_of = |db: f32| 1.0 + height * scale_position(db);
        for (channel, (peak, rms)) in [
            (self.levels.peak_l, self.levels.rms_l),
            (self.levels.peak_r, self.levels.rms_r),
        ]
        .into_iter()
        .enumerate()
        {
            let x = channel as f32 * (BAR_W + BAR_GAP);
            frame.fill_rectangle(
                Point::new(x, 1.0),
                Size::new(BAR_W, height),
                th::fader_track(),
            );
            let level_db = amplitude_to_db(rms);
            for (from, to, color) in [
                (f32::NEG_INFINITY, YELLOW_FROM_DB, th::meter_green()),
                (YELLOW_FROM_DB, RED_FROM_DB, th::meter_yellow()),
                (RED_FROM_DB, 0.0, th::meter_red()),
            ] {
                if level_db <= from {
                    continue;
                }
                let top = y_of(level_db.min(to));
                let bottom = y_of(from);
                if bottom > top {
                    frame.fill_rectangle(Point::new(x, top), Size::new(BAR_W, bottom - top), color);
                }
            }
            let peak_db = amplitude_to_db(peak);
            if peak_db > FLOOR_DB {
                frame.fill_rectangle(
                    Point::new(x, y_of(peak_db).min(height - 1.0)),
                    Size::new(BAR_W, 2.0),
                    if peak_db >= 0.0 {
                        th::meter_red()
                    } else {
                        th::text_dim()
                    },
                );
            }
        }
        for &db in scale_marks(height) {
            draw_scale_label(&mut frame, db, Point::new(bounds.width, y_of(db)));
        }
        vec![frame.into_geometry()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scale_marks_are_evenly_spaced_and_monotonic() {
        let last = (SCALE_DB.len() - 1) as f32;
        for (index, &db) in SCALE_DB.iter().enumerate() {
            assert!(
                (scale_position(db) - index as f32 / last).abs() < 1e-6,
                "{db}"
            );
        }
        assert_eq!(scale_position(6.0), 0.0);
        assert_eq!(scale_position(-90.0), 1.0);
        assert_eq!(scale_position(f32::NEG_INFINITY), 1.0);
        let mut previous = -1.0;
        for step in 0..=480 {
            let position = scale_position(-(step as f32) / 10.0);
            assert!(position >= previous);
            previous = position;
        }
    }
}
