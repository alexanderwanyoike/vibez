//! Where the shell's regions sit across the window, so views and pointer
//! gestures measure the workspace from one place.

/// Space between neighbouring panes, which is also the handle that resizes
/// them.
pub const PANE_GAP: f32 = 7.0;

/// Height of the status bar along the bottom of the window.
pub const STATUS_HEIGHT: f32 = 24.0;

/// A region's left edge and width, in window pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Span {
    pub left: f32,
    pub width: f32,
}

/// The workspace's span in a window `window_width` wide, beside a Browser
/// `browser_width` wide when one is shown.
pub fn workspace_span(window_width: f32, browser_width: Option<f32>) -> Span {
    let left = browser_width.map_or(0.0, |width| width + PANE_GAP);
    Span {
        left,
        width: (window_width - left).max(0.0),
    }
}

/// The Browser width a drag on its gap asks for with the pointer at
/// `cursor_x`.
pub fn browser_width_at(cursor_x: f32) -> f32 {
    cursor_x
}

/// The detail panel height a drag on its gap asks for with the pointer at
/// `cursor_y` in a window `window_height` tall.
pub fn detail_height_at(cursor_y: f32, window_height: f32) -> f32 {
    window_height - cursor_y - STATUS_HEIGHT
}
