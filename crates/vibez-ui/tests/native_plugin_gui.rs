#[cfg(target_os = "macos")]
fn main() {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSApplication, NSView};
    use objc2_foundation::NSSize;
    use vibez_core::id::TrackId;
    use vibez_plugin_host::PluginGuiKey;
    use vibez_plugin_host::{gui::GuiApi, test_fixtures as fixtures};
    use vibez_ui::plugin_window::{PluginRawPtr, PluginWindowManager};

    let mtm = MainThreadMarker::new().expect("native GUI test must run on the process main thread");
    let _app = NSApplication::sharedApplication(mtm);
    for vst3 in [false, true] {
        let state = fixtures::State::new(GuiApi::Cocoa);
        state.parent_probe.set(Some(parent_is_alive));
        let clap = fixtures::Clap::new(&state);
        let vst = fixtures::Vst::new(&state);
        let mut manager =
            PluginWindowManager::new().expect("AppKit must not require an X11 display");
        let ptr = if vst3 {
            PluginRawPtr::Vst3(vst.ptr())
        } else {
            PluginRawPtr::Clap(clap.raw_ptr())
        };
        let key = PluginGuiKey::Instrument {
            track_id: TrackId::new(),
        };
        for expected_destroys in 1..=2 {
            assert!(manager.open(key, ptr, "Native plugin test".into()));
            assert!(state.correct_api.get());
            let parent = state.parent.get();
            assert!(!parent.is_null());
            let view = unsafe { &*(parent as *const NSView) };
            let window = view.window().unwrap();
            assert_eq!(view.bounds().size, NSSize::new(320.0, 240.0));
            manager.raise(key);
            state.resize_step.set(16);
            state.resize_calls.borrow_mut().clear();
            window.setContentSize(NSSize::new(490.0, 370.0));
            manager.poll_events();
            assert_eq!(state.size.get(), (480, 368));
            assert_eq!(view.bounds().size, NSSize::new(480.0, 368.0));
            assert_eq!(*state.resize_calls.borrow(), ["adjust", "set_size"]);
            window.performClose(None);
            assert_eq!(manager.poll_events().len(), 1);
            assert!(!manager.is_open(key));
            assert_eq!(state.destroys.get(), expected_destroys);
            assert!(state.parent_alive_at_destroy.get());
        }
        assert!(manager.open(key, ptr, "Track teardown".into()));
        if let PluginGuiKey::Instrument { track_id } = key {
            manager.close_track_effects(track_id);
        }
        assert_eq!(state.destroys.get(), 3);
        assert!(manager.open(key, ptr, "App teardown".into()));
        drop(manager);
        assert_eq!(state.destroys.get(), 4);
        assert!(state.parent_alive_at_destroy.get());
    }
}

#[cfg(windows)]
fn main() {
    use vibez_core::id::TrackId;
    use vibez_plugin_host::PluginGuiKey;
    use vibez_plugin_host::{gui::GuiApi, test_fixtures as fixtures};
    use vibez_ui::plugin_window::{PluginRawPtr, PluginWindowManager};
    use windows_sys::Win32::UI::WindowsAndMessaging::{SendMessageW, WM_CLOSE};

    for vst3 in [false, true] {
        let state = fixtures::State::new(GuiApi::Win32);
        state.parent_probe.set(Some(win32::parent_is_alive));
        let clap = fixtures::Clap::new(&state);
        let vst = fixtures::Vst::new(&state);
        let mut manager =
            PluginWindowManager::new().expect("Win32 must not require an X11 display");
        let ptr = if vst3 {
            PluginRawPtr::Vst3(vst.ptr())
        } else {
            PluginRawPtr::Clap(clap.raw_ptr())
        };
        let key = PluginGuiKey::Instrument {
            track_id: TrackId::new(),
        };
        for expected_destroys in 1..=2 {
            assert!(manager.open(key, ptr, "Native plugin test".into()));
            assert!(state.correct_api.get());
            let hwnd = state.parent.get();
            assert!(!hwnd.is_null());
            assert_eq!(win32::client_size(hwnd), (320, 240));
            manager.raise(key);
            state.resize_step.set(16);
            state.resize_calls.borrow_mut().clear();
            win32::set_client_size(hwnd, (490, 370));
            manager.poll_events();
            assert_eq!(state.size.get(), (480, 368));
            assert_eq!(win32::client_size(hwnd), (480, 368));
            assert_eq!(*state.resize_calls.borrow(), ["adjust", "set_size"]);
            unsafe { SendMessageW(hwnd, WM_CLOSE, 0, 0) };
            assert_eq!(manager.poll_events().len(), 1);
            assert!(!manager.is_open(key));
            assert_eq!(state.destroys.get(), expected_destroys);
            assert!(state.parent_alive_at_destroy.get());
        }
        assert!(manager.open(key, ptr, "Track teardown".into()));
        if let PluginGuiKey::Instrument { track_id } = key {
            manager.close_track_effects(track_id);
        }
        assert_eq!(state.destroys.get(), 3);
        assert!(manager.open(key, ptr, "App teardown".into()));
        drop(manager);
        assert_eq!(state.destroys.get(), 4);
        assert!(state.parent_alive_at_destroy.get());
    }
}

#[cfg(not(any(target_os = "macos", windows)))]
fn main() {}

#[cfg(target_os = "macos")]
unsafe fn parent_is_alive(parent: *mut std::ffi::c_void) -> bool {
    !parent.is_null()
        && (&*(parent as *const objc2_app_kit::NSView))
            .window()
            .is_some()
}

#[cfg(windows)]
mod win32 {
    use std::ffi::c_void;
    use windows_sys::Win32::Foundation::RECT;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        AdjustWindowRectEx, GetClientRect, GetWindowLongW, IsWindow, SetWindowPos, GWL_EXSTYLE,
        GWL_STYLE, SWP_NOMOVE, SWP_NOZORDER,
    };

    pub unsafe fn parent_is_alive(parent: *mut c_void) -> bool {
        !parent.is_null() && IsWindow(parent) != 0
    }

    pub fn client_size(hwnd: *mut c_void) -> (u32, u32) {
        let mut rect = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        unsafe { GetClientRect(hwnd, &mut rect) };
        (
            (rect.right - rect.left) as u32,
            (rect.bottom - rect.top) as u32,
        )
    }

    /// Stands in for the user dragging the window border.
    pub fn set_client_size(hwnd: *mut c_void, (width, height): (i32, i32)) {
        let mut rect = RECT {
            left: 0,
            top: 0,
            right: width,
            bottom: height,
        };
        unsafe {
            let style = GetWindowLongW(hwnd, GWL_STYLE) as u32;
            let ex_style = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
            AdjustWindowRectEx(&mut rect, style, 0, ex_style);
            SetWindowPos(
                hwnd,
                std::ptr::null_mut(),
                0,
                0,
                rect.right - rect.left,
                rect.bottom - rect.top,
                SWP_NOMOVE | SWP_NOZORDER,
            );
        }
    }
}
