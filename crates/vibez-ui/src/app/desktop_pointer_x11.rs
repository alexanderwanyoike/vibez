//! The pointer read straight from the X server, which answers even while
//! another app's drag holds it.

use x11rb::connection::Connection;
use x11rb::protocol::xproto::ConnectionExt as _;
use x11rb::rust_connection::RustConnection;

/// The app's window on its own connection to the X server.
pub(super) struct PointerWindow {
    connection: RustConnection,
    window: u32,
}

impl PointerWindow {
    pub(super) fn connect(window: u32) -> Option<Self> {
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
        let _ = self.connection.flush();
        reply
            .same_screen
            .then(|| (f32::from(reply.win_x), f32::from(reply.win_y)))
    }
}

/// The X window id behind `handle`, when the window is an X11 one.
pub(super) fn x11_window(handle: iced::window::raw_window_handle::WindowHandle<'_>) -> Option<u32> {
    use iced::window::raw_window_handle::RawWindowHandle;
    match handle.as_raw() {
        RawWindowHandle::Xlib(window) => u32::try_from(window.window).ok(),
        RawWindowHandle::Xcb(window) => Some(window.window.get()),
        _ => None,
    }
}
