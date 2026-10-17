//! How a device body's height divides between its plots, its knob row and
//! Advanced, for cards built from those three bands.

/// Knobs with their names and values.
pub const KNOB_ROW_H: f32 = 74.0;
/// Names, values and small knobs on a single line.
pub const LINE_H: f32 = 24.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Stack {
    /// Height of the plots, zero when there is no room for them.
    pub plots: f32,
    pub knob_row: f32,
    pub advanced: Option<f32>,
    /// Advanced is open and only it fits, so it takes the whole body.
    pub advanced_only: bool,
}

/// Splits `below`, the height under the preset row, giving the knobs their
/// row first, then Advanced (`advanced_h` tall when open) and the plots
/// whatever is left when that reaches `plot_min`. A divider separates each
/// band. Short of room, the plots go first, then the knobs fold onto a line,
/// and only then does Advanced take the whole body.
pub fn stack(below: f32, advanced_h: Option<f32>, plot_min: f32) -> Stack {
    let stacked = advanced_h.map_or(0.0, |height| 1.0 + height);
    let knob_row = if below >= KNOB_ROW_H + stacked {
        KNOB_ROW_H
    } else {
        LINE_H
    };
    if advanced_h.is_some() && below < knob_row + stacked {
        return Stack {
            plots: 0.0,
            knob_row: 0.0,
            advanced: Some(below),
            advanced_only: true,
        };
    }
    let rest = below - knob_row - stacked;
    Stack {
        plots: if rest >= plot_min + 1.0 {
            rest - 1.0
        } else {
            0.0
        },
        knob_row,
        advanced: advanced_h,
        advanced_only: false,
    }
}

/// Splits controls `widths` wide into lines no wider than `width`, `gap`
/// apart.
pub fn pack(widths: &[f32], width: f32, gap: f32) -> Vec<std::ops::Range<usize>> {
    let mut lines = Vec::new();
    let (mut start, mut used) = (0, 0.0);
    for (index, item) in widths.iter().enumerate() {
        let gap = if index > start { gap } else { 0.0 };
        if index > start && used + gap + item > width {
            lines.push(start..index);
            start = index;
            used = *item;
        } else {
            used += gap + item;
        }
    }
    if start < widths.len() {
        lines.push(start..widths.len());
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLOT_MIN: f32 = 72.0;

    #[test]
    fn opening_advanced_gives_up_the_plots_before_the_knobs() {
        // Room for the knob row and Advanced, not for the plots as well.
        let below = KNOB_ROW_H + 1.0 + KNOB_ROW_H + 40.0;
        let open = stack(below, Some(KNOB_ROW_H), PLOT_MIN);
        assert!(!open.advanced_only, "{open:?}");
        assert_eq!(open.plots, 0.0);
        assert_eq!(open.knob_row, KNOB_ROW_H);
        assert_eq!(open.advanced, Some(KNOB_ROW_H));
    }

    #[test]
    fn opening_advanced_puts_the_knobs_on_a_line_before_hiding_them() {
        let below = LINE_H + 1.0 + KNOB_ROW_H + 10.0;
        let open = stack(below, Some(KNOB_ROW_H), PLOT_MIN);
        assert!(!open.advanced_only, "{open:?}");
        assert_eq!(open.knob_row, LINE_H);
        assert_eq!(open.advanced, Some(KNOB_ROW_H));
    }

    #[test]
    fn advanced_takes_the_body_only_when_nothing_else_fits() {
        let open = stack(LINE_H + KNOB_ROW_H, Some(KNOB_ROW_H), PLOT_MIN);
        assert!(open.advanced_only, "{open:?}");
        assert_eq!(open.advanced, Some(LINE_H + KNOB_ROW_H));
    }

    #[test]
    fn closed_cards_keep_their_plots_above_the_knobs() {
        let closed = stack(200.0, None, PLOT_MIN);
        assert_eq!(closed.knob_row, KNOB_ROW_H);
        assert_eq!(closed.plots, 200.0 - KNOB_ROW_H - 1.0);
        assert_eq!(stack(40.0, None, PLOT_MIN).knob_row, LINE_H);
    }

    #[test]
    fn lines_break_where_the_next_control_would_not_fit() {
        assert_eq!(pack(&[100.0, 100.0, 100.0], 210.0, 10.0), vec![0..2, 2..3]);
        assert_eq!(pack(&[100.0, 100.0], 210.0, 10.0), vec![0..2]);
        // A control wider than the line still gets a line to itself.
        assert_eq!(pack(&[300.0, 50.0], 210.0, 10.0), vec![0..1, 1..2]);
    }
}
