//! Win32 top-level windows that host native plugin editors on Windows.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Once;

use vibez_plugin_host::gui::{GuiParent, PluginGuiKey};
use vibez_plugin_host::PluginGuiHandle;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AdjustWindowRectEx, CreateWindowExW, DefWindowProcW, DestroyWindow, GetClientRect,
    GetWindowLongW, IsIconic, LoadCursorW, RegisterClassExW, SetForegroundWindow, SetWindowLongW,
    SetWindowPos, ShowWindow, CW_USEDEFAULT, GWL_EXSTYLE, GWL_STYLE, IDC_ARROW, SWP_FRAMECHANGED,
    SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SW_RESTORE, SW_SHOW, WM_CLOSE,
    WNDCLASSEXW, WS_CAPTION, WS_CLIPCHILDREN, WS_MAXIMIZEBOX, WS_MINIMIZEBOX, WS_OVERLAPPED,
    WS_SYSMENU, WS_THICKFRAME,
};

use super::{gui_handle, requested_resize, PluginRawPtr, PluginWindowEvent};

const CLASS_NAME: windows_sys::core::PCWSTR = windows_sys::core::w!("VibezPlugin");
const FIXED_STYLE: u32 = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX | WS_CLIPCHILDREN;
const RESIZABLE_STYLE: u32 = FIXED_STYLE | WS_THICKFRAME | WS_MAXIMIZEBOX;

thread_local! {
    /// Windows whose X button was pressed since the last poll. Plugin
    /// windows live on the UI thread, which is the only thread the
    /// window procedure runs on.
    static CLOSE_REQUESTS: RefCell<Vec<usize>> = const { RefCell::new(Vec::new()) };
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if msg == WM_CLOSE {
        // Keep the parent alive until the UI tick detaches the plugin.
        CLOSE_REQUESTS.with(|r| r.borrow_mut().push(hwnd as usize));
        return 0;
    }
    DefWindowProcW(hwnd, msg, wparam, lparam)
}

fn register_class() {
    static REGISTER: Once = Once::new();
    REGISTER.call_once(|| unsafe {
        let class = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: 0,
            lpfnWndProc: Some(window_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: GetModuleHandleW(std::ptr::null()),
            hIcon: std::ptr::null_mut(),
            hCursor: LoadCursorW(std::ptr::null_mut(), IDC_ARROW),
            hbrBackground: std::ptr::null_mut(),
            lpszMenuName: std::ptr::null(),
            lpszClassName: CLASS_NAME,
            hIconSm: std::ptr::null_mut(),
        };
        RegisterClassExW(&class);
    });
}

struct OpenPluginWindow {
    hwnd: HWND,
    gui_handle: PluginGuiHandle,
    size: (u32, u32),
}

impl Drop for OpenPluginWindow {
    fn drop(&mut self) {
        self.gui_handle.hide();
        self.gui_handle.destroy();
        unsafe { DestroyWindow(self.hwnd) };
        // HWNDs are reused, so a stale request must not close a later window.
        let hwnd = self.hwnd as usize;
        CLOSE_REQUESTS.with(|r| r.borrow_mut().retain(|&h| h != hwnd));
    }
}

pub struct PluginWindowManager {
    windows: HashMap<PluginGuiKey, OpenPluginWindow>,
}

impl PluginWindowManager {
    pub fn new() -> Option<Self> {
        register_class();
        Some(Self {
            windows: HashMap::new(),
        })
    }

    pub fn open(&mut self, key: PluginGuiKey, raw_ptr: PluginRawPtr, title: String) -> bool {
        if self.is_open(key) {
            self.raise(key);
            return true;
        }
        let Some(mut gui_handle) = (unsafe { gui_handle(raw_ptr) }) else {
            return false;
        };
        if !gui_handle.create_gui() {
            eprintln!("vibez: plugin GUI create() failed");
            return false;
        }
        let size = gui_handle.get_size().unwrap_or((800, 600));
        let title: Vec<u16> = title.encode_utf16().chain(Some(0)).collect();
        let hwnd = unsafe {
            CreateWindowExW(
                0,
                CLASS_NAME,
                title.as_ptr(),
                FIXED_STYLE,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                GetModuleHandleW(std::ptr::null()),
                std::ptr::null(),
            )
        };
        if hwnd.is_null() {
            eprintln!("vibez: failed to create Win32 plugin window");
            return false;
        }
        set_client_size(hwnd, size);
        let mut opened = OpenPluginWindow {
            hwnd,
            gui_handle,
            size,
        };
        if !unsafe { opened.gui_handle.open_embedded(GuiParent::Win32(hwnd)) } {
            eprintln!("vibez: open_window - plugin rejected its parent window");
            return false;
        }
        // Some plugins only know their real size once attached.
        opened.size = opened.gui_handle.get_size().unwrap_or(size);
        if opened.gui_handle.can_resize() {
            unsafe {
                SetWindowLongW(hwnd, GWL_STYLE, RESIZABLE_STYLE as i32);
                SetWindowPos(
                    hwnd,
                    std::ptr::null_mut(),
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
                );
            }
        }
        set_client_size(hwnd, opened.size);
        unsafe {
            ShowWindow(hwnd, SW_SHOW);
            SetForegroundWindow(hwnd);
        }
        self.windows.insert(key, opened);
        true
    }

    pub fn close(&mut self, key: PluginGuiKey) {
        self.windows.remove(&key);
    }

    pub fn raise(&self, key: PluginGuiKey) {
        if let Some(opened) = self.windows.get(&key) {
            unsafe {
                if IsIconic(opened.hwnd) != 0 {
                    ShowWindow(opened.hwnd, SW_RESTORE);
                }
                SetForegroundWindow(opened.hwnd);
            }
        }
    }

    pub fn close_all(&mut self) {
        self.windows.clear();
    }

    pub fn is_open(&self, key: PluginGuiKey) -> bool {
        self.windows.contains_key(&key)
    }

    pub fn close_track_effects(&mut self, track_id: vibez_core::id::TrackId) {
        self.windows.retain(|key, _| key.track_id() != track_id);
    }

    pub fn poll_events(&mut self) -> Vec<PluginWindowEvent> {
        let requests = CLOSE_REQUESTS.with(|r| std::mem::take(&mut *r.borrow_mut()));
        let closed: Vec<_> = self
            .windows
            .iter()
            .filter(|(_, w)| requests.contains(&(w.hwnd as usize)))
            .map(|(key, _)| *key)
            .collect();
        let mut events = Vec::new();
        for key in closed {
            self.close(key);
            events.push(PluginWindowEvent::Closed(key));
        }
        let clap_requests = vibez_plugin_host::clap_host::host_impl::take_pending_gui_resizes();
        for opened in self.windows.values_mut() {
            let requested = requested_resize(&opened.gui_handle, &clap_requests);
            if let Some(size) = requested.filter(|(w, h)| *w > 0 && *h > 0) {
                opened.size = size;
                set_client_size(opened.hwnd, size);
            }
            let size = client_size(opened.hwnd);
            if size != opened.size && size.0 > 0 && size.1 > 0 {
                if let Some(adjusted) = opened.gui_handle.resize_from_host(size.0, size.1) {
                    opened.size = adjusted;
                }
                if size != opened.size {
                    set_client_size(opened.hwnd, opened.size);
                }
            }
        }
        events
    }
}

fn set_client_size(hwnd: HWND, size: (u32, u32)) {
    let (style, ex_style) = unsafe {
        (
            GetWindowLongW(hwnd, GWL_STYLE) as u32,
            GetWindowLongW(hwnd, GWL_EXSTYLE) as u32,
        )
    };
    let mut rect = RECT {
        left: 0,
        top: 0,
        right: size.0 as i32,
        bottom: size.1 as i32,
    };
    unsafe {
        AdjustWindowRectEx(&mut rect, style, 0, ex_style);
        SetWindowPos(
            hwnd,
            std::ptr::null_mut(),
            0,
            0,
            rect.right - rect.left,
            rect.bottom - rect.top,
            SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
        );
    }
}

fn client_size(hwnd: HWND) -> (u32, u32) {
    let mut rect = RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    unsafe { GetClientRect(hwnd, &mut rect) };
    (
        (rect.right - rect.left).max(0) as u32,
        (rect.bottom - rect.top).max(0) as u32,
    )
}
