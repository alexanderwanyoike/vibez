//! Files other apps drag over the window, from window event to their drop target.

use iced::Task;

use super::App;
use crate::domains::browser::BrowserMsg;
use crate::message::Message;
use crate::state::BrowserDropTarget;

/// The Browser message a file dragged over the window from another app sends.
pub(super) fn desktop_drop_message(event: &iced::window::Event) -> Option<Message> {
    use iced::window::Event;
    let message = match event {
        Event::FileHovered(path) => BrowserMsg::DesktopFileHovered(path.clone()),
        Event::FileDropped(path) => BrowserMsg::DesktopFileDropped(path.clone()),
        Event::FilesHoveredLeft => BrowserMsg::DesktopDragLeft,
        _ => return None,
    };
    Some(Message::Browser(message))
}

impl App {
    /// A file dropped from another app arrives with no button release, so
    /// the target the pointer reported takes the drop in its place.
    pub(super) fn release_media_drag_at(&mut self, target: BrowserDropTarget) -> Task<Message> {
        let message = match target {
            // The Browser domain releases only on targets that take the drag.
            BrowserDropTarget::ArrangementLane { track_id, beat, .. } => {
                Message::DropSampleOnArrangement {
                    track_id,
                    position_samples: self.state.transport.beats_to_samples(beat),
                }
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
        self.state.browser.end_desktop_drop();
        self.update(message)
    }
}
