//! Where the shell's regions sit across the window, so views and pointer
//! gestures measure the workspace from one place.

/// Space between neighbouring cards and between a card and the window's
/// edge. The gap between two cards is also the handle that resizes them.
pub const PANE_GAP: f32 = 8.0;

/// Height of the status bar along the bottom of the window.
pub const STATUS_HEIGHT: f32 = 26.0;

/// A region's left edge and width, in window pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Span {
    pub left: f32,
    pub width: f32,
}

/// The workspace's span in a window `window_width` wide, beside a Browser
/// `browser_width` wide when one is shown.
pub fn workspace_span(window_width: f32, browser_width: Option<f32>) -> Span {
    let left = PANE_GAP + browser_width.map_or(0.0, |width| width + PANE_GAP);
    Span {
        left,
        width: (window_width - left - PANE_GAP).max(0.0),
    }
}

/// The Browser width a drag on its gap asks for with the pointer at
/// `cursor_x`.
pub fn browser_width_at(cursor_x: f32) -> f32 {
    cursor_x - PANE_GAP - PANE_GAP / 2.0
}

/// The detail panel height a drag on its gap asks for with the pointer at
/// `cursor_y` in a window `window_height` tall.
pub fn detail_height_at(cursor_y: f32, window_height: f32) -> f32 {
    let detail_bottom = window_height - STATUS_HEIGHT - PANE_GAP;
    detail_bottom - (cursor_y + PANE_GAP / 2.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_workspace_card_sits_one_gap_from_the_browser_card_and_the_window_edge() {
        assert_eq!(
            workspace_span(1440.0, Some(430.0)),
            Span {
                left: 446.0,
                width: 986.0
            }
        );
        assert_eq!(
            workspace_span(1440.0, None),
            Span {
                left: 8.0,
                width: 1424.0
            }
        );
    }

    #[test]
    fn dragging_the_middle_of_a_gap_keeps_the_card_edge_under_the_pointer() {
        assert_eq!(browser_width_at(442.0), 430.0);
        // A detail card from 566 down to the gap above the 26 px status bar.
        assert_eq!(detail_height_at(562.0, 900.0), 300.0);
    }
}
