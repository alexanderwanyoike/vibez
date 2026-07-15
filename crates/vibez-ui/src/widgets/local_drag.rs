//! Shared cursor-to-local translation for the big timeline editors
//! (piano roll, arrangement clips, automation lane).
//!
//! Each one tracks drag motion from `cursor.position()` in absolute
//! window coordinates rather than the widget-local position, because
//! the drag has to keep working when the cursor leaves the widget
//! mid-gesture (the same reason [`super::drag::ValueDrag`] tracks
//! absolute motion). That leaves every editor hand-rolling the same
//! `absolute - bounds.origin` subtraction, and disagreeing only on
//! whether the result is clamped back into `bounds`:
//!
//! - lanes that pin a runaway drag to the nearest edge clamp, so the
//!   value stays live instead of stalling at the boundary;
//! - editors whose coordinates legitimately extend past the visible
//!   area leave the result unclamped.

use iced::mouse;
use iced::{Point, Rectangle};

/// Translates the absolute cursor position into `bounds`-local
/// coordinates, optionally clamping the result into `bounds`.
#[derive(Debug, Clone, Copy)]
pub struct LocalDrag {
    clamp: bool,
}

impl LocalDrag {
    /// Local coordinates may run outside `bounds` (editors that let a
    /// gesture extend past the visible area).
    pub const fn unclamped() -> Self {
        Self { clamp: false }
    }

    /// Local coordinates are pinned into `bounds`, so a drag that
    /// wanders off an edge stays live at the nearest edge.
    pub const fn clamped() -> Self {
        Self { clamp: true }
    }

    /// The current cursor position in `bounds`-local coordinates, or
    /// `None` when the cursor position is unavailable.
    pub fn position(self, cursor: mouse::Cursor, bounds: Rectangle) -> Option<Point> {
        let absolute = cursor.position()?;
        let mut local = Point::new(absolute.x - bounds.x, absolute.y - bounds.y);
        if self.clamp {
            local.x = local.x.clamp(0.0, bounds.width);
            local.y = local.y.clamp(0.0, bounds.height);
        }
        Some(local)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::Size;

    fn bounds() -> Rectangle {
        Rectangle::new(Point::new(100.0, 50.0), Size::new(200.0, 80.0))
    }

    fn at(x: f32, y: f32) -> mouse::Cursor {
        mouse::Cursor::Available(Point::new(x, y))
    }

    #[test]
    fn translates_into_local_coordinates() {
        let local = LocalDrag::unclamped().position(at(150.0, 90.0), bounds());
        assert_eq!(local, Some(Point::new(50.0, 40.0)));
    }

    #[test]
    fn unclamped_lets_coordinates_run_past_the_edge() {
        // Far below and left of the widget.
        let local = LocalDrag::unclamped().position(at(60.0, 300.0), bounds());
        assert_eq!(local, Some(Point::new(-40.0, 250.0)));
    }

    #[test]
    fn clamped_pins_coordinates_into_bounds() {
        let local = LocalDrag::clamped().position(at(60.0, 300.0), bounds());
        assert_eq!(local, Some(Point::new(0.0, 80.0)));
    }

    #[test]
    fn no_cursor_position_yields_none() {
        assert_eq!(
            LocalDrag::clamped().position(mouse::Cursor::Unavailable, bounds()),
            None
        );
    }
}
