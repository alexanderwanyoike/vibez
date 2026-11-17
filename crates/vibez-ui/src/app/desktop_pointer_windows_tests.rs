//! The Win32 pointer source reads the cursor against the client area.

use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, SetCursorPos, WS_POPUP,
};

use super::desktop_pointer_windows::PointerWindow;

#[test]
fn the_cursor_is_read_relative_to_the_windows_client_area() {
    // A borderless popup's client area starts where the window does.
    // SAFETY: STATIC is a system class, and no pointer argument is kept.
    let hwnd = unsafe {
        CreateWindowExW(
            0,
            windows_sys::core::w!("STATIC"),
            std::ptr::null(),
            WS_POPUP,
            100,
            200,
            50,
            50,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null(),
        )
    };
    assert!(!hwnd.is_null(), "a window to read against");
    // SAFETY: moving the cursor touches no memory of ours.
    assert_ne!(
        unsafe { SetCursorPos(130, 215) },
        0,
        "a desktop to point at"
    );

    let pointer = PointerWindow::from_hwnd(hwnd).pointer();

    // SAFETY: this thread made the window and nothing else holds it.
    unsafe { DestroyWindow(hwnd) };
    assert_eq!(pointer, Some((30.0, 15.0)));
}
