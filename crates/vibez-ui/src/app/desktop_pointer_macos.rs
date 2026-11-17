//! The pointer read from AppKit, which answers even while another app's
//! drag session holds the mouse.

use iced::window::raw_window_handle::{RawWindowHandle, WindowHandle};
use objc2::MainThreadMarker;
use objc2_app_kit::{NSApplication, NSEvent, NSView};

/// The app's window, by number, so it can cross to the UI thread and be
/// looked up again there.
#[derive(Debug)]
pub struct PointerWindow {
    number: isize,
}

impl PointerWindow {
    /// The window behind `handle`, when it is an AppKit one.
    pub(super) fn from_handle(handle: WindowHandle<'_>) -> Option<Self> {
        let RawWindowHandle::AppKit(window) = handle.as_raw() else {
            return None;
        };
        // SAFETY: the handle borrows the live window this view belongs to,
        // and iced hands it over on the main thread.
        let view = unsafe { window.ns_view.cast::<NSView>().as_ref() };
        Some(Self {
            number: view.window()?.windowNumber(),
        })
    }

    /// The pointer relative to the window's content, in physical pixels.
    pub(super) fn pointer(&self) -> Option<(f32, f32)> {
        let main_thread = MainThreadMarker::new()?;
        let window =
            NSApplication::sharedApplication(main_thread).windowWithWindowNumber(self.number)?;
        let height = window.contentView()?.frame().size.height;
        let point = window.convertPointFromScreen(NSEvent::mouseLocation());
        Some(super::desktop_pointer::top_left_pixels(
            point.x,
            point.y,
            height,
            window.backingScaleFactor(),
        ))
    }
}
