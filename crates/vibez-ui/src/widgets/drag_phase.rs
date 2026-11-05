//! A press that becomes a drag only once the pointer travels past the click
//! slop, so a shaky click means the same on the timeline lanes as in the
//! piano roll.

use iced::Point;

/// How far, in pixels along either axis, a press may wobble and still count
/// as a click: a rubber-band rather than a seek or deselect, or a move
/// rather than a click that would snap an off-grid clip or note.
pub const CLICK_SLOP_PX: f32 = 4.0;

/// Whether the pointer at `to` has left a click pressed at `from`.
pub fn past_click_slop(from: Point, to: Point) -> bool {
    past_slop(to.x - from.x) || past_slop(to.y - from.y)
}

fn past_slop(travel: f32) -> bool {
    travel.abs() > CLICK_SLOP_PX
}

/// Whether a press is still a click or has become a drag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DragPhase {
    #[default]
    Pending,
    Dragging,
}

impl DragPhase {
    /// Becomes `Dragging` once the pointer pressed at `from` passes the click
    /// slop at `to`, and stays so; true while dragging.
    pub fn follow(&mut self, from: Point, to: Point) -> bool {
        self.advance(past_click_slop(from, to))
    }

    /// [`Self::follow`] for a drag along x alone, which ignores any
    /// vertical wobble.
    pub fn follow_x(&mut self, from_x: f32, to_x: f32) -> bool {
        self.advance(past_slop(to_x - from_x))
    }

    fn advance(&mut self, past_slop: bool) -> bool {
        if *self == Self::Pending && past_slop {
            *self = Self::Dragging;
        }
        *self == Self::Dragging
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn travel_up_to_the_slop_is_still_a_click() {
        let pressed = Point::new(100.0, 50.0);

        assert!(!past_click_slop(pressed, Point::new(104.0, 46.0)));
        assert!(past_click_slop(pressed, Point::new(104.5, 50.0)));
        assert!(past_click_slop(pressed, Point::new(100.0, 54.5)));
    }

    #[test]
    fn a_drag_stays_a_drag_when_the_pointer_comes_back() {
        let pressed = Point::new(100.0, 50.0);
        let mut phase = DragPhase::default();

        assert!(!phase.follow(pressed, Point::new(102.0, 50.0)));
        assert!(phase.follow(pressed, Point::new(110.0, 50.0)));
        assert!(phase.follow(pressed, pressed));
    }
}
