//! Where the beat ruler's labels and ticks fall at a zoom.

use vibez_core::time::BEATS_PER_BAR;

/// Beats between ruler labels, about 60 px apart at any zoom.
pub(super) fn label_step_beats(pixels_per_beat: f32) -> f64 {
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
