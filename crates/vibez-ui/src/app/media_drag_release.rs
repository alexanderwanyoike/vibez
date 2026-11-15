//! Releases a media drag on its target, as a button release over it would.

use iced::Task;

use super::App;
use crate::domains::browser::BrowserMsg;
use crate::message::Message;
use crate::state::BrowserDropTarget;

impl App {
    /// A file dropped from another app arrives with no button release, so
    /// the target the pointer reported takes the drop in its place.
    pub(super) fn release_media_drag_at(&mut self, target: BrowserDropTarget) -> Task<Message> {
        let message = match target {
            BrowserDropTarget::ArrangementLane {
                track_id,
                beat,
                compatible: true,
            } => Message::DropSampleOnArrangement {
                track_id,
                position_samples: self.state.transport.beats_to_samples(beat),
            },
            refused @ BrowserDropTarget::ArrangementLane { .. } => {
                let media = self
                    .state
                    .browser
                    .drag_media()
                    .unwrap_or(crate::domains::browser::DragMedia::Audio);
                Message::Browser(BrowserMsg::CancelDrag(refused.status(media)))
            }
            BrowserDropTarget::EmptyArrangement { .. } => Message::DropSampleOnEmptyArrangement,
            BrowserDropTarget::Sampler { track_id } => Message::DropSampleOnSampler { track_id },
            BrowserDropTarget::DrumRackPad {
                track_id,
                pad_index,
            } => Message::DropSampleOnDrumPad {
                track_id,
                pad_index,
            },
            BrowserDropTarget::DrumRack { track_id, .. } => {
                Message::DropSampleOnDrumRack { track_id }
            }
            BrowserDropTarget::DevicePanel { track_id, .. } => {
                Message::DropSampleOnDevicePanel { track_id }
            }
            BrowserDropTarget::ClipSlot { track_id, row } => {
                Message::ImportLauncherClip { track_id, row }
            }
        };
        self.update(message)
    }
}
