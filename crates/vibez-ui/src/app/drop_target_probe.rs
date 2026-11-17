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

#[cfg(test)]
mod tests {
    use iced::advanced::widget::operation::scrollable::{AbsoluteOffset, RelativeOffset};
    use iced::Size;
    use vibez_core::id::TrackId;

    use super::*;

    struct StillScrollable;

    impl Scrollable for StillScrollable {
        fn snap_to(&mut self, _offset: RelativeOffset) {}
        fn scroll_to(&mut self, _offset: AbsoluteOffset) {}
        fn scroll_by(&mut self, _offset: AbsoluteOffset, _bounds: Rectangle, _content: Rectangle) {}
    }

    fn id(target: PointerTarget) -> Id {
        Id::from(super::super::desktop_pointer::target_id(target))
    }

    fn rect(x: f32, y: f32, width: f32, height: f32) -> Rectangle {
        Rectangle::new(Point::new(x, y), Size::new(width, height))
    }

    fn found(probe: &DropTargetProbe) -> Option<(PointerTarget, Rectangle)> {
        match probe.finish() {
            Outcome::Some(found) => found,
            _ => None,
        }
    }

    /// A list scrolled down by `scrolled` inside a 200 pixel tall viewport
    /// at y 100, holding `target` at content y `content_y`, as iced's
    /// scrollable reports it.
    fn scrolled_list(
        probe: &mut DropTargetProbe,
        scrolled: f32,
        target: PointerTarget,
        content_y: f32,
    ) {
        let viewport = rect(0.0, 100.0, 800.0, 200.0);
        probe.scrollable(
            &mut StillScrollable,
            None,
            viewport,
            rect(0.0, 100.0, 800.0, 2_000.0),
            Vector::new(0.0, scrolled),
        );
        probe.container(None, viewport, &mut |probe| {
            probe.container(
                Some(&id(target)),
                rect(0.0, content_y, 800.0, 60.0),
                &mut |_| {},
            );
        });
    }

    #[test]
    fn the_target_under_the_point_is_found_with_its_bounds() {
        let lane = PointerTarget::Lane(TrackId::new());
        let mut probe = DropTargetProbe::new(Point::new(50.0, 30.0), [lane]);

        probe.container(Some(&id(lane)), rect(0.0, 0.0, 800.0, 60.0), &mut |_| {});

        assert_eq!(found(&probe), Some((lane, rect(0.0, 0.0, 800.0, 60.0))));
    }

    #[test]
    fn a_scrolled_target_is_found_where_it_shows() {
        let lane = PointerTarget::Lane(TrackId::new());
        let mut probe = DropTargetProbe::new(Point::new(50.0, 130.0), [lane]);

        scrolled_list(&mut probe, 300.0, lane, 400.0);

        assert_eq!(found(&probe), Some((lane, rect(0.0, 100.0, 800.0, 60.0))));
    }

    #[test]
    fn a_target_scrolled_out_of_view_is_not_under_the_point() {
        let lane = PointerTarget::Lane(TrackId::new());
        let mut probe = DropTargetProbe::new(Point::new(50.0, 60.0), [lane]);

        // Scrolled up past the viewport's top edge, so it shows at y 50,
        // clipped away by the viewport that starts at y 100.
        scrolled_list(&mut probe, 350.0, lane, 400.0);

        assert_eq!(found(&probe), None);
    }

    #[test]
    fn the_innermost_tagged_target_wins_and_untagged_ones_are_ignored() {
        let row = PointerTarget::NewTrackRow;
        let slot = PointerTarget::NewTrackSlot { row: 2 };
        let untagged = PointerTarget::Lane(TrackId::new());
        let mut probe = DropTargetProbe::new(Point::new(50.0, 30.0), [row, slot]);

        probe.container(Some(&id(row)), rect(0.0, 0.0, 800.0, 60.0), &mut |probe| {
            probe.container(Some(&id(slot)), rect(0.0, 0.0, 200.0, 60.0), &mut |probe| {
                probe.container(
                    Some(&id(untagged)),
                    rect(0.0, 0.0, 100.0, 60.0),
                    &mut |_| {},
                );
            });
        });

        assert_eq!(found(&probe).map(|(target, _)| target), Some(slot));
    }
}
