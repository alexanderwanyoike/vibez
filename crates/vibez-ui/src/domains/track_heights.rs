//! How tall each Arrange track's lane is drawn, as header-edge drags
//! resize one lane or, with Alt, every lane.

use std::collections::HashMap;

use vibez_core::id::TrackId;
use vibez_project::TrackHeight;

/// Messages the track height domain handles.
#[derive(Debug, Clone)]
pub enum TrackHeightMsg {
    /// Pressed on this track header's bottom edge.
    BeginResize(TrackId),
    /// The pointer moved during a resize.
    PointerMoved,
    EndResize,
}

/// Read-only facts a resize needs from outside the domain.
#[derive(Debug, Clone, Copy)]
pub struct TrackHeightCtx<'a> {
    /// Window y of the pointer.
    pub cursor_y: f32,
    /// Alt is held, so the drag sets every track to one height.
    pub all_tracks: bool,
    /// Every Arrange track, top to bottom.
    pub track_ids: &'a [TrackId],
}

/// Cross-domain effects requested by a track height update.
#[derive(Debug, Default, PartialEq)]
pub struct TrackHeightAction {
    /// A finished resize changed a height the project saves.
    pub mark_dirty: bool,
}

/// Lane height per Arrange track. Tracks never resized are absent and
/// draw at [`TrackHeight::DEFAULT`].
#[derive(Debug, Clone, Default)]
pub struct TrackHeights {
    heights: HashMap<TrackId, TrackHeight>,
    resize: Option<TrackResize>,
}

#[derive(Debug, Clone)]
struct TrackResize {
    track_id: TrackId,
    anchor_y: f32,
    start_height: TrackHeight,
    targets: Vec<TrackId>,
    changed: bool,
}

impl TrackHeights {
    /// Heights read back from a project file.
    pub fn from_saved(saved: &[(TrackId, TrackHeight)]) -> Self {
        Self {
            heights: saved.iter().copied().collect(),
            resize: None,
        }
    }

    /// The heights a project file keeps for these tracks.
    pub fn saved(
        &self,
        track_ids: impl IntoIterator<Item = TrackId>,
    ) -> Vec<(TrackId, TrackHeight)> {
        track_ids
            .into_iter()
            .filter_map(|track_id| {
                self.heights
                    .get(&track_id)
                    .map(|&height| (track_id, height))
            })
            .collect()
    }

    pub fn height(&self, track_id: TrackId) -> TrackHeight {
        self.heights.get(&track_id).copied().unwrap_or_default()
    }

    /// The track whose header edge is being dragged.
    pub fn resizing(&self) -> Option<TrackId> {
        self.resize.as_ref().map(|resize| resize.track_id)
    }

    pub fn update(&mut self, msg: TrackHeightMsg, ctx: TrackHeightCtx<'_>) -> TrackHeightAction {
        let mut action = TrackHeightAction::default();
        match msg {
            TrackHeightMsg::BeginResize(track_id) => {
                self.resize = Some(TrackResize {
                    track_id,
                    anchor_y: ctx.cursor_y,
                    start_height: self.height(track_id),
                    targets: if ctx.all_tracks {
                        ctx.track_ids.to_vec()
                    } else {
                        vec![track_id]
                    },
                    changed: false,
                });
            }
            TrackHeightMsg::PointerMoved => {
                if let Some(resize) = &mut self.resize {
                    let height = TrackHeight::new(
                        resize.start_height.pixels() + ctx.cursor_y - resize.anchor_y,
                    );
                    for &track_id in &resize.targets {
                        let previous = self.heights.get(&track_id).copied();
                        if previous.unwrap_or_default() != height {
                            self.heights.insert(track_id, height);
                            resize.changed = true;
                        }
                    }
                }
            }
            TrackHeightMsg::EndResize => {
                action.mark_dirty = self.resize.take().is_some_and(|resize| resize.changed);
            }
        }
        action
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tracks() -> Vec<TrackId> {
        (0..3).map(|_| TrackId::new()).collect()
    }

    fn ctx(cursor_y: f32, all_tracks: bool, track_ids: &[TrackId]) -> TrackHeightCtx<'_> {
        TrackHeightCtx {
            cursor_y,
            all_tracks,
            track_ids,
        }
    }

    fn drag(
        heights: &mut TrackHeights,
        track_id: TrackId,
        (from_y, to_y): (f32, f32),
        all_tracks: bool,
        ids: &[TrackId],
    ) -> TrackHeightAction {
        heights.update(
            TrackHeightMsg::BeginResize(track_id),
            ctx(from_y, all_tracks, ids),
        );
        heights.update(TrackHeightMsg::PointerMoved, ctx(to_y, all_tracks, ids));
        heights.update(TrackHeightMsg::EndResize, ctx(to_y, all_tracks, ids))
    }

    #[test]
    fn dragging_a_header_edge_resizes_only_that_track() {
        let ids = tracks();
        let mut heights = TrackHeights::default();

        heights.update(TrackHeightMsg::BeginResize(ids[0]), ctx(300.0, false, &ids));
        assert_eq!(heights.resizing(), Some(ids[0]));
        heights.update(TrackHeightMsg::PointerMoved, ctx(440.0, false, &ids));

        assert_eq!(heights.height(ids[0]), TrackHeight::new(210.0));
        assert_eq!(heights.height(ids[1]), TrackHeight::DEFAULT);
    }

    #[test]
    fn a_resize_stops_at_the_minimum_and_maximum_height() {
        let ids = tracks();
        let mut heights = TrackHeights::default();

        drag(&mut heights, ids[0], (300.0, -2000.0), false, &ids);
        assert_eq!(heights.height(ids[0]), TrackHeight::MIN);

        drag(&mut heights, ids[0], (300.0, 5000.0), false, &ids);
        assert_eq!(heights.height(ids[0]), TrackHeight::MAX);
    }

    #[test]
    fn alt_dragging_sets_every_track_to_the_dragged_tracks_height() {
        let ids = tracks();
        let mut heights = TrackHeights::default();
        drag(&mut heights, ids[0], (100.0, 240.0), false, &ids);

        drag(&mut heights, ids[1], (300.0, 330.0), true, &ids);

        assert!(ids
            .iter()
            .all(|&id| heights.height(id) == TrackHeight::new(100.0)));
    }

    #[test]
    fn only_a_resize_that_changed_a_height_marks_the_project_dirty() {
        let ids = tracks();
        let mut heights = TrackHeights::default();

        assert_eq!(
            drag(&mut heights, ids[0], (300.0, 300.0), false, &ids),
            TrackHeightAction::default()
        );
        assert_eq!(
            drag(&mut heights, ids[0], (300.0, 320.0), false, &ids),
            TrackHeightAction { mark_dirty: true }
        );
        assert_eq!(heights.resizing(), None);
    }

    #[test]
    fn moving_the_pointer_without_a_resize_changes_nothing() {
        let ids = tracks();
        let mut heights = TrackHeights::default();

        heights.update(TrackHeightMsg::PointerMoved, ctx(500.0, false, &ids));

        assert_eq!(heights.height(ids[0]), TrackHeight::DEFAULT);
    }

    #[test]
    fn saved_heights_come_back_and_skip_tracks_no_longer_there() {
        let ids = tracks();
        let mut heights = TrackHeights::default();
        drag(&mut heights, ids[0], (100.0, 240.0), false, &ids);
        drag(&mut heights, ids[2], (100.0, 80.0), false, &ids);

        let saved = heights.saved([ids[0], ids[1]]);
        assert_eq!(saved, vec![(ids[0], TrackHeight::new(210.0))]);

        let reopened = TrackHeights::from_saved(&saved);
        assert_eq!(reopened.height(ids[0]), TrackHeight::new(210.0));
        assert_eq!(reopened.height(ids[2]), TrackHeight::DEFAULT);
    }
}
