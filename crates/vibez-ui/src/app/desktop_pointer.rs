//! Another app's drag, followed by polling the pointer, since the window
//! sees no pointer motion until the drop.

use iced::{Point, Task};

use super::App;
use crate::domains::desktop_pointer::{LaneTiming, PointerTarget};
use crate::message::Message;
use crate::state::Workspace;

/// Clip launcher rows a probe looks for, as many as the launcher shows.
const LAUNCHER_ROWS: u32 = 6;

/// The id the view gives the container around `target`.
pub(super) fn target_id(target: PointerTarget) -> iced::widget::container::Id {
    let name = match target {
        PointerTarget::Lane(track_id) => format!("lane/{}", track_id.raw()),
        PointerTarget::NewTrackRow => "new-track-row".to_string(),
        PointerTarget::NewTrackSlot { row } => format!("new-track-slot/{row}"),
        PointerTarget::ClipSlot { track_id, row } => {
            format!("clip-slot/{}/{row}", track_id.raw())
        }
    };
    iced::widget::container::Id::new(format!("desktop-drop-target:{name}"))
}

/// `content` tagged as `target`, so a polled pointer can find it.
pub(super) fn drop_target<'a>(
    content: impl Into<iced::Element<'a, Message>>,
    target: PointerTarget,
) -> iced::Element<'a, Message> {
    iced::widget::container(content)
        .id(target_id(target))
        .into()
}

/// How far the app has got with reading the pointer itself.
#[derive(Default)]
pub(super) enum PointerPoll {
    #[default]
    Unasked,
    Asking,
    #[cfg(not(any(target_os = "macos", windows)))]
    Ready {
        window: Box<super::desktop_pointer_x11::PointerWindow>,
        /// The window's scale factor, which with the interface scale turns
        /// physical pixels into the layout's.
        scale: f32,
        last: Option<Point>,
    },
    Unavailable,
}

impl App {
    /// Reports the target the polled pointer is over to the Browser drag,
    /// as the widget there would have.
    pub(super) fn on_desktop_pointer_probed(
        &mut self,
        x: f32,
        under: Option<(PointerTarget, f32)>,
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
            .pointer_messages(under, x, timing, |track_id| {
                self.state
                    .find_track(track_id)
                    .is_some_and(|track| self.state.browser.lane_refusal(track.kind).is_none())
            });
        Task::batch(messages.into_iter().map(|message| {
            let action = self.state.browser.update(message);
            self.apply_browser_action(action)
        }))
    }

    /// Reads the pointer while another app's drag hovers, and asks the
    /// view what is under it whenever it moves.
    pub(super) fn poll_desktop_pointer(&mut self) -> Task<Message> {
        if !self.state.browser.polls_desktop_pointer() {
            // Each drag asks afresh, so a window moved to another scale, or
            // an answer that never came, holds for one drag at most.
            if !matches!(
                self.pointer_poll,
                PointerPoll::Unasked | PointerPoll::Unavailable
            ) {
                self.pointer_poll = PointerPoll::Unasked;
            }
            return Task::none();
        }
        match &mut self.pointer_poll {
            PointerPoll::Unasked => {
                self.pointer_poll = PointerPoll::Asking;
                find_pointer_window()
            }
            #[cfg(not(any(target_os = "macos", windows)))]
            PointerPoll::Ready {
                window,
                scale,
                last,
            } => {
                let Some((x, y)) = window.pointer() else {
                    return Task::none();
                };
                let factor = *scale * self.interface_scale;
                let point = Point::new(x / factor, y / factor);
                if *last == Some(point) {
                    return Task::none();
                }
                *last = Some(point);
                let probe =
                    super::drop_target_probe::DropTargetProbe::new(point, self.pointer_targets());
                iced::advanced::widget::operate(probe).map(move |under| {
                    Message::DesktopPointerProbed {
                        x: point.x,
                        under: under.map(|(target, bounds)| (target, bounds.x)),
                    }
                })
            }
            _ => Task::none(),
        }
    }

    /// Keeps the X window a poll reads the pointer against, or gives up
    /// polling where there is none.
    pub(super) fn on_desktop_pointer_window(&mut self, window: Option<u32>, scale: f32) {
        self.pointer_poll = PointerPoll::Unavailable;
        #[cfg(not(any(target_os = "macos", windows)))]
        if let Some(window) = window.and_then(super::desktop_pointer_x11::PointerWindow::connect) {
            self.pointer_poll = PointerPoll::Ready {
                window: Box::new(window),
                scale,
                last: None,
            };
        }
        #[cfg(any(target_os = "macos", windows))]
        let _ = (window, scale);
    }

    /// Every drop target the view may have tagged.
    fn pointer_targets(&self) -> Vec<PointerTarget> {
        let first_row = self.state.perform.clip_editor.first_row;
        let rows = 0..first_row + LAUNCHER_ROWS;
        let tracks = self
            .state
            .project_tracks
            .tracks
            .iter()
            .map(|track| track.id);
        tracks
            .clone()
            .map(PointerTarget::Lane)
            .chain([PointerTarget::NewTrackRow])
            .chain(rows.clone().map(|row| PointerTarget::NewTrackSlot { row }))
            .chain(tracks.flat_map(move |track_id| {
                rows.clone()
                    .map(move |row| PointerTarget::ClipSlot { track_id, row })
            }))
            .collect()
    }
}

/// The app's X window and scale factor, once iced reports them.
fn find_pointer_window() -> Task<Message> {
    iced::window::get_latest().and_then(|id| {
        iced::window::run_with_handle(id, x11_window).then(move |window| {
            iced::window::get_scale_factor(id)
                .map(move |scale| Message::DesktopPointerWindow { window, scale })
        })
    })
}

#[cfg(not(any(target_os = "macos", windows)))]
use super::desktop_pointer_x11::x11_window;

#[cfg(any(target_os = "macos", windows))]
fn x11_window(_handle: iced::window::raw_window_handle::WindowHandle<'_>) -> Option<u32> {
    None
}
