//! The Win32 pointer source reads the cursor against the client area.

use windows_sys::Win32::Foundation::POINT;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, GetCursorPos, WS_POPUP,
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
    // Reading the cursor where it is, rather than moving it, leaves it alone
    // for whoever runs the tests.
    let mut cursor = POINT { x: 0, y: 0 };
    // SAFETY: the call writes only the point it is given.
    assert_ne!(unsafe { GetCursorPos(&mut cursor) }, 0, "a cursor to read");

    let pointer = PointerWindow::from_hwnd(hwnd).pointer();

    // SAFETY: this thread made the window and nothing else holds it.
    unsafe { DestroyWindow(hwnd) };
    assert_eq!(
        pointer,
        Some(((cursor.x - 100) as f32, (cursor.y - 200) as f32))
    );
}
