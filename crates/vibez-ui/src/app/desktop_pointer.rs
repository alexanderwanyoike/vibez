//! Another app's drag, followed by polling the pointer, since the window
//! sees no pointer motion until the drop.

use iced::{Point, Rectangle, Task};

use super::App;
use crate::domains::desktop_pointer::{LaneTiming, PointerTarget};
use crate::message::Message;
use crate::state::Workspace;

impl App {
    /// Reports the target the polled pointer is over to the Browser drag,
    /// as the widget there would have.
    pub(super) fn on_desktop_pointer_probed(
        &mut self,
        point: Point,
        under: Option<(PointerTarget, Rectangle)>,
    ) -> Task<Message> {
        if !self.state.browser.polls_desktop_pointer() {
            return Task::none();
        }
        let geometry = if self.state.view.workspace == Workspace::Perform {
            self.state.perform.section_editor.viewport().geometry()
        } else {
            self.state.view.viewport.geometry()
        };
        let timing = LaneTiming {
            geometry,
            grid: self.state.view.arrange_grid,
        };
        let messages = self
            .state
            .browser
            .pointer_messages(under, point, timing, |track_id| {
                self.state
                    .find_track(track_id)
                    .is_some_and(|track| self.state.browser.drag_lands_on(track.kind))
            });
        Task::batch(messages.into_iter().map(|message| {
            let action = self.state.browser.update(message);
            self.apply_browser_action(action)
        }))
    }
}
