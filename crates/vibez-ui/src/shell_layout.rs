//! Where the shell's regions sit across the window, so views and pointer
//! gestures measure the workspace from one place.

/// Space between neighbouring panes, which is also the handle that resizes
/// them.
pub const PANE_GAP: f32 = 7.0;

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
