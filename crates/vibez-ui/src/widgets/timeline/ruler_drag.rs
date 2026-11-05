//! Where a drag in the ruler's loop strip puts the Arrange loop.

use crate::state::GridConfig;
use crate::widgets::span_edges::{nearest_edge, span_with_end, span_with_start, SpanEdge};

/// Which part of the loop brace a press in the loop strip took hold of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BraceGrab {
    Start,
    End,
    Whole,
    /// Outside the brace: the drag draws a new loop.
    New,
}

/// Pixels either side of a brace end that still grab it.
const END_GRAB_PX: f32 = 6.0;

/// What a press at `x` grabs of a brace drawn from `start_x` to `end_x`.
pub(super) fn brace_grab(x: f32, start_x: f32, end_x: f32) -> BraceGrab {
    if end_x <= start_x {
        return BraceGrab::New;
    }
    match nearest_edge(start_x, end_x, x, END_GRAB_PX) {
        Some(SpanEdge::Start) => BraceGrab::Start,
        Some(SpanEdge::End) => BraceGrab::End,
        None if x > start_x && x < end_x => BraceGrab::Whole,
        None => BraceGrab::New,
    }
}

/// A loop strip drag from press to release.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct BraceDrag {
    pub grab: BraceGrab,
    /// The loop when the press landed, as `(start, end)` beats.
    pub original: (f64, f64),
    pub pressed_beat: f64,
}

impl BraceDrag {
    /// The loop with the pointer at `beat`, snapped to the grid at
    /// `pixels_per_beat`; `None` while a new loop is still too short to
    /// draw.
    pub fn loop_at(self, beat: f64, grid: GridConfig, pixels_per_beat: f32) -> Option<(f64, f64)> {
        let step = grid.effective_grid(pixels_per_beat).beat_size();
        let snap = |beat: f64| grid.snap_beat(beat, pixels_per_beat);
        let (start, end) = self.original;
        match self.grab {
            BraceGrab::Start => Some(span_with_start(snap(beat), end, step)),
            BraceGrab::End => Some(span_with_end(snap(beat), start, step, f64::INFINITY)),
            BraceGrab::Whole => {
                let shift = snap(beat - self.pressed_beat).max(-start);
                Some((start + shift, end + shift))
            }
            BraceGrab::New => {
                let anchor = snap(self.pressed_beat);
                let current = snap(beat);
                ((current - anchor).abs() >= step / 2.0)
                    .then(|| (anchor.min(current).max(0.0), anchor.max(current)))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::SnapGrid;

    /// One beat grid at 20 px per beat.
    const PPB: f32 = 20.0;

    fn grid() -> GridConfig {
        GridConfig::new(SnapGrid::QUARTER, true, false, 0)
    }

    fn drag(grab: BraceGrab, pressed_beat: f64) -> BraceDrag {
        BraceDrag {
            grab,
            original: (16.0, 32.0),
            pressed_beat,
        }
    }

    #[test]
    fn a_press_near_an_end_grabs_that_end_and_inside_grabs_the_whole_brace() {
        assert_eq!(brace_grab(103.0, 100.0, 300.0), BraceGrab::Start);
        assert_eq!(brace_grab(296.0, 100.0, 300.0), BraceGrab::End);
        assert_eq!(brace_grab(200.0, 100.0, 300.0), BraceGrab::Whole);
        assert_eq!(brace_grab(40.0, 100.0, 300.0), BraceGrab::New);
        assert_eq!(brace_grab(340.0, 100.0, 300.0), BraceGrab::New);
    }

    #[test]
    fn a_short_brace_hands_each_end_the_half_nearest_it() {
        assert_eq!(brace_grab(103.0, 100.0, 108.0), BraceGrab::Start);
        assert_eq!(brace_grab(105.0, 100.0, 108.0), BraceGrab::End);
    }

    #[test]
    fn an_empty_loop_is_drawn_anew() {
        assert_eq!(brace_grab(100.0, 100.0, 100.0), BraceGrab::New);
    }

    #[test]
    fn dragging_an_end_snaps_it_and_never_crosses_the_other() {
        let start = drag(BraceGrab::Start, 16.0);
        assert_eq!(start.loop_at(11.6, grid(), PPB), Some((12.0, 32.0)));
        assert_eq!(start.loop_at(40.0, grid(), PPB), Some((31.0, 32.0)));
        assert_eq!(start.loop_at(-5.0, grid(), PPB), Some((0.0, 32.0)));

        let end = drag(BraceGrab::End, 32.0);
        assert_eq!(end.loop_at(35.3, grid(), PPB), Some((16.0, 35.0)));
        assert_eq!(end.loop_at(2.0, grid(), PPB), Some((16.0, 17.0)));
    }

    #[test]
    fn dragging_the_middle_moves_the_whole_loop_by_whole_grid_steps() {
        let whole = drag(BraceGrab::Whole, 20.3);

        assert_eq!(whole.loop_at(24.1, grid(), PPB), Some((20.0, 36.0)));
        assert_eq!(whole.loop_at(-30.0, grid(), PPB), Some((0.0, 16.0)));
    }

    #[test]
    fn dragging_outside_the_brace_draws_a_new_snapped_loop_either_way() {
        let new = drag(BraceGrab::New, 40.2);

        assert_eq!(new.loop_at(40.4, grid(), PPB), None);
        assert_eq!(new.loop_at(43.8, grid(), PPB), Some((40.0, 44.0)));
        assert_eq!(new.loop_at(37.1, grid(), PPB), Some((37.0, 40.0)));
    }
}
