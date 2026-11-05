//! Tests for where a lane's backdrop is tinted under the loop.

use super::*;

/// 20 px per beat from beat 0.
fn geometry() -> TimelineGeometry {
    TimelineGeometry::new(20.0, 0.0)
}

#[test]
fn the_loop_spans_whole_pixels_inside_the_lane() {
    assert_eq!(
        LoopSpan::across(geometry(), 400.0, (4.02, 30.0)),
        Some(LoopSpan {
            start: 80.0,
            end: 400.0
        })
    );
    assert_eq!(LoopSpan::across(geometry(), 400.0, (24.0, 30.0)), None);
    assert_eq!(LoopSpan::across(geometry(), 400.0, (8.0, 8.0)), None);
}

#[test]
fn a_backdrop_part_crossing_the_loop_is_cut_where_the_loop_starts_and_ends() {
    let span = Some(LoopSpan {
        start: 80.0,
        end: 160.0,
    });

    assert_eq!(
        parts(span, 40.0, 200.0),
        [
            (40.0, 80.0, false),
            (80.0, 160.0, true),
            (160.0, 200.0, false)
        ]
    );
    assert_eq!(
        parts(span, 100.0, 101.0),
        [
            (100.0, 100.0, false),
            (100.0, 101.0, true),
            (101.0, 101.0, false)
        ]
    );
    assert_eq!(
        parts(None, 40.0, 200.0),
        [
            (40.0, 200.0, false),
            (200.0, 200.0, true),
            (200.0, 200.0, false)
        ]
    );
}
