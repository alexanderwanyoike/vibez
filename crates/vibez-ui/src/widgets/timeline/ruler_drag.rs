//! Where a drag in the ruler's loop strip puts the Arrange loop.

use crate::timeline_snap::{SnapContext, SnapTargets};
use crate::widgets::clip_loop_markers::{marker_at, LoopMarker};
use crate::widgets::span_edges::{span_with_end, span_with_start};

/// Pixels either side of a brace end that still grab it.
const END_GRAB_PX: f32 = 6.0;

/// What a press at `x` grabs of a brace drawn from `start_x` to `end_x`;
/// `None` outside it, where the drag draws a new loop.
pub(super) fn brace_grab(x: f32, start_x: f32, end_x: f32) -> Option<LoopMarker> {
    if end_x <= start_x {
        return None;
    }
    marker_at(start_x, end_x, x, END_GRAB_PX)
}

/// A loop strip drag from press to release.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct BraceDrag {
    /// `None` draws a new loop.
    pub grab: Option<LoopMarker>,
    /// The loop when the press landed, as `(start, end)` beats.
    pub original: (f64, f64),
    pub pressed_beat: f64,
}

impl BraceDrag {
    /// The loop with the pointer at `beat`, snapped onto `targets`; `None`
    /// while a new loop is still too short to draw.
    pub fn loop_at(
        self,
        beat: f64,
        targets: &SnapTargets,
        context: SnapContext,
    ) -> Option<(f64, f64)> {
        let step = context
            .grid
            .effective_grid(context.pixels_per_beat)
            .beat_size();
        let snap = |beat: f64| targets.loop_edge(beat, context).beat;
        let (start, end) = self.original;
        match self.grab {
            Some(LoopMarker::Start) => Some(span_with_start(snap(beat), end, step)),
            Some(LoopMarker::End) => Some(span_with_end(snap(beat), start, step, f64::INFINITY)),
            Some(LoopMarker::Body) => {
                let moved = targets
                    .moved_loop(start, end - start, beat - self.pressed_beat, context)
                    .beat
                    .max(0.0);
                Some((moved, moved + end - start))
            }
            None => {
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
    use crate::state::{GridConfig, SnapGrid};

    /// A one beat grid at 20 px per beat.
    fn grid() -> SnapContext {
        SnapContext {
            grid: GridConfig::new(SnapGrid::QUARTER, true, false, 0),
            pixels_per_beat: 20.0,
            free: false,
        }
    }

    fn loop_at(drag: BraceDrag, beat: f64) -> Option<(f64, f64)> {
        drag.loop_at(beat, &SnapTargets::default(), grid())
    }

    fn drag(grab: Option<LoopMarker>, pressed_beat: f64) -> BraceDrag {
        BraceDrag {
            grab,
            original: (16.0, 32.0),
            pressed_beat,
        }
    }

    #[test]
    fn a_press_near_an_end_grabs_that_end_and_inside_grabs_the_whole_brace() {
        assert_eq!(brace_grab(103.0, 100.0, 300.0), Some(LoopMarker::Start));
        assert_eq!(brace_grab(296.0, 100.0, 300.0), Some(LoopMarker::End));
        assert_eq!(brace_grab(200.0, 100.0, 300.0), Some(LoopMarker::Body));
        assert_eq!(brace_grab(40.0, 100.0, 300.0), None);
        assert_eq!(brace_grab(340.0, 100.0, 300.0), None);
    }

    #[test]
    fn a_short_brace_hands_each_end_the_half_nearest_it() {
        assert_eq!(brace_grab(103.0, 100.0, 108.0), Some(LoopMarker::Start));
        assert_eq!(brace_grab(105.0, 100.0, 108.0), Some(LoopMarker::End));
    }

    #[test]
    fn an_empty_loop_is_drawn_anew() {
        assert_eq!(brace_grab(100.0, 100.0, 100.0), None);
    }

    #[test]
    fn dragging_an_end_snaps_it_and_never_crosses_the_other() {
        let start = drag(Some(LoopMarker::Start), 16.0);
        assert_eq!(loop_at(start, 11.6), Some((12.0, 32.0)));
        assert_eq!(loop_at(start, 40.0), Some((31.0, 32.0)));
        assert_eq!(loop_at(start, -5.0), Some((0.0, 32.0)));

        let end = drag(Some(LoopMarker::End), 32.0);
        assert_eq!(loop_at(end, 35.3), Some((16.0, 35.0)));
        assert_eq!(loop_at(end, 2.0), Some((16.0, 17.0)));
    }

    #[test]
    fn dragging_the_middle_moves_the_whole_loop_by_whole_grid_steps() {
        let whole = drag(Some(LoopMarker::Body), 20.3);

        assert_eq!(loop_at(whole, 24.1), Some((20.0, 36.0)));
        assert_eq!(loop_at(whole, -30.0), Some((0.0, 16.0)));
    }

    #[test]
    fn dragging_outside_the_brace_draws_a_new_snapped_loop_either_way() {
        let new = drag(None, 40.2);

        assert_eq!(loop_at(new, 40.4), None);
        assert_eq!(loop_at(new, 43.8), Some((40.0, 44.0)));
        assert_eq!(loop_at(new, 37.1), Some((37.0, 40.0)));
    }
}
