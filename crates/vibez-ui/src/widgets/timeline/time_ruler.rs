//! The minutes and seconds scale along the bottom of the Arrange lanes.

use crate::timeline_geometry::TimelineGeometry;

/// Labels across `width` pixels as whole-pixel x and "m:ss" text.
fn time_marks(geometry: TimelineGeometry, width: f32, bpm: f64) -> Vec<(f32, String)> {
    let _ = (geometry, width, bpm);
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seconds_are_labelled_at_least_sixty_four_pixels_apart() {
        // 120 BPM at 20 px per beat is 40 px per second.
        let marks = time_marks(TimelineGeometry::new(20.0, 0.0), 170.0, 120.0);

        assert_eq!(
            marks,
            vec![
                (0.0, "0:00".to_string()),
                (80.0, "0:02".to_string()),
                (160.0, "0:04".to_string())
            ]
        );
    }

    #[test]
    fn labels_read_minutes_and_seconds_on_whole_pixels_when_scrolled() {
        // 128.3 beats in at 120 BPM is just past 1:04; labels step by 2 s.
        let marks = time_marks(TimelineGeometry::new(20.0, 128.3), 100.0, 120.0);

        assert_eq!(marks, vec![(74.0, "1:06".to_string())]);
    }

    #[test]
    fn zoomed_out_labels_step_by_minutes() {
        // One pixel per second labels every two minutes.
        let marks = time_marks(TimelineGeometry::new(0.5, 0.0), 300.0, 120.0);

        assert_eq!(
            marks,
            vec![
                (0.0, "0:00".to_string()),
                (120.0, "2:00".to_string()),
                (240.0, "4:00".to_string())
            ]
        );
    }
}
