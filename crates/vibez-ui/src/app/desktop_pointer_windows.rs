//! The pointer read with Win32, which answers even while another app's OLE
//! drag holds the mouse.

use windows_sys::Win32::Foundation::HWND;

/// The app's top-level window.
#[derive(Debug)]
pub struct PointerWindow {
    /// The HWND as a number, so the window can cross to the UI thread.
    hwnd: isize,
}

impl PointerWindow {
    pub(super) fn from_hwnd(hwnd: HWND) -> Self {
        Self {
            hwnd: hwnd as isize,
        }
    }

    /// The pointer relative to the window's client area, in physical pixels.
    pub(super) fn pointer(&self) -> Option<(f32, f32)> {
        let _ = self.hwnd;
        todo!()
    }
}
