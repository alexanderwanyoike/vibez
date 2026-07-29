//! Rubber-band geometry for the arrangement.
//!
//! Each track lane is its own canvas, so the lane that starts a drag has
//! to resolve the box against the whole column on everyone's behalf. That
//! only works if lanes know their real vertical placement: the cross-track
//! clip drag next door assumes a uniform 70px row and silently mis-targets
//! once automation lanes are expanded. These spans carry the true layout.

use vibez_core::id::TrackId;

/// Height of one track's clip lane in the arrangement column.
pub const TRACK_ROW_HEIGHT: f32 = 70.0;

/// Vertical placement of one track row within the arrangement column,
/// measured from the top of the first row and including any expanded
/// automation lanes beneath the clip lane.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrackRowSpan {
    pub track_id: TrackId,
    /// Offset of the row's top edge from the top of the column.
    pub top: f32,
    /// Full row height, clip lane plus expanded automation lanes.
    pub height: f32,
    /// Height of the clip lane alone, where clips are actually drawn.
    pub lane_height: f32,
}

impl TrackRowSpan {
    pub fn bottom(&self) -> f32 {
        self.top + self.height
    }
}

/// A rubber-band rectangle resolved against the column layout.
#[derive(Debug, Clone, PartialEq)]
pub struct MarqueeSpan {
    pub start_beats: f64,
    pub end_beats: f64,
    /// Box edges in column coordinates.
    pub top_y: f32,
    pub bottom_y: f32,
    /// Every track the box vertically intersects.
    pub track_ids: Vec<TrackId>,
}

/// Resolve a drag into the beat span and the lanes it covers.
///
/// `anchor_y` and `current_y` are in column coordinates. A zero-height box
/// still catches the lane it sits in, so a purely horizontal drag behaves
/// like the single-lane region select it replaces.
pub fn resolve(
    anchor_beat: f64,
    current_beat: f64,
    anchor_y: f32,
    current_y: f32,
    rows: &[TrackRowSpan],
) -> MarqueeSpan {
    let top_y = anchor_y.min(current_y);
    let bottom_y = anchor_y.max(current_y);

    let track_ids = rows
        .iter()
        .filter(|row| row.top <= bottom_y && row.bottom() >= top_y)
        .map(|row| row.track_id)
        .collect();

    MarqueeSpan {
        start_beats: anchor_beat.min(current_beat).max(0.0),
        end_beats: anchor_beat.max(current_beat).max(0.0),
        top_y,
        bottom_y,
        track_ids,
    }
}

/// The vertical slice of a marquee that falls inside one lane, in
/// lane-local coordinates. `None` when the box misses this lane.
///
/// Clipped to the clip lane rather than the full row: drawing a box over
/// an expanded automation lane would imply the automation points are
/// selected too, which they are not.
pub fn slice_for_lane(
    top_y: f32,
    bottom_y: f32,
    row_top: f32,
    lane_height: f32,
) -> Option<(f32, f32)> {
    let local_top = (top_y - row_top).max(0.0);
    let local_bottom = (bottom_y - row_top).min(lane_height);
    (local_bottom > local_top).then_some((local_top, local_bottom))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows() -> Vec<TrackRowSpan> {
        // Track 1 has an automation lane open, so rows are not uniform.
        let ids: Vec<TrackId> = (0..3).map(|_| TrackId::new()).collect();
        vec![
            TrackRowSpan {
                track_id: ids[0],
                top: 0.0,
                height: 70.0,
                lane_height: 70.0,
            },
            TrackRowSpan {
                track_id: ids[1],
                top: 70.0,
                height: 126.0,
                lane_height: 70.0,
            },
            TrackRowSpan {
                track_id: ids[2],
                top: 196.0,
                height: 70.0,
                lane_height: 70.0,
            },
        ]
    }

    #[test]
    fn a_horizontal_drag_still_catches_its_own_lane() {
        let rows = rows();
        let span = resolve(4.0, 1.0, 30.0, 30.0, &rows);

        assert_eq!(span.track_ids, vec![rows[0].track_id]);
        // Dragging leftwards must not produce an inverted range.
        assert_eq!(span.start_beats, 1.0);
        assert_eq!(span.end_beats, 4.0);
    }

    #[test]
    fn a_box_spans_every_row_it_touches_despite_uneven_heights() {
        let rows = rows();
        // From track 0's lane down into track 2's lane. A uniform-70px
        // assumption would stop short at track 1 because of the
        // automation lane inflating that row.
        let span = resolve(0.0, 8.0, 10.0, 200.0, &rows);

        assert_eq!(
            span.track_ids,
            vec![rows[0].track_id, rows[1].track_id, rows[2].track_id]
        );
        assert_eq!(span.top_y, 10.0);
        assert_eq!(span.bottom_y, 200.0);
    }

    #[test]
    fn a_box_below_a_lane_does_not_select_it() {
        let rows = rows();
        let span = resolve(0.0, 8.0, 200.0, 240.0, &rows);

        assert_eq!(span.track_ids, vec![rows[2].track_id]);
    }

    #[test]
    fn lane_slices_clip_to_the_clip_lane_not_the_automation_area() {
        // Box covers all of track 1's row, automation lanes included.
        assert_eq!(slice_for_lane(70.0, 196.0, 70.0, 70.0), Some((0.0, 70.0)));
        // Box sitting purely in the automation area draws nothing.
        assert_eq!(slice_for_lane(150.0, 190.0, 70.0, 70.0), None);
        // Partial overlap from above.
        assert_eq!(slice_for_lane(50.0, 90.0, 70.0, 70.0), Some((0.0, 20.0)));
    }
}
