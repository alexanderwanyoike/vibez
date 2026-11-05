//! Which whole pixels of a timeline backdrop lie under the Arrange loop.

use crate::timeline_geometry::TimelineGeometry;

/// The whole-pixel stretch of a lane the loop covers.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct LoopSpan {
    pub start: f32,
    pub end: f32,
}

impl LoopSpan {
    /// Where the loop from `beats.0` to `beats.1` covers a lane `width`
    /// pixels wide, if it shows at all.
    pub fn across(geometry: TimelineGeometry, width: f32, beats: (f64, f64)) -> Option<Self> {
        if beats.1 <= beats.0 {
            return None;
        }
        let start = geometry.beat_to_x(beats.0).round().max(0.0);
        let end = geometry.beat_to_x(beats.1).round().min(width);
        (end > start).then_some(Self { start, end })
    }
}

/// `x0..x1` cut where the loop starts and ends: the part before it, the
/// part under it (marked `true`) and the part after it. Parts outside the
/// range are empty.
pub(super) fn parts(span: Option<LoopSpan>, x0: f32, x1: f32) -> [(f32, f32, bool); 3] {
    let (start, end) = span.map_or((x1, x1), |span| {
        (span.start.clamp(x0, x1), span.end.clamp(x0, x1))
    });
    [(x0, start, false), (start, end, true), (end, x1, false)]
}

#[cfg(test)]
#[path = "loop_tint_tests.rs"]
mod tests;
