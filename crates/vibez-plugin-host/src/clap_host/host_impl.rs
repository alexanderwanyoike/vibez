//! CLAP host callback routing and main-thread service registries.

use std::cell::Cell;
use std::ffi::CStr;
use std::os::raw::c_char;
use std::sync::{Mutex, OnceLock};
use std::thread::ThreadId;
use std::time::Instant;

use clap_sys::ext::gui::{clap_host_gui, CLAP_EXT_GUI};
// The FD flag constants are only read by poll_fds (Unix) and tests,
// so the Windows lib build sees them as unused.
#[cfg_attr(not(unix), allow(unused_imports))]
use clap_sys::ext::posix_fd_support::{
    clap_host_posix_fd_support, clap_posix_fd_flags, CLAP_EXT_POSIX_FD_SUPPORT,
    CLAP_POSIX_FD_ERROR, CLAP_POSIX_FD_READ, CLAP_POSIX_FD_WRITE,
};
use clap_sys::ext::thread_check::{clap_host_thread_check, CLAP_EXT_THREAD_CHECK};
use clap_sys::ext::timer_support::{clap_host_timer_support, CLAP_EXT_TIMER_SUPPORT};
use clap_sys::host::clap_host;
use clap_sys::plugin::clap_plugin;
use clap_sys::version::CLAP_VERSION;

/// Name exposed to plugins as the host name.
const HOST_NAME: &CStr = c"vibez";
const HOST_VENDOR: &CStr = c"vibez-daw";
const HOST_URL: &CStr = c"https://github.com/vibez-daw/vibez";
const HOST_VERSION: &CStr = c"0.1.0";

// ── Thread identity for CLAP_EXT_THREAD_CHECK ──

/// The thread ID of the CLAP main thread (UI thread). Set once at startup
/// by calling `set_clap_main_thread()`.
/// Before it's set, `is_main_thread()` returns true for any thread
/// (so `init()` on the loader thread won't trigger plugin assertions).
static CLAP_MAIN_THREAD_ID: OnceLock<ThreadId> = OnceLock::new();

thread_local! {
    /// Set on the callback thread immediately before the host invokes
    /// `clap_plugin.process()`. Thread-local state survives stream rebuilds
    /// without adding a lock to the realtime path.
    static IS_CLAP_AUDIO_THREAD: Cell<bool> = const { Cell::new(false) };
}

/// Register the current thread as the CLAP "main thread" for GUI operations.
/// Call this once from the UI thread at startup.
pub fn set_clap_main_thread() {
    let tid = std::thread::current().id();
    if CLAP_MAIN_THREAD_ID.set(tid).is_err() {
        eprintln!("vibez: set_clap_main_thread called more than once (ignored)");
    } else {
        eprintln!("vibez: CLAP main thread registered: {tid:?}");
    }
}

fn is_on_clap_main_thread() -> bool {
    CLAP_MAIN_THREAD_ID
        .get()
        .is_none_or(|id| *id == std::thread::current().id())
}

/// Register the current thread as a CLAP audio thread.
pub fn mark_clap_audio_thread() {
    IS_CLAP_AUDIO_THREAD.set(true);
}

fn is_on_clap_audio_thread() -> bool {
    IS_CLAP_AUDIO_THREAD.get()
}

// ── Per-plugin host data ──

/// Stored in `clap_host.host_data` — associates a host instance with its plugin.
/// This is needed so timer/FD callbacks can call back into the correct plugin.
pub struct ClapHostUserData {
    pub plugin_ptr: *const clap_plugin,
}

// Safety: Only accessed from the main thread (timer/fd/gui callbacks).
unsafe impl Send for ClapHostUserData {}
unsafe impl Sync for ClapHostUserData {}

/// Set the `host_data` on a leaked `clap_host` to point to a `ClapHostUserData`.
/// Must be called after `create_plugin()` returns, before `init()`.
///
/// # Safety
/// `host` must be a valid, leaked `clap_host` pointer. `plugin_ptr` must be valid.
pub unsafe fn set_host_user_data(host: &mut clap_host, plugin_ptr: *const clap_plugin) {
    let data = Box::leak(Box::new(ClapHostUserData { plugin_ptr }));
    host.host_data = data as *mut ClapHostUserData as *mut std::ffi::c_void;
}

/// Create a `clap_host` descriptor for the vibez host.
///
/// The returned struct has a static lifetime and is safe to pass to plugins.
pub fn make_clap_host() -> clap_host {
    clap_host {
        clap_version: CLAP_VERSION,
        host_data: std::ptr::null_mut(),
        name: HOST_NAME.as_ptr(),
        vendor: HOST_VENDOR.as_ptr(),
        url: HOST_URL.as_ptr(),
        version: HOST_VERSION.as_ptr(),
        get_extension: Some(host_get_extension),
        request_restart: Some(host_request_restart),
        request_process: Some(host_request_process),
        request_callback: Some(host_request_callback),
    }
}

// ── Timer support ──

struct TimerEntry {
    plugin_ptr: *const clap_plugin,
    timer_id: u32,
    period_ms: u32,
    last_fired: Instant,
}

// Safety: TimerEntry is only accessed from the main thread via poll_clap_events().
unsafe impl Send for TimerEntry {}

static NEXT_TIMER_ID: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(1);
static CLAP_TIMERS: Mutex<Vec<TimerEntry>> = Mutex::new(Vec::new());

static CLAP_HOST_TIMER_SUPPORT_IMPL: clap_host_timer_support = clap_host_timer_support {
    register_timer: Some(host_register_timer),
    unregister_timer: Some(host_unregister_timer),
};

unsafe extern "C" fn host_register_timer(
    host: *const clap_host,
    period_ms: u32,
    timer_id: *mut u32,
) -> bool {
    if host.is_null() || timer_id.is_null() {
        return false;
    }
    let host_ref = &*host;
    let plugin_ptr = if !host_ref.host_data.is_null() {
        let data = &*(host_ref.host_data as *const ClapHostUserData);
        data.plugin_ptr
    } else {
        return false;
    };

    let id = NEXT_TIMER_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    *timer_id = id;

    if let Ok(mut timers) = CLAP_TIMERS.lock() {
        timers.push(TimerEntry {
            plugin_ptr,
            timer_id: id,
            period_ms,
            last_fired: Instant::now(),
        });
    }

    eprintln!("vibez: register_timer(period={period_ms}ms) → id={id}");
    true
}

unsafe extern "C" fn host_unregister_timer(_host: *const clap_host, timer_id: u32) -> bool {
    if let Ok(mut timers) = CLAP_TIMERS.lock() {
        let before = timers.len();
        timers.retain(|t| t.timer_id != timer_id);
        let removed = before != timers.len();
        eprintln!("vibez: unregister_timer(id={timer_id}) → {removed}");
        removed
    } else {
        false
    }
}

// ── POSIX FD support ──

struct FdEntry {
    plugin_ptr: *const clap_plugin,
    fd: i32,
    flags: clap_posix_fd_flags,
}

// Safety: FdEntry is only accessed from the main thread via poll_clap_events().
unsafe impl Send for FdEntry {}

static CLAP_FDS: Mutex<Vec<FdEntry>> = Mutex::new(Vec::new());

static CLAP_HOST_POSIX_FD_SUPPORT_IMPL: clap_host_posix_fd_support = clap_host_posix_fd_support {
    register_fd: Some(host_register_fd),
    modify_fd: Some(host_modify_fd),
    unregister_fd: Some(host_unregister_fd),
};

unsafe extern "C" fn host_register_fd(
    host: *const clap_host,
    fd: i32,
    flags: clap_posix_fd_flags,
) -> bool {
    if host.is_null() {
        return false;
    }
    let host_ref = &*host;
    let plugin_ptr = if !host_ref.host_data.is_null() {
        let data = &*(host_ref.host_data as *const ClapHostUserData);
        data.plugin_ptr
    } else {
        return false;
    };

    if let Ok(mut fds) = CLAP_FDS.lock() {
        fds.push(FdEntry {
            plugin_ptr,
            fd,
            flags,
        });
    }

    eprintln!("vibez: register_fd(fd={fd}, flags={flags:#x})");
    true
}

unsafe extern "C" fn host_modify_fd(
    _host: *const clap_host,
    fd: i32,
    flags: clap_posix_fd_flags,
) -> bool {
    if let Ok(mut fds) = CLAP_FDS.lock() {
        if let Some(entry) = fds.iter_mut().find(|e| e.fd == fd) {
            entry.flags = flags;
            eprintln!("vibez: modify_fd(fd={fd}, flags={flags:#x})");
            return true;
        }
    }
    false
}

unsafe extern "C" fn host_unregister_fd(_host: *const clap_host, fd: i32) -> bool {
    if let Ok(mut fds) = CLAP_FDS.lock() {
        let before = fds.len();
        fds.retain(|e| e.fd != fd);
        let removed = before != fds.len();
        eprintln!("vibez: unregister_fd(fd={fd}) → {removed}");
        removed
    } else {
        false
    }
}

// ── Poll registered timers and FDs (call from UI tick) ──

/// Poll all registered CLAP timers and POSIX FDs.
/// Must be called on the main thread (e.g., from iced's 60fps tick).
/// Fires `on_timer` for elapsed timers and `on_fd` for ready file descriptors.
pub fn poll_clap_events() {
    poll_timers();
    poll_fds();
}

fn poll_timers() {
    let now = Instant::now();
    let Ok(mut timers) = CLAP_TIMERS.lock() else {
        return;
    };

    for timer in timers.iter_mut() {
        let elapsed_ms = now.duration_since(timer.last_fired).as_millis() as u32;
        if elapsed_ms >= timer.period_ms {
            timer.last_fired = now;
            let plugin_ptr = timer.plugin_ptr;
            let timer_id = timer.timer_id;

            // Get the plugin's timer extension
            let plugin_ref = unsafe { &*plugin_ptr };
            let ext_ptr = unsafe {
                (plugin_ref.get_extension.unwrap())(plugin_ptr, CLAP_EXT_TIMER_SUPPORT.as_ptr())
            }
                as *const clap_sys::ext::timer_support::clap_plugin_timer_support;

            if !ext_ptr.is_null() {
                let ext = unsafe { &*ext_ptr };
                unsafe { (ext.on_timer.unwrap())(plugin_ptr, timer_id) };
            }
        }
    }
}

/// CLAP posix-fd support is a Unix-only contract; there is no poll()
/// to link against on Windows, where registered fds simply never fire.
#[cfg(not(unix))]
fn poll_fds() {}

#[cfg(unix)]
fn poll_fds() {
    let Ok(fds) = CLAP_FDS.lock() else { return };

    for fd_entry in fds.iter() {
        // poll() with 0 timeout — non-blocking check
        let mut pfd = Pollfd {
            fd: fd_entry.fd,
            events: 0,
            revents: 0,
        };

        if fd_entry.flags & CLAP_POSIX_FD_READ != 0 {
            pfd.events |= POLLIN;
        }
        if fd_entry.flags & CLAP_POSIX_FD_WRITE != 0 {
            pfd.events |= POLLOUT;
        }
        if fd_entry.flags & CLAP_POSIX_FD_ERROR != 0 {
            pfd.events |= POLLERR;
        }

        let ret = unsafe { poll(&mut pfd, 1, 0) };
        if ret > 0 {
            let mut flags: clap_posix_fd_flags = 0;
            if pfd.revents & POLLIN != 0 {
                flags |= CLAP_POSIX_FD_READ;
            }
            if pfd.revents & POLLOUT != 0 {
                flags |= CLAP_POSIX_FD_WRITE;
            }
            if pfd.revents & POLLERR != 0 {
                flags |= CLAP_POSIX_FD_ERROR;
            }

            if flags != 0 {
                let plugin_ptr = fd_entry.plugin_ptr;
                let plugin_ref = unsafe { &*plugin_ptr };
                let ext_ptr = unsafe {
                    (plugin_ref.get_extension.unwrap())(
                        plugin_ptr,
                        CLAP_EXT_POSIX_FD_SUPPORT.as_ptr(),
                    )
                }
                    as *const clap_sys::ext::posix_fd_support::clap_plugin_posix_fd_support;

                if !ext_ptr.is_null() {
                    let ext = unsafe { &*ext_ptr };
                    unsafe { (ext.on_fd.unwrap())(plugin_ptr, fd_entry.fd, flags) };
                }
            }
        }
    }
}

// Minimal poll() FFI — avoids adding libc as a dependency
#[cfg(unix)]
#[repr(C)]
struct Pollfd {
    fd: i32,
    events: i16,
    revents: i16,
}

#[cfg(unix)]
const POLLIN: i16 = 0x001;
#[cfg(unix)]
const POLLOUT: i16 = 0x004;
#[cfg(unix)]
const POLLERR: i16 = 0x008;

#[cfg(unix)]
extern "C" {
    fn poll(fds: *mut Pollfd, nfds: u64, timeout: i32) -> i32;
}

// ── Host thread-check extension ──

static CLAP_HOST_THREAD_CHECK_IMPL: clap_host_thread_check = clap_host_thread_check {
    is_main_thread: Some(host_is_main_thread),
    is_audio_thread: Some(host_is_audio_thread),
};

unsafe extern "C" fn host_is_main_thread(_host: *const clap_host) -> bool {
    is_on_clap_main_thread()
}

unsafe extern "C" fn host_is_audio_thread(_host: *const clap_host) -> bool {
    is_on_clap_audio_thread()
}

// ── Host GUI extension (returned to plugins that query CLAP_EXT_GUI) ──

static CLAP_HOST_GUI_IMPL: clap_host_gui = clap_host_gui {
    resize_hints_changed: Some(host_gui_resize_hints_changed),
    request_resize: Some(host_gui_request_resize),
    request_show: Some(host_gui_request_show),
    request_hide: Some(host_gui_request_hide),
    closed: Some(host_gui_closed),
};

unsafe extern "C" fn host_gui_resize_hints_changed(_host: *const clap_host) {}

/// GUI resize requests from plugins, keyed by plugin pointer. The
/// plugin window manager drains this every tick and resizes the
/// actual X11 window; returning true without acting (the previous
/// behavior) made plugins render at a size the window never had.
static PENDING_GUI_RESIZES: Mutex<Vec<(usize, u32, u32)>> = Mutex::new(Vec::new());

/// Drain pending plugin-initiated GUI resizes: (plugin_ptr, w, h).
pub fn take_pending_gui_resizes() -> Vec<(usize, u32, u32)> {
    PENDING_GUI_RESIZES
        .lock()
        .map(|mut v| std::mem::take(&mut *v))
        .unwrap_or_default()
}

unsafe extern "C" fn host_gui_request_resize(
    host: *const clap_host,
    width: u32,
    height: u32,
) -> bool {
    if host.is_null() || (*host).host_data.is_null() {
        return false;
    }
    let data = &*((*host).host_data as *const ClapHostUserData);
    if let Ok(mut pending) = PENDING_GUI_RESIZES.lock() {
        pending.retain(|(p, _, _)| *p != data.plugin_ptr as usize);
        pending.push((data.plugin_ptr as usize, width, height));
    }
    true
}

unsafe extern "C" fn host_gui_request_show(_host: *const clap_host) -> bool {
    false
}

unsafe extern "C" fn host_gui_request_hide(_host: *const clap_host) -> bool {
    false
}

unsafe extern "C" fn host_gui_closed(_host: *const clap_host, _was_destroyed: bool) {}

// ── Core host callbacks ──

unsafe extern "C" fn host_get_extension(
    _host: *const clap_host,
    extension_id: *const c_char,
) -> *const std::ffi::c_void {
    if extension_id.is_null() {
        return std::ptr::null();
    }
    let ext_id = CStr::from_ptr(extension_id);
    let ext_name = ext_id.to_str().unwrap_or("?");

    if ext_id == CLAP_EXT_THREAD_CHECK {
        eprintln!("vibez: host_get_extension({ext_name}) → thread-check");
        return &CLAP_HOST_THREAD_CHECK_IMPL as *const clap_host_thread_check
            as *const std::ffi::c_void;
    }
    if ext_id == CLAP_EXT_GUI {
        eprintln!("vibez: host_get_extension({ext_name}) → gui");
        return &CLAP_HOST_GUI_IMPL as *const clap_host_gui as *const std::ffi::c_void;
    }
    if ext_id == CLAP_EXT_TIMER_SUPPORT {
        eprintln!("vibez: host_get_extension({ext_name}) → timer-support");
        return &CLAP_HOST_TIMER_SUPPORT_IMPL as *const clap_host_timer_support
            as *const std::ffi::c_void;
    }
    if ext_id == CLAP_EXT_POSIX_FD_SUPPORT {
        eprintln!("vibez: host_get_extension({ext_name}) → posix-fd-support");
        return &CLAP_HOST_POSIX_FD_SUPPORT_IMPL as *const clap_host_posix_fd_support
            as *const std::ffi::c_void;
    }

    eprintln!("vibez: host_get_extension({ext_name}) → null (not implemented)");
    std::ptr::null()
}

unsafe extern "C" fn host_request_restart(_host: *const clap_host) {
    // TODO: handle restart request from plugin
}

unsafe extern "C" fn host_request_process(_host: *const clap_host) {
    // TODO: handle process request from plugin
}

unsafe extern "C" fn host_request_callback(_host: *const clap_host) {
    // TODO: handle callback request from plugin
}

#[cfg(test)]
#[path = "host_impl_tests.rs"]
mod tests;
