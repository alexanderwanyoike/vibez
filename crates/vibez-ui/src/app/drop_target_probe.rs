//! Finds which tagged drop target the view laid out under a point.

use std::collections::HashMap;

use iced::advanced::widget::operation::{Operation, Outcome, Scrollable};
use iced::advanced::widget::Id;
use iced::{Point, Rectangle, Vector};

use crate::domains::desktop_pointer::PointerTarget;

/// Walks the widget tree for the last of `wanted` whose visible part holds
/// `point`, since later widgets draw on top.
pub(super) struct DropTargetProbe {
    point: Point,
    wanted: HashMap<Id, PointerTarget>,
    /// Each scrollable entered: its total translation, its visible area and
    /// the depth it was entered at, as iced's own visible-bounds query keeps.
    scrollables: Vec<(Vector, Rectangle, usize)>,
    depth: usize,
    found: Option<(PointerTarget, Rectangle)>,
}

impl DropTargetProbe {
    pub(super) fn new(point: Point, wanted: impl IntoIterator<Item = PointerTarget>) -> Self {
        Self {
            point,
            wanted: wanted
                .into_iter()
                .map(|target| (Id::from(super::desktop_pointer::target_id(target)), target))
                .collect(),
            scrollables: Vec::new(),
            depth: 0,
            found: None,
        }
    }
}

impl Operation<Option<(PointerTarget, Rectangle)>> for DropTargetProbe {
    fn scrollable(
        &mut self,
        _state: &mut dyn Scrollable,
        _id: Option<&Id>,
        bounds: Rectangle,
        _content_bounds: Rectangle,
        translation: Vector,
    ) {
        let entry = match self.scrollables.last() {
            Some((outer, viewport, _)) => (
                translation + *outer,
                viewport
                    .intersection(&(bounds - *outer))
                    .unwrap_or(Rectangle::new(Point::ORIGIN, iced::Size::ZERO)),
                self.depth,
            ),
            None => (translation, bounds, self.depth),
        };
        self.scrollables.push(entry);
    }

    fn container(
        &mut self,
        id: Option<&Id>,
        bounds: Rectangle,
        operate_on_children: &mut dyn FnMut(&mut dyn Operation<Option<(PointerTarget, Rectangle)>>),
    ) {
        if let Some(target) = id.and_then(|id| self.wanted.get(id)).copied() {
            let (shown, visible) = match self.scrollables.last() {
                Some((translation, viewport, _)) => {
                    let shown = bounds - *translation;
                    (shown, viewport.intersection(&shown))
                }
                None => (bounds, Some(bounds)),
            };
            if visible.is_some_and(|visible| visible.contains(self.point)) {
                self.found = Some((target, shown));
            }
        }
        self.depth += 1;
        operate_on_children(self);
        self.depth -= 1;
        if matches!(self.scrollables.last(), Some((_, _, depth)) if *depth == self.depth) {
            let _ = self.scrollables.pop();
        }
    }

    fn finish(&self) -> Outcome<Option<(PointerTarget, Rectangle)>> {
        Outcome::Some(self.found)
    }
}
