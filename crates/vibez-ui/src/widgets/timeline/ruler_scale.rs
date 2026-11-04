//! Where the beat ruler's labels and ticks fall at a zoom.

use vibez_core::time::BEATS_PER_BAR;

/// Beats between ruler labels: bars about 60 px apart, then every beat
/// once a beat is 40 px wide, then every sixteenth once one is 60 px wide.
pub(super) fn label_step_beats(pixels_per_beat: f32) -> f64 {
    if pixels_per_beat * 0.25 >= 60.0 {
        return 0.25;
    }
    if pixels_per_beat >= 40.0 {
        return 1.0;
    }
    let pixels_per_bar = pixels_per_beat * BEATS_PER_BAR as f32;
    let bars = if pixels_per_bar >= 60.0 {
        1.0
    } else if pixels_per_bar >= 30.0 {
        2.0
    } else if pixels_per_bar >= 15.0 {
        4.0
    } else if pixels_per_bar >= 8.0 {
        8.0
    } else if pixels_per_bar >= 4.0 {
        16.0
    } else {
        32.0
    };
    bars * BEATS_PER_BAR
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_step_by_bars_until_a_beat_is_forty_pixels_wide() {
        assert_eq!(label_step_beats(14.0), 8.0);
        assert_eq!(label_step_beats(39.0), 4.0);
        assert_eq!(label_step_beats(40.0), 1.0);
    }

    #[test]
    fn sixteenths_are_labelled_once_one_is_sixty_pixels_wide() {
        assert_eq!(label_step_beats(239.0), 1.0);
        assert_eq!(label_step_beats(240.0), 0.25);
    }
}
