//! The stretch of time a clip editor shows, framed on its opening span
//! until the producer zooms, scrolls or frames the loop.

use iced::mouse::ScrollDelta;
use iced::Point;

use crate::timeline_geometry::{
    wheel_delta_pixels, zoom_factor_from_pixels, TimelineGeometry, TimelineViewport,
    FRAME_MARGIN_PIXELS,
};

/// What the producer last asked one clip editor to show.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum StoredClipView {
    /// A zoom and scroll of their own.
    Viewport(TimelineViewport),
    /// The loop, framed at whatever width the editor has (Ableton's Z).
    Loop,
}

/// One clip editor's horizontal view, resolved against the editor's width.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ClipEditorView {
    stored: Option<StoredClipView>,
    opening: (f64, f64),
    loop_span: (f64, f64),
    content_beats: f64,
}

/// What a wheel turn over a clip editor asks for.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum WheelIntent {
    Zoom(f32),
    /// Pixels to scroll later in time (negative scrolls earlier).
    ScrollTime(f32),
    /// A plain vertical turn, which each editor reads its own way.
    Plain(f32),
}

/// The Arrange timeline's wheel, so one gesture zooms everywhere: Shift
/// zooms and a sideways swipe scrolls in time.
pub(crate) fn wheel_intent(delta: ScrollDelta, shift: bool) -> WheelIntent {
    let (dx, dy) = wheel_delta_pixels(delta);
    if dx.abs() > dy.abs() {
        WheelIntent::ScrollTime(-dx)
    } else if shift && dy != 0.0 {
        WheelIntent::Zoom(zoom_factor_from_pixels(dy))
    } else {
        WheelIntent::Plain(dy)
    }
}

impl ClipEditorView {
    /// `stored` is the producer's view of this clip, if any; until then the
    /// editor frames `opening`, and [`StoredClipView::Loop`] frames
    /// `loop_span`. Scrolling reaches the later of `content_beats` and
    /// either span.
    pub(crate) fn new(
        stored: Option<StoredClipView>,
        opening: (f64, f64),
        loop_span: (f64, f64),
        content_beats: f64,
    ) -> Self {
        Self {
            stored,
            opening,
            loop_span,
            content_beats: content_beats.max(opening.1).max(loop_span.1),
        }
    }

    /// Opens on `loop_span` while the clip loops, otherwise on the clip
    /// from its start, and frames the same span for the loop.
    pub(crate) fn framing(
        stored: Option<StoredClipView>,
        loop_span: Option<(f64, f64)>,
        clip_beats: f64,
        content_beats: f64,
    ) -> Self {
        let shown = loop_span
            .filter(|(start, end)| end > start)
            .unwrap_or((0.0, clip_beats));
        Self::new(stored, shown, shown, content_beats)
    }

    /// `width` is the time area's width, excluding any fixed header.
    pub(crate) fn viewport(self, width: f32) -> TimelineViewport {
        let framed = |(start, end): (f64, f64)| {
            let mut viewport = TimelineViewport::default();
            viewport.frame(start, end, width);
            viewport
        };
        match self.stored {
            Some(StoredClipView::Viewport(viewport)) => viewport,
            Some(StoredClipView::Loop) => framed(self.loop_span),
            None => framed(self.opening),
        }
    }

    /// The framing to store when a marker drag begins: the frame follows
    /// the loop, so dragging the loop would otherwise move the editor
    /// under the pointer.
    pub(crate) fn pin(self, width: f32) -> Option<TimelineViewport> {
        (!matches!(self.stored, Some(StoredClipView::Viewport(_)))).then(|| self.viewport(width))
    }

    pub(crate) fn geometry(self, width: f32, origin_x: f32) -> TimelineGeometry {
        self.viewport(width).geometry().with_origin(origin_x)
    }

    /// `anchor_x` is measured from the start of the time area.
    pub(crate) fn zoomed(self, factor: f32, anchor_x: f32, width: f32) -> TimelineViewport {
        let mut viewport = self.viewport(width);
        viewport.zoom_around(factor, anchor_x, self.scroll_extent(viewport), width);
        viewport
    }

    pub(crate) fn scrolled(self, pixels: f32, width: f32) -> TimelineViewport {
        let mut viewport = self.viewport(width);
        let delta = viewport.geometry().beats_for_width(pixels);
        viewport.scroll_by(delta, self.scroll_extent(viewport), width);
        viewport
    }

    /// The framing margin stays reachable past the content end, so the
    /// first scroll from a loop framed at the end does not jump.
    fn scroll_extent(self, viewport: TimelineViewport) -> f64 {
        self.content_beats + viewport.geometry().beats_for_width(FRAME_MARGIN_PIXELS)
    }
}

/// A drag along a clip editor's ruler, zooming and panning as the Arrange
/// ruler does, held inside the editor's content.
#[derive(Debug, Clone, Copy)]
pub(crate) struct RulerZoom {
    view: ClipEditorView,
    start: TimelineViewport,
    grabbed_beat: f64,
    pressed_y: f32,
}

impl RulerZoom {
    /// `at` is measured from the start of the time area.
    pub(crate) fn begin(view: ClipEditorView, at: Point, width: f32) -> Self {
        let start = view.viewport(width);
        Self {
            view,
            start,
            grabbed_beat: start.geometry().x_to_beat(at.x),
            pressed_y: at.y,
        }
    }

    pub(crate) fn drag_to(self, to: Point, width: f32) -> TimelineViewport {
        let mut viewport = self
            .start
            .ruler_dragged(self.grabbed_beat, to.x, to.y - self.pressed_y);
        viewport.clamp(self.view.scroll_extent(viewport), width);
        viewport
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_clip_editor_ruler_drag_zooms_and_pans_as_the_arrange_ruler_does() {
        let view = ClipEditorView::new(None, (0.0, 16.0), (0.0, 16.0), 64.0);
        let width = 600.0;
        let start = view.viewport(width);
        let pressed = Point::new(300.0, 10.0);
        let grabbed_beat = start.geometry().x_to_beat(pressed.x);

        let zoom = RulerZoom::begin(view, pressed, width);
        zoom.drag_to(Point::new(320.0, 40.0), width);
        let reached = zoom.drag_to(Point::new(340.0, 70.0), width);

        assert_eq!(reached, start.ruler_dragged(grabbed_beat, 340.0, 60.0));
    }
}
