//! The stretch of time a clip editor shows, framed on the loop until the
//! producer zooms or scrolls it.

use iced::mouse::ScrollDelta;
use iced::Point;

use crate::timeline_geometry::{
    wheel_delta_pixels, zoom_factor_from_pixels, TimelineGeometry, TimelineViewport,
    FRAME_MARGIN_PIXELS,
};

/// A ruler drag covers the zoom range in a short throw, as Ableton's does;
/// the wheel's rate would need the drag to cross the whole screen.
const RULER_ZOOM_GAIN: f32 = 3.0;

/// One clip editor's horizontal view, resolved against the editor's width.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ClipEditorView {
    stored: Option<TimelineViewport>,
    framed_start: f64,
    framed_end: f64,
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
    /// editor frames `framed_start..framed_end`. Scrolling reaches the later
    /// of `content_beats` and the framed span.
    pub(crate) fn new(
        stored: Option<TimelineViewport>,
        framed_start: f64,
        framed_end: f64,
        content_beats: f64,
    ) -> Self {
        Self {
            stored,
            framed_start,
            framed_end,
            content_beats: content_beats.max(framed_end),
        }
    }

    /// Frames `loop_span` while the clip loops, otherwise the clip from its
    /// start.
    pub(crate) fn framing(
        stored: Option<TimelineViewport>,
        loop_span: Option<(f64, f64)>,
        clip_beats: f64,
        content_beats: f64,
    ) -> Self {
        match loop_span.filter(|(start, end)| end > start) {
            Some((start, end)) => Self::new(stored, start, end, content_beats),
            None => Self::new(stored, 0.0, clip_beats, content_beats),
        }
    }

    /// `width` is the time area's width, excluding any fixed header.
    pub(crate) fn viewport(self, width: f32) -> TimelineViewport {
        self.stored.unwrap_or_else(|| {
            let mut viewport = TimelineViewport::default();
            viewport.frame(self.framed_start, self.framed_end, width);
            viewport
        })
    }

    fn showing(self, viewport: TimelineViewport) -> Self {
        Self {
            stored: Some(viewport),
            ..self
        }
    }

    /// The framing to store when a marker drag begins: the frame follows
    /// the loop, so dragging the loop would otherwise move the editor
    /// under the pointer.
    pub(crate) fn pin(self, width: f32) -> Option<TimelineViewport> {
        self.stored.is_none().then(|| self.viewport(width))
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

/// A drag along a clip editor's ruler: down zooms in around the press,
/// sideways scrolls with the content following the pointer.
#[derive(Debug, Clone, Copy)]
pub(crate) struct RulerZoom {
    view: ClipEditorView,
    anchor_x: f32,
    last: Point,
}

impl RulerZoom {
    /// `at` is measured from the start of the time area.
    pub(crate) fn begin(view: ClipEditorView, at: Point, width: f32) -> Self {
        Self {
            view: view.showing(view.viewport(width)),
            anchor_x: at.x,
            last: at,
        }
    }

    /// Each step builds on the last view this drag produced, not on the
    /// widget's, which can lag a frame behind the pointer.
    pub(crate) fn drag_to(&mut self, to: Point, width: f32) -> TimelineViewport {
        let factor = zoom_factor_from_pixels((to.y - self.last.y) * RULER_ZOOM_GAIN);
        let zoomed = self.view.zoomed(factor, self.anchor_x, width);
        let scrolled = self
            .view
            .showing(zoomed)
            .scrolled(self.last.x - to.x, width);
        self.view = self.view.showing(scrolled);
        self.last = to;
        scrolled
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_clip_editor_ruler_drag_zooms_and_pans_as_the_arrange_ruler_does() {
        let view = ClipEditorView::new(None, 0.0, 16.0, 64.0);
        let width = 600.0;
        let start = view.viewport(width);
        let pressed = Point::new(300.0, 10.0);
        let grabbed_beat = start.geometry().x_to_beat(pressed.x);

        let mut zoom = RulerZoom::begin(view, pressed, width);
        zoom.drag_to(Point::new(320.0, 40.0), width);
        let reached = zoom.drag_to(Point::new(340.0, 70.0), width);

        assert_eq!(reached, start.ruler_dragged(grabbed_beat, 340.0, 60.0));
    }
}
