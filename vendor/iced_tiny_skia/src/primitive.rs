use crate::core::Rectangle;

#[derive(Debug, Clone, PartialEq)]
pub enum Primitive {
    /// A path filled with some paint.
    Fill {
        /// The path to fill.
        path: tiny_skia::Path,
        /// The paint to use.
        paint: tiny_skia::Paint<'static>,
        /// The fill rule to follow.
        rule: tiny_skia::FillRule,
    },
    /// A path stroked with some paint.
    Stroke {
        /// The path to stroke.
        path: tiny_skia::Path,
        /// The paint to use.
        paint: tiny_skia::Paint<'static>,
        /// The stroke settings.
        stroke: tiny_skia::Stroke,
    },
}

impl Primitive {
    /// Returns the visible bounds of the [`Primitive`].
    pub fn visible_bounds(&self) -> Rectangle {
        let (bounds, outset) = match self {
            Primitive::Fill { path, .. } => (path.bounds(), 0.0),
            // A stroke reaches half its width past the path, and a
            // straight axis-aligned path has zero-area bounds that would
            // otherwise be culled and never damaged.
            Primitive::Stroke { path, stroke, .. } => {
                (path.bounds(), stroke.width / 2.0)
            }
        };

        Rectangle {
            x: bounds.x() - outset,
            y: bounds.y() - outset,
            width: bounds.width() + outset * 2.0,
            height: bounds.height() + outset * 2.0,
        }
    }
}
