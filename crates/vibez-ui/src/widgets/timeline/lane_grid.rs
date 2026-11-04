//! Where the lane grid's alternating bar bands and lines fall on screen.

use crate::timeline_geometry::TimelineGeometry;

/// How strongly a grid line is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum GridLine {
    /// On the ruler's label step.
    Label,
    Bar,
    Fine,
}

#[derive(Debug, Default, PartialEq)]
pub(super) struct LaneGrid {
    /// Whole-pixel `(start, end)` spans of the shaded bands.
    pub bands: Vec<(f32, f32)>,
    /// Whole-pixel x of each one pixel line.
    pub lines: Vec<(f32, GridLine)>,
}

const BEATS_PER_BAR: f64 = 4.0;

/// Lines any closer turn a zoomed-out lane into a barcode.
const MIN_LINE_GAP_PX: f32 = 14.0;

/// Bands and lines across `width` pixels for a grid of `step_beats`.
pub(super) fn lane_grid(geometry: TimelineGeometry, width: f32, step_beats: f64) -> LaneGrid {
    let ppb = geometry.pixels_per_beat();
    if ppb <= 0.0 || step_beats <= 0.0 {
        return LaneGrid::default();
    }
    let first_beat = geometry.x_to_beat(0.0);
    let last_beat = geometry.x_to_beat(width);
    let label_beats =
        f64::from(super::ruler::bar_label_step(ppb * BEATS_PER_BAR as f32)) * BEATS_PER_BAR;

    let mut bands = Vec::new();
    let first_band = (first_beat / label_beats).floor() as i64;
    let last_band = (last_beat / label_beats).ceil() as i64;
    for band in (first_band..last_band).filter(|band| band.rem_euclid(2) == 1) {
        let start = geometry
            .beat_to_x(band as f64 * label_beats)
            .round()
            .max(0.0);
        let end = geometry
            .beat_to_x((band + 1) as f64 * label_beats)
            .round()
            .min(width.floor());
        if end > start {
            bands.push((start, end));
        }
    }

    let bars_apart = ppb * BEATS_PER_BAR as f32 >= MIN_LINE_GAP_PX;
    let mut fine_beats = step_beats;
    while ppb * (fine_beats as f32) < MIN_LINE_GAP_PX {
        fine_beats *= 2.0;
    }
    let fine_apart = fine_beats < BEATS_PER_BAR;
    let mut lines = Vec::new();
    let first_step = (first_beat / step_beats).ceil() as i64;
    let last_step = (last_beat / step_beats).floor() as i64;
    for step in first_step..=last_step {
        let beat = step as f64 * step_beats;
        let line = if is_multiple(beat, label_beats) {
            GridLine::Label
        } else if is_multiple(beat, BEATS_PER_BAR) {
            GridLine::Bar
        } else {
            GridLine::Fine
        };
        let shown = match line {
            GridLine::Label => true,
            GridLine::Bar => bars_apart,
            GridLine::Fine => fine_apart && is_multiple(beat, fine_beats),
        };
        let x = geometry.beat_to_x(beat).round();
        if shown && (0.0..width).contains(&x) {
            lines.push((x, line));
        }
    }

    LaneGrid { bands, lines }
}

fn is_multiple(beat: f64, of: f64) -> bool {
    let ratio = beat / of;
    (ratio - ratio.round()).abs() < 1e-6
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines_of(grid: &LaneGrid, kind: GridLine) -> Vec<f32> {
        grid.lines
            .iter()
            .filter(|(_, line)| *line == kind)
            .map(|(x, _)| *x)
            .collect()
    }

    #[test]
    fn every_other_label_step_is_a_shaded_band() {
        // 40 px per bar labels every second bar.
        let grid = lane_grid(TimelineGeometry::new(10.0, 0.0), 400.0, 1.0);

        assert_eq!(grid.bands, vec![(80.0, 160.0), (240.0, 320.0)]);
    }

    #[test]
    fn a_band_is_never_narrower_than_a_bar() {
        // 400 px per bar labels every bar.
        let grid = lane_grid(TimelineGeometry::new(100.0, 0.0), 1000.0, 0.25);

        assert_eq!(grid.bands, vec![(400.0, 800.0)]);
    }

    #[test]
    fn bands_and_lines_land_on_whole_pixels_when_scrolled_between_them() {
        let grid = lane_grid(TimelineGeometry::new(14.3, 3.37), 900.0, 1.0);

        assert!(!grid.bands.is_empty() && !grid.lines.is_empty());
        for (start, end) in &grid.bands {
            assert_eq!((start.fract(), end.fract()), (0.0, 0.0));
        }
        for (x, _) in &grid.lines {
            assert_eq!(x.fract(), 0.0);
        }
    }

    #[test]
    fn lines_on_the_label_step_outrank_bar_lines_and_bar_lines_outrank_finer_ones() {
        // 56 px per bar labels every second bar; beats are 14 px apart.
        let grid = lane_grid(TimelineGeometry::new(14.0, 0.0), 450.0, 1.0);

        assert_eq!(
            lines_of(&grid, GridLine::Label),
            vec![0.0, 112.0, 224.0, 336.0, 448.0]
        );
        assert_eq!(
            lines_of(&grid, GridLine::Bar),
            vec![56.0, 168.0, 280.0, 392.0]
        );
        assert_eq!(lines_of(&grid, GridLine::Fine).len(), 32 - 8);
    }

    #[test]
    fn finer_lines_thin_out_until_fourteen_pixels_apart() {
        // Eighths are 5 px apart and beats 10 px, so every second beat.
        let grid = lane_grid(TimelineGeometry::new(10.0, 0.0), 400.0, 0.5);

        assert_eq!(
            lines_of(&grid, GridLine::Fine),
            (0..10)
                .map(|bar| bar as f32 * 40.0 + 20.0)
                .collect::<Vec<_>>()
        );
        assert_eq!(
            lines_of(&grid, GridLine::Bar),
            vec![40.0, 120.0, 200.0, 280.0, 360.0]
        );
    }

    #[test]
    fn no_finer_lines_when_even_bars_are_crowded() {
        let grid = lane_grid(TimelineGeometry::new(3.0, 0.0), 400.0, 0.25);

        assert!(lines_of(&grid, GridLine::Fine).is_empty());
    }

    #[test]
    fn crowded_bar_lines_give_way_to_the_label_step() {
        // 8 px per bar labels every eighth bar.
        let grid = lane_grid(TimelineGeometry::new(2.0, 0.0), 200.0, 4.0);

        assert!(lines_of(&grid, GridLine::Bar).is_empty());
        assert_eq!(
            lines_of(&grid, GridLine::Label),
            vec![0.0, 64.0, 128.0, 192.0]
        );
    }

    #[test]
    fn lines_follow_the_step_they_are_given() {
        let triplet_eighths = 2.0 / 3.0;
        let grid = lane_grid(TimelineGeometry::new(30.0, 0.0), 125.0, triplet_eighths);

        assert_eq!(
            lines_of(&grid, GridLine::Fine),
            vec![20.0, 40.0, 60.0, 80.0, 100.0]
        );
        assert_eq!(lines_of(&grid, GridLine::Label), vec![0.0, 120.0]);
    }
}
