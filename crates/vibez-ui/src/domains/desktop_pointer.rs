//! What a polled pointer over a drop target reports during another app's drag.

use vibez_core::id::TrackId;

use crate::domains::browser::BrowserMsg;
use crate::state::{BrowserState, DesktopDrop, GridConfig};
use crate::timeline_geometry::TimelineGeometry;

/// A place the view tags so a polled pointer can find it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PointerTarget {
    /// A track's lane, in Arrange or the selected Section.
    Lane(TrackId),
    /// The lane of the row where a drop adds a track.
    NewTrackRow,
    /// A slot of the clip launcher's new-track column.
    NewTrackSlot { row: u32 },
    /// An empty Clip slot of a track that takes the drag.
    ClipSlot { track_id: TrackId, row: u32 },
}

/// How the active timeline maps a lane's x to a beat, as its widgets do.
#[derive(Debug, Clone, Copy)]
pub struct LaneTiming {
    pub geometry: TimelineGeometry,
    pub grid: GridConfig,
}

impl LaneTiming {
    fn beat_at(self, x: f32) -> f64 {
        self.grid
            .snap_beat(self.geometry.x_to_beat(x), self.geometry.pixels_per_beat())
            .max(0.0)
    }
}

impl BrowserState {
    /// Whether another app's drag hovers the window with nothing yet
    /// dropped, so only a poll can say where its pointer is.
    pub fn polls_desktop_pointer(&self) -> bool {
        matches!(
            self.desktop_drop,
            Some(DesktopDrop::Hovering | DesktopDrop::PointerSeen)
        )
    }

    /// What the widget under a pointer polled at `x` would have reported:
    /// its hover followed by the pointer moving, or that no tagged target is
    /// there. `under` is the target there and its left edge; `compatible`
    /// says whether a track's lane takes the drag.
    pub fn pointer_messages(
        &self,
        under: Option<(PointerTarget, f32)>,
        x: f32,
        timing: LaneTiming,
        compatible: impl Fn(TrackId) -> bool,
    ) -> Vec<BrowserMsg> {
        let hover = match under {
            Some((PointerTarget::Lane(track_id), left)) => Some(BrowserMsg::DragHoverTrack {
                track_id,
                beat: timing.beat_at(x - left),
                compatible: compatible(track_id),
            }),
            Some((PointerTarget::NewTrackRow, left)) => {
                Some(BrowserMsg::DragHoverEmptyArrangement {
                    beat: timing.beat_at(x - left),
                })
            }
            Some((PointerTarget::NewTrackSlot { row }, _)) => {
                Some(BrowserMsg::DragHoverNewTrackSlot { row })
            }
            Some((PointerTarget::ClipSlot { track_id, row }, _)) => {
                Some(BrowserMsg::DragHoverClipSlot { track_id, row })
            }
            // Why finding nothing leaves the drop free: docs/PATTERNS.md, 9.
            None => return vec![BrowserMsg::DesktopPointerOffTargets],
        };
        hover
            .into_iter()
            .chain([BrowserMsg::DesktopPointerMoved])
            .collect()
    }
}

#[cfg(test)]
#[path = "desktop_pointer_tests.rs"]
mod tests;
