//! The pointer read with Win32, which answers even while another app's OLE
//! drag holds the mouse.

use iced::window::raw_window_handle::{RawWindowHandle, WindowHandle};
use windows_sys::Win32::Foundation::{HWND, POINT};
use windows_sys::Win32::Graphics::Gdi::ScreenToClient;
use windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos;

/// The app's top-level window.
#[derive(Debug)]
pub struct PointerWindow {
    /// The HWND as a number, since a raw HWND cannot travel in a `Send` message.
    hwnd: isize,
}

impl PointerWindow {
    /// The window behind `handle`, when it is a Win32 one.
    pub(super) fn from_handle(handle: WindowHandle<'_>) -> Option<Self> {
        match handle.as_raw() {
            RawWindowHandle::Win32(window) => Some(Self::from_hwnd(window.hwnd.get() as HWND)),
            _ => None,
        }
    }

    pub(super) fn from_hwnd(hwnd: HWND) -> Self {
        Self {
            hwnd: hwnd as isize,
        }
    }

    /// The pointer relative to the window's client area, in physical pixels
    /// when the process is per-monitor DPI aware, as winit makes it.
    pub(super) fn pointer(&self) -> Option<(f32, f32)> {
        let mut point = POINT { x: 0, y: 0 };
        // SAFETY: each call writes only the point it is given, and a window
        // destroyed meanwhile makes ScreenToClient fail rather than misbehave.
        let read = unsafe {
            GetCursorPos(&mut point) != 0 && ScreenToClient(self.hwnd as HWND, &mut point) != 0
        };
        read.then_some((point.x as f32, point.y as f32))
    }
}
