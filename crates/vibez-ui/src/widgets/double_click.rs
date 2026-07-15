//! Shared double-click detection for canvas controls (knobs, EQ
//! bands, breakpoints, empty piano-roll space).
//!
//! Every control that treats a quick second press as "reset" or "add"
//! needs the same guarantees, and getting them subtly wrong makes the
//! gesture fire when it shouldn't:
//!
//! - the pair is gated on a time window, so two unrelated clicks a
//!   second apart never merge into one gesture;
//! - an optional distance gate rejects a pair whose clicks drifted
//!   apart, so a double-click stays a click in place and never
//!   swallows the start of a drag;
//! - detection only *reports* the double-click; whether a completed
//!   one keeps the second press on record (so a third rapid click
//!   chains) or forgets it (so it doesn't) is left to the caller via
//!   [`DoubleClick::clear`], because the two editors that chain and
//!   the controls that don't both rely on their current behavior.
//!
//! Thresholds live at the call site, matching [`super::drag::ValueDrag`],
//! so each control keeps its own tuned window and distance.

use std::time::{Duration, Instant};

use iced::Point;

#[derive(Debug, Clone, Copy, Default)]
pub struct DoubleClick {
    last: Option<(Instant, Point)>,
}

impl DoubleClick {
    /// Record a left-press at `now`/`position` and report whether it
    /// completes a double-click with the previous press: it must fall
    /// within `window`, and within `max_distance` pixels on each axis
    /// when a distance gate is given. The press is always kept on
    /// record; call [`clear`](Self::clear) after consuming a
    /// double-click to stop a third rapid click chaining into it.
    pub fn press(
        &mut self,
        now: Instant,
        position: Point,
        window: Duration,
        max_distance: Option<f32>,
    ) -> bool {
        let is_double = self.last.is_some_and(|(last, at)| {
            now.duration_since(last) < window
                && max_distance.is_none_or(|max| {
                    (at.x - position.x).abs() < max && (at.y - position.y).abs() < max
                })
        });
        self.last = Some((now, position));
        is_double
    }

    /// Forget the pending press so the next one starts a fresh
    /// sequence (call after consuming a double-click, or when another
    /// gesture takes over the control).
    pub fn clear(&mut self) {
        self.last = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WINDOW: Duration = Duration::from_millis(300);

    fn at(x: f32, y: f32) -> Point {
        Point::new(x, y)
    }

    #[test]
    fn second_press_inside_the_window_is_a_double_click() {
        let mut dc = DoubleClick::default();
        let t0 = Instant::now();
        assert!(!dc.press(t0, at(10.0, 10.0), WINDOW, None));
        let t1 = t0 + Duration::from_millis(100);
        assert!(dc.press(t1, at(10.0, 10.0), WINDOW, None));
    }

    #[test]
    fn a_slow_second_press_is_two_single_clicks() {
        let mut dc = DoubleClick::default();
        let t0 = Instant::now();
        assert!(!dc.press(t0, at(10.0, 10.0), WINDOW, None));
        let t1 = t0 + Duration::from_millis(400);
        assert!(!dc.press(t1, at(10.0, 10.0), WINDOW, None));
    }

    #[test]
    fn distance_gate_rejects_a_pair_that_drifted() {
        let mut dc = DoubleClick::default();
        let t0 = Instant::now();
        assert!(!dc.press(t0, at(10.0, 10.0), WINDOW, Some(8.0)));
        let t1 = t0 + Duration::from_millis(100);
        // 20px apart on x exceeds the 8px gate.
        assert!(!dc.press(t1, at(30.0, 10.0), WINDOW, Some(8.0)));
    }

    #[test]
    fn no_distance_gate_ignores_how_far_the_clicks_drifted() {
        let mut dc = DoubleClick::default();
        let t0 = Instant::now();
        assert!(!dc.press(t0, at(10.0, 10.0), WINDOW, None));
        let t1 = t0 + Duration::from_millis(100);
        assert!(dc.press(t1, at(300.0, 300.0), WINDOW, None));
    }

    #[test]
    fn clearing_stops_a_third_click_from_chaining() {
        let mut dc = DoubleClick::default();
        let t0 = Instant::now();
        assert!(!dc.press(t0, at(10.0, 10.0), WINDOW, None));
        let t1 = t0 + Duration::from_millis(50);
        assert!(dc.press(t1, at(10.0, 10.0), WINDOW, None));
        dc.clear();
        // Without the pending press, the third click starts over.
        let t2 = t1 + Duration::from_millis(50);
        assert!(!dc.press(t2, at(10.0, 10.0), WINDOW, None));
    }

    #[test]
    fn without_clearing_a_third_click_chains() {
        let mut dc = DoubleClick::default();
        let t0 = Instant::now();
        assert!(!dc.press(t0, at(10.0, 10.0), WINDOW, None));
        let t1 = t0 + Duration::from_millis(50);
        assert!(dc.press(t1, at(10.0, 10.0), WINDOW, None));
        // The double-click is kept on record, so a third rapid press
        // pairs with it (matches the piano-roll add-note gesture).
        let t2 = t1 + Duration::from_millis(50);
        assert!(dc.press(t2, at(10.0, 10.0), WINDOW, None));
    }
}
