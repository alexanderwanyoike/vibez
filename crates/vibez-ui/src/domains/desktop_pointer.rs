//! The drop target under the pointer while another app's drag hovers the
//! window. On X11 the dragging app holds the pointer, so the widgets never
//! see it move; a poll finds the pointer and the target the view tagged
//! there, and reports it as the widget would have.

use iced::{Point, Rectangle};
use vibez_core::id::TrackId;

use crate::domains::browser::BrowserMsg;
use crate::state::{BrowserState, DesktopDrop, GridConfig};
use crate::timeline_geometry::TimelineGeometry;

const ID_PREFIX: &str = "desktop-drop-target:";

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

impl PointerTarget {
    /// The id the view gives the container around this target.
    pub fn id(self) -> iced::widget::container::Id {
        let name = match self {
            Self::Lane(track_id) => format!("lane/{}", track_id.raw()),
            Self::NewTrackRow => "new-track-row".to_string(),
            Self::NewTrackSlot { row } => format!("new-track-slot/{row}"),
            Self::ClipSlot { track_id, row } => {
                format!("clip-slot/{}/{row}", track_id.raw())
            }
        };
        iced::widget::container::Id::new(format!("{ID_PREFIX}{name}"))
    }
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

    /// What the widget under the polled pointer would have reported: its
    /// hover, or leaving the target it had, followed by the pointer moving.
    /// `compatible` says whether a track's lane takes the drag.
    pub fn pointer_messages(
        &self,
        under: Option<(PointerTarget, Rectangle)>,
        point: Point,
        timing: LaneTiming,
        compatible: impl Fn(TrackId) -> bool,
    ) -> Vec<BrowserMsg> {
        let hover = match under {
            Some((PointerTarget::Lane(track_id), bounds)) => Some(BrowserMsg::DragHoverTrack {
                track_id,
                beat: timing.beat_at(point.x - bounds.x),
                compatible: compatible(track_id),
            }),
            Some((PointerTarget::NewTrackRow, bounds)) => {
                Some(BrowserMsg::DragHoverEmptyArrangement {
                    beat: timing.beat_at(point.x - bounds.x),
                })
            }
            Some((PointerTarget::NewTrackSlot { row }, _)) => {
                Some(BrowserMsg::DragHoverNewTrackSlot { row })
            }
            Some((PointerTarget::ClipSlot { track_id, row }, _)) => {
                Some(BrowserMsg::DragHoverClipSlot { track_id, row })
            }
            None => self
                .drag_target
                .is_some()
                .then_some(BrowserMsg::ClearDragTarget),
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
