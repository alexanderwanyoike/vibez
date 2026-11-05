//! Where the beat ruler's labels and ticks fall at a zoom.

use crate::timeline_geometry::TimelineGeometry;
use vibez_core::time::BEATS_PER_BAR;

/// One labelled or plain tick on the ruler, at a whole-pixel x.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum RulerMark {
    /// A full-height tick at the label step; bar labels read stronger.
    Label {
        x: f32,
        text: String,
        bar: bool,
    },
    /// A taller short tick halfway between labels.
    Mid {
        x: f32,
    },
    Minor {
        x: f32,
    },
}

/// Ticks any closer blur into a solid strip.
const MIN_TICK_GAP_PX: f32 = 7.0;

/// Labels and ticks across `width` pixels.
pub(super) fn ruler_marks(geometry: TimelineGeometry, width: f32) -> Vec<RulerMark> {
    let ppb = geometry.pixels_per_beat();
    let label_beats = label_step_beats(ppb);
    let tick_beats = [label_beats / 4.0, label_beats / 2.0]
        .into_iter()
        .find(|step| geometry.width_for_beats(*step) >= MIN_TICK_GAP_PX);
    let step = tick_beats.unwrap_or(label_beats);

    let mut marks = Vec::new();
    for beat in geometry.grid_beats(step, 0.0, width) {
        let x = geometry.beat_to_x(beat).round();
        if !(0.0..=width).contains(&x) {
            continue;
        }
        marks.push(if is_multiple(beat, label_beats) {
            RulerMark::Label {
                x,
                text: label_text(beat, label_beats),
                bar: is_multiple(beat, BEATS_PER_BAR),
            }
        } else if step < label_beats / 2.0 && is_multiple(beat, label_beats / 2.0) {
            RulerMark::Mid { x }
        } else {
            RulerMark::Minor { x }
        });
    }
    marks
}

/// "33" on a bar, "33.2" on a beat, "33.2.3" on a sixteenth.
fn label_text(beat: f64, label_beats: f64) -> String {
    let bar = (beat / BEATS_PER_BAR + 1e-9).floor();
    let in_bar = beat - bar * BEATS_PER_BAR;
    let beat_in_bar = (in_bar + 1e-9).floor();
    let sixteenth = ((in_bar - beat_in_bar) * 4.0).round();
    if label_beats >= BEATS_PER_BAR || in_bar < 1e-6 {
        format!("{}", bar + 1.0)
    } else if label_beats >= 1.0 || sixteenth == 0.0 {
        format!("{}.{}", bar + 1.0, beat_in_bar + 1.0)
    } else {
        format!("{}.{}.{}", bar + 1.0, beat_in_bar + 1.0, sixteenth + 1.0)
    }
}

pub(super) fn is_multiple(beat: f64, of: f64) -> bool {
    let ratio = beat / of;
    (ratio - ratio.round()).abs() < 1e-6
}

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

    fn labels(marks: &[RulerMark]) -> Vec<(f32, &str, bool)> {
        marks
            .iter()
            .filter_map(|mark| match mark {
                RulerMark::Label { x, text, bar } => Some((*x, text.as_str(), *bar)),
                _ => None,
            })
            .collect()
    }

    fn ticks(marks: &[RulerMark]) -> (Vec<f32>, Vec<f32>) {
        let mids = marks
            .iter()
            .filter_map(|mark| match mark {
                RulerMark::Mid { x } => Some(*x),
                _ => None,
            })
            .collect();
        let minors = marks
            .iter()
            .filter_map(|mark| match mark {
                RulerMark::Minor { x } => Some(*x),
                _ => None,
            })
            .collect();
        (mids, minors)
    }

    #[test]
    fn bar_numbers_sit_on_whole_pixels_at_the_label_step() {
        // 40 px bars label every second bar.
        let marks = ruler_marks(TimelineGeometry::new(10.0, 0.37), 250.0);

        assert_eq!(
            labels(&marks),
            vec![(76.0, "3", true), (156.0, "5", true), (236.0, "7", true)]
        );
    }

    #[test]
    fn beat_labels_read_bar_dot_beat_and_bar_labels_stay_plain() {
        let marks = ruler_marks(TimelineGeometry::new(50.0, 128.0), 210.0);

        assert_eq!(
            labels(&marks),
            vec![
                (0.0, "33", true),
                (50.0, "33.2", false),
                (100.0, "33.3", false),
                (150.0, "33.4", false),
                (200.0, "34", true)
            ]
        );
    }

    #[test]
    fn sixteenth_labels_read_bar_beat_sixteenth() {
        let marks = ruler_marks(TimelineGeometry::new(240.0, 0.0), 130.0);

        assert_eq!(
            labels(&marks),
            vec![
                (0.0, "1", true),
                (60.0, "1.1.2", false),
                (120.0, "1.1.3", false)
            ]
        );
    }

    #[test]
    fn quarter_label_steps_get_ticks_and_the_half_step_tick_is_taller() {
        // 60 px bars label every bar; beats are 15 px apart.
        let marks = ruler_marks(TimelineGeometry::new(15.0, 0.0), 120.0);

        assert_eq!(
            ticks(&marks),
            (vec![30.0, 90.0], vec![15.0, 45.0, 75.0, 105.0])
        );
    }

    #[test]
    fn ticks_halve_and_then_vanish_when_closer_than_seven_pixels() {
        // Labels every 128 beats: quarters are 6.4 px apart, halves 12.8.
        let halves = ruler_marks(TimelineGeometry::new(0.2, 0.0), 60.0);
        assert_eq!(ticks(&halves), (vec![], vec![13.0, 38.0]));

        // Labels every 128 beats: halves are 6.4 px apart.
        let none = ruler_marks(TimelineGeometry::new(0.1, 0.0), 60.0);
        assert_eq!(ticks(&none), (vec![], vec![]));
    }
}
