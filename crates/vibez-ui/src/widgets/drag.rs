//! Shared drag tracking for canvas value controls (faders, knobs).
//!
//! Every control that maps mouse drags onto a value needs the same
//! three guarantees, and losing any of them makes the control feel
//! sticky:
//!
//! - the grab is gated on the widget, but motion is tracked in
//!   absolute cursor coordinates, so the drag keeps working when the
//!   cursor leaves the (usually thin) widget mid-drag;
//! - the value accumulates inside the drag state rather than being
//!   re-derived from the widget's `self.value`, because several
//!   `CursorMoved` events can arrive between two view rebuilds and
//!   incremental `self.value + delta` math silently drops every
//!   delta in the batch except the last;
//! - clamping happens on the accumulated value, so reversing
//!   direction at the range ends responds immediately.

use std::ops::RangeInclusive;

use iced::mouse;
use iced::{Point, Rectangle};

#[derive(Debug, Default)]
pub struct ValueDrag {
    drag: Option<Drag>,
}

#[derive(Debug)]
struct Drag {
    last: Point,
    value: f32,
}

impl ValueDrag {
    /// Start tracking when the press lands on the widget; `value` is
    /// the control's value at grab time. Returns true when grabbed.
    pub fn grab(&mut self, cursor: mouse::Cursor, bounds: Rectangle, value: f32) -> bool {
        if !cursor.is_over(bounds) {
            return false;
        }
        let Some(last) = cursor.position() else {
            return false;
        };
        self.drag = Some(Drag { last, value });
        true
    }

    /// Stop tracking. Returns true when a drag was active.
    pub fn release(&mut self) -> bool {
        self.drag.take().is_some()
    }

    pub fn is_active(&self) -> bool {
        self.drag.is_some()
    }

    /// Advance the drag to the current cursor position and return
    /// the new value: rightward motion adds `value_per_x` per pixel,
    /// downward motion adds `value_per_y` per pixel, and the
    /// accumulated value clamps to `range`.
    pub fn drag_to(
        &mut self,
        cursor: mouse::Cursor,
        value_per_x: f32,
        value_per_y: f32,
        range: RangeInclusive<f32>,
    ) -> Option<f32> {
        let drag = self.drag.as_mut()?;
        let pos = cursor.position()?;
        let value =
            drag.value + (pos.x - drag.last.x) * value_per_x + (pos.y - drag.last.y) * value_per_y;
        drag.last = pos;
        drag.value = value.clamp(*range.start(), *range.end());
        Some(drag.value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::Size;

    fn bounds() -> Rectangle {
        Rectangle::new(Point::new(100.0, 100.0), Size::new(24.0, 100.0))
    }

    fn at(x: f32, y: f32) -> mouse::Cursor {
        mouse::Cursor::Available(Point::new(x, y))
    }

    #[test]
    fn grab_requires_the_widget_but_dragging_does_not() {
        let mut drag = ValueDrag::default();
        assert!(!drag.grab(at(10.0, 10.0), bounds(), 1.0));
        assert!(drag.grab(at(110.0, 150.0), bounds(), 1.0));
        // Cursor far outside the widget keeps tracking.
        let value = drag.drag_to(at(300.0, 140.0), 0.0, -0.02, 0.0..=2.0);
        assert_eq!(value, Some(1.2));
    }

    #[test]
    fn batched_moves_accumulate_instead_of_dropping_deltas() {
        let mut drag = ValueDrag::default();
        assert!(drag.grab(at(110.0, 150.0), bounds(), 1.0));
        // Two moves between view rebuilds: both deltas must count.
        drag.drag_to(at(110.0, 145.0), 0.0, -0.02, 0.0..=2.0);
        let value = drag.drag_to(at(110.0, 140.0), 0.0, -0.02, 0.0..=2.0);
        assert_eq!(value, Some(1.2));
    }

    #[test]
    fn reversing_at_the_clamp_edge_responds_immediately() {
        let mut drag = ValueDrag::default();
        assert!(drag.grab(at(110.0, 150.0), bounds(), 1.9));
        // Overshoot far past the max...
        assert_eq!(
            drag.drag_to(at(110.0, 50.0), 0.0, -0.02, 0.0..=2.0),
            Some(2.0)
        );
        // ...then a small reversal moves the value right away.
        let value = drag.drag_to(at(110.0, 60.0), 0.0, -0.02, 0.0..=2.0);
        assert_eq!(value, Some(1.8));
    }

    #[test]
    fn release_ends_tracking() {
        let mut drag = ValueDrag::default();
        assert!(drag.grab(at(110.0, 150.0), bounds(), 1.0));
        assert!(drag.release());
        assert!(!drag.release());
        assert_eq!(drag.drag_to(at(110.0, 140.0), 0.0, -0.02, 0.0..=2.0), None);
    }
}
