//! The pointer read straight from the X server, which answers even while
//! another app's drag holds it.

use iced::window::raw_window_handle::{RawWindowHandle, WindowHandle};
use x11rb::protocol::xproto::ConnectionExt as _;
use x11rb::rust_connection::RustConnection;

/// The app's window on its own connection to the X server.
#[derive(Debug)]
pub struct PointerWindow {
    connection: RustConnection,
    window: u32,
}

impl PointerWindow {
    /// Connects for the window behind `handle`, when it is an X11 one.
    pub(super) fn from_handle(handle: WindowHandle<'_>) -> Option<Self> {
        let window = match handle.as_raw() {
            RawWindowHandle::Xlib(window) => u32::try_from(window.window).ok()?,
            RawWindowHandle::Xcb(window) => window.window.get(),
            _ => return None,
        };
        let (connection, _) = x11rb::connect(None).ok()?;
        Some(Self { connection, window })
    }

    /// The pointer relative to the window, in physical pixels.
    pub(super) fn pointer(&self) -> Option<(f32, f32)> {
        let reply = self
            .connection
            .query_pointer(self.window)
            .ok()?
            .reply()
            .ok()?;
        reply
            .same_screen
            .then(|| (f32::from(reply.win_x), f32::from(reply.win_y)))
    }
}
