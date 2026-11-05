//! Which edge of a span on a timeline a pointer grabs, and the span its drag
//! leaves.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SpanEdge {
    Start,
    End,
}

/// The edge of a span from `start_x` to `end_x` within `radius` of `x`, the
/// nearer one where both are.
pub(crate) fn nearest_edge(start_x: f32, end_x: f32, x: f32, radius: f32) -> Option<SpanEdge> {
    let start_distance = (x - start_x).abs();
    let end_distance = (x - end_x).abs();
    let nearest = if start_distance <= end_distance {
        (SpanEdge::Start, start_distance)
    } else {
        (SpanEdge::End, end_distance)
    };
    (nearest.1 <= radius).then_some(nearest.0)
}

/// The span with its start at `pointer` and its end at `end`, at least
/// `min_length` long and starting at or after zero.
pub(crate) fn span_with_start(pointer: f64, end: f64, min_length: f64) -> (f64, f64) {
    (pointer.clamp(0.0, (end - min_length).max(0.0)), end)
}

/// The span with its end at `pointer` and its start at `start`, at least
/// `min_length` long and ending at or before `max`.
pub(crate) fn span_with_end(pointer: f64, start: f64, min_length: f64, max: f64) -> (f64, f64) {
    (start, pointer.clamp((start + min_length).min(max), max))
}
