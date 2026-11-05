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
    (to.x - from.x).abs() > CLICK_SLOP_PX || (to.y - from.y).abs() > CLICK_SLOP_PX
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
}
