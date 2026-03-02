use std::ffi::CStr;
use std::os::raw::c_char;
use std::sync::{Mutex, OnceLock};
use std::thread::ThreadId;
use std::time::Instant;

use clap_sys::ext::gui::{clap_host_gui, CLAP_EXT_GUI};
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
                (plugin_ref.get_extension.unwrap())(
                    plugin_ptr,
                    CLAP_EXT_TIMER_SUPPORT.as_ptr(),
                )
            } as *const clap_sys::ext::timer_support::clap_plugin_timer_support;

            if !ext_ptr.is_null() {
                let ext = unsafe { &*ext_ptr };
                unsafe { (ext.on_timer.unwrap())(plugin_ptr, timer_id) };
            }
        }
    }
}

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
                } as *const clap_sys::ext::posix_fd_support::clap_plugin_posix_fd_support;

                if !ext_ptr.is_null() {
                    let ext = unsafe { &*ext_ptr };
                    unsafe { (ext.on_fd.unwrap())(plugin_ptr, fd_entry.fd, flags) };
                }
            }
        }
    }
}

// Minimal poll() FFI — avoids adding libc as a dependency
#[repr(C)]
struct Pollfd {
    fd: i32,
    events: i16,
    revents: i16,
}

const POLLIN: i16 = 0x001;
const POLLOUT: i16 = 0x004;
const POLLERR: i16 = 0x008;

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
    // We don't track the audio thread identity yet — return false as a safe default.
    // Plugins will just skip their audio-thread assertions.
    false
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

unsafe extern "C" fn host_gui_request_resize(
    _host: *const clap_host,
    _width: u32,
    _height: u32,
) -> bool {
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
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    // ── Fake plugin infrastructure ──

    // Counters incremented by fake plugin callbacks
    static TEST_TIMER_CALL_COUNT: AtomicU32 = AtomicU32::new(0);
    static TEST_FD_CALL_COUNT: AtomicU32 = AtomicU32::new(0);

    static TEST_PLUGIN_TIMER_EXT: clap_sys::ext::timer_support::clap_plugin_timer_support =
        clap_sys::ext::timer_support::clap_plugin_timer_support {
            on_timer: Some(test_on_timer),
        };

    static TEST_PLUGIN_FD_EXT: clap_sys::ext::posix_fd_support::clap_plugin_posix_fd_support =
        clap_sys::ext::posix_fd_support::clap_plugin_posix_fd_support {
            on_fd: Some(test_on_fd),
        };

    unsafe extern "C" fn test_on_timer(_plugin: *const clap_plugin, _timer_id: u32) {
        TEST_TIMER_CALL_COUNT.fetch_add(1, Ordering::Relaxed);
    }

    unsafe extern "C" fn test_on_fd(
        _plugin: *const clap_plugin,
        _fd: i32,
        _flags: clap_posix_fd_flags,
    ) {
        TEST_FD_CALL_COUNT.fetch_add(1, Ordering::Relaxed);
    }

    unsafe extern "C" fn test_get_extension(
        _plugin: *const clap_plugin,
        extension_id: *const c_char,
    ) -> *const std::ffi::c_void {
        let ext_id = CStr::from_ptr(extension_id);
        if ext_id == CLAP_EXT_TIMER_SUPPORT {
            return &TEST_PLUGIN_TIMER_EXT
                as *const clap_sys::ext::timer_support::clap_plugin_timer_support
                as *const std::ffi::c_void;
        }
        if ext_id == CLAP_EXT_POSIX_FD_SUPPORT {
            return &TEST_PLUGIN_FD_EXT
                as *const clap_sys::ext::posix_fd_support::clap_plugin_posix_fd_support
                as *const std::ffi::c_void;
        }
        std::ptr::null()
    }

    /// Create a fake `clap_plugin` with only `get_extension` implemented.
    fn make_test_plugin() -> clap_plugin {
        clap_plugin {
            desc: std::ptr::null(),
            plugin_data: std::ptr::null_mut(),
            init: None,
            destroy: None,
            activate: None,
            deactivate: None,
            start_processing: None,
            stop_processing: None,
            reset: None,
            process: None,
            get_extension: Some(test_get_extension),
            on_main_thread: None,
        }
    }

    /// Create a `clap_host` with `host_data` pointing to a fake plugin.
    fn make_test_host_with_plugin(plugin_ptr: *const clap_plugin) -> clap_host {
        let mut host = make_clap_host();
        let data = Box::leak(Box::new(ClapHostUserData { plugin_ptr }));
        host.host_data = data as *mut ClapHostUserData as *mut std::ffi::c_void;
        host
    }

    /// Clear the timer and FD registries (for test isolation).
    fn clear_registries() {
        CLAP_TIMERS.lock().unwrap().clear();
        CLAP_FDS.lock().unwrap().clear();
    }

    // ── make_clap_host tests ──

    #[test]
    fn test_make_clap_host_fields() {
        let host = make_clap_host();
        assert_eq!(host.clap_version, CLAP_VERSION);
        assert!(host.get_extension.is_some());
        assert!(host.request_restart.is_some());
        assert!(host.request_process.is_some());
        assert!(host.request_callback.is_some());
        assert!(host.host_data.is_null());

        // Verify name string
        let name = unsafe { CStr::from_ptr(host.name) };
        assert_eq!(name, c"vibez");
    }

    // ── host_get_extension tests ──

    #[test]
    fn test_host_get_extension_returns_thread_check() {
        let host = make_clap_host();
        let ptr = unsafe {
            host_get_extension(&host, CLAP_EXT_THREAD_CHECK.as_ptr())
        };
        assert!(!ptr.is_null());
    }

    #[test]
    fn test_host_get_extension_returns_gui() {
        let host = make_clap_host();
        let ptr = unsafe { host_get_extension(&host, CLAP_EXT_GUI.as_ptr()) };
        assert!(!ptr.is_null());
    }

    #[test]
    fn test_host_get_extension_returns_timer_support() {
        let host = make_clap_host();
        let ptr = unsafe {
            host_get_extension(&host, CLAP_EXT_TIMER_SUPPORT.as_ptr())
        };
        assert!(!ptr.is_null());
    }

    #[test]
    fn test_host_get_extension_returns_posix_fd_support() {
        let host = make_clap_host();
        let ptr = unsafe {
            host_get_extension(&host, CLAP_EXT_POSIX_FD_SUPPORT.as_ptr())
        };
        assert!(!ptr.is_null());
    }

    #[test]
    fn test_host_get_extension_returns_null_for_unknown() {
        let host = make_clap_host();
        let unknown = c"clap.unknown-extension";
        let ptr = unsafe { host_get_extension(&host, unknown.as_ptr()) };
        assert!(ptr.is_null());
    }

    #[test]
    fn test_host_get_extension_returns_null_for_null_id() {
        let host = make_clap_host();
        let ptr = unsafe { host_get_extension(&host, std::ptr::null()) };
        assert!(ptr.is_null());
    }

    // ── set_host_user_data tests ──

    #[test]
    fn test_set_host_user_data() {
        let plugin = make_test_plugin();
        let mut host = make_clap_host();
        assert!(host.host_data.is_null());

        unsafe { set_host_user_data(&mut host, &plugin as *const _) };

        assert!(!host.host_data.is_null());
        let data = unsafe { &*(host.host_data as *const ClapHostUserData) };
        assert_eq!(data.plugin_ptr, &plugin as *const _);
    }

    // ── Timer register/unregister tests ──

    #[test]
    fn test_timer_register_unregister() {
        clear_registries();

        let plugin = make_test_plugin();
        let host = make_test_host_with_plugin(&plugin);

        let mut timer_id: u32 = 0;
        let ok = unsafe { host_register_timer(&host, 100, &mut timer_id) };
        assert!(ok);
        assert_ne!(timer_id, 0);

        // Verify timer is in registry
        {
            let timers = CLAP_TIMERS.lock().unwrap();
            assert_eq!(timers.len(), 1);
            assert_eq!(timers[0].timer_id, timer_id);
            assert_eq!(timers[0].period_ms, 100);
        }

        // Unregister
        let ok = unsafe { host_unregister_timer(&host, timer_id) };
        assert!(ok);

        {
            let timers = CLAP_TIMERS.lock().unwrap();
            assert_eq!(timers.len(), 0);
        }

        clear_registries();
    }

    #[test]
    fn test_timer_register_assigns_unique_ids() {
        clear_registries();

        let plugin = make_test_plugin();
        let host = make_test_host_with_plugin(&plugin);

        let mut id1: u32 = 0;
        let mut id2: u32 = 0;
        let mut id3: u32 = 0;

        unsafe {
            host_register_timer(&host, 50, &mut id1);
            host_register_timer(&host, 100, &mut id2);
            host_register_timer(&host, 200, &mut id3);
        }

        assert_ne!(id1, id2);
        assert_ne!(id2, id3);
        assert_ne!(id1, id3);

        {
            let timers = CLAP_TIMERS.lock().unwrap();
            assert_eq!(timers.len(), 3);
        }

        clear_registries();
    }

    #[test]
    fn test_unregister_nonexistent_timer() {
        clear_registries();

        let host = make_clap_host();
        let ok = unsafe { host_unregister_timer(&host, 99999) };
        assert!(!ok);

        clear_registries();
    }

    #[test]
    fn test_timer_register_fails_without_host_data() {
        clear_registries();

        let host = make_clap_host(); // host_data is null
        let mut timer_id: u32 = 0;
        let ok = unsafe { host_register_timer(&host, 100, &mut timer_id) };
        assert!(!ok);

        clear_registries();
    }

    // ── FD register/modify/unregister tests ──

    #[test]
    fn test_fd_register_unregister() {
        clear_registries();

        let plugin = make_test_plugin();
        let host = make_test_host_with_plugin(&plugin);

        let ok = unsafe { host_register_fd(&host, 42, CLAP_POSIX_FD_READ) };
        assert!(ok);

        {
            let fds = CLAP_FDS.lock().unwrap();
            assert_eq!(fds.len(), 1);
            assert_eq!(fds[0].fd, 42);
            assert_eq!(fds[0].flags, CLAP_POSIX_FD_READ);
        }

        let ok = unsafe { host_unregister_fd(&host, 42) };
        assert!(ok);

        {
            let fds = CLAP_FDS.lock().unwrap();
            assert_eq!(fds.len(), 0);
        }

        clear_registries();
    }

    #[test]
    fn test_fd_modify() {
        clear_registries();

        let plugin = make_test_plugin();
        let host = make_test_host_with_plugin(&plugin);

        unsafe { host_register_fd(&host, 10, CLAP_POSIX_FD_READ) };

        let ok = unsafe {
            host_modify_fd(&host, 10, CLAP_POSIX_FD_READ | CLAP_POSIX_FD_WRITE)
        };
        assert!(ok);

        {
            let fds = CLAP_FDS.lock().unwrap();
            assert_eq!(fds[0].flags, CLAP_POSIX_FD_READ | CLAP_POSIX_FD_WRITE);
        }

        clear_registries();
    }

    #[test]
    fn test_fd_modify_nonexistent() {
        clear_registries();

        let host = make_clap_host();
        let ok = unsafe { host_modify_fd(&host, 999, CLAP_POSIX_FD_READ) };
        assert!(!ok);

        clear_registries();
    }

    #[test]
    fn test_fd_register_fails_without_host_data() {
        clear_registries();

        let host = make_clap_host(); // host_data is null
        let ok = unsafe { host_register_fd(&host, 5, CLAP_POSIX_FD_READ) };
        assert!(!ok);

        clear_registries();
    }

    // ── Timer polling tests ──

    #[test]
    fn test_poll_timers_fires_elapsed() {
        clear_registries();
        TEST_TIMER_CALL_COUNT.store(0, Ordering::Relaxed);

        let plugin = make_test_plugin();
        let plugin_ptr: *const clap_plugin = &plugin;

        // Manually insert a timer with last_fired in the past
        {
            let mut timers = CLAP_TIMERS.lock().unwrap();
            timers.push(TimerEntry {
                plugin_ptr,
                timer_id: 1,
                period_ms: 0, // fire immediately
                last_fired: Instant::now() - std::time::Duration::from_secs(1),
            });
        }

        poll_timers();

        assert!(TEST_TIMER_CALL_COUNT.load(Ordering::Relaxed) >= 1);

        clear_registries();
    }

    #[test]
    fn test_poll_timers_skips_not_elapsed() {
        clear_registries();
        TEST_TIMER_CALL_COUNT.store(0, Ordering::Relaxed);

        let plugin = make_test_plugin();
        let plugin_ptr: *const clap_plugin = &plugin;

        // Timer with very long period — should NOT fire
        {
            let mut timers = CLAP_TIMERS.lock().unwrap();
            timers.push(TimerEntry {
                plugin_ptr,
                timer_id: 2,
                period_ms: 999_999,
                last_fired: Instant::now(),
            });
        }

        poll_timers();

        assert_eq!(TEST_TIMER_CALL_COUNT.load(Ordering::Relaxed), 0);

        clear_registries();
    }

    // ── FD polling tests ──

    #[test]
    fn test_poll_fds_fires_on_ready_pipe() {
        clear_registries();
        TEST_FD_CALL_COUNT.store(0, Ordering::Relaxed);

        let plugin = make_test_plugin();
        let plugin_ptr: *const clap_plugin = &plugin;

        // Create a pipe — write end makes read end ready
        let mut fds = [0i32; 2];
        let ret = unsafe { libc_pipe(fds.as_mut_ptr()) };
        assert_eq!(ret, 0);

        let read_fd = fds[0];
        let write_fd = fds[1];

        // Register read end with the CLAP FD registry
        {
            let mut fd_entries = CLAP_FDS.lock().unwrap();
            fd_entries.push(FdEntry {
                plugin_ptr,
                fd: read_fd,
                flags: CLAP_POSIX_FD_READ,
            });
        }

        // Write a byte to make read end ready
        let byte = [0x42u8];
        unsafe { libc_write(write_fd, byte.as_ptr() as *const std::ffi::c_void, 1) };

        // Poll — should fire on_fd
        poll_fds();

        assert!(TEST_FD_CALL_COUNT.load(Ordering::Relaxed) >= 1);

        // Cleanup
        unsafe {
            libc_close(read_fd);
            libc_close(write_fd);
        }
        clear_registries();
    }

    #[test]
    fn test_poll_fds_does_not_fire_empty_pipe() {
        clear_registries();
        TEST_FD_CALL_COUNT.store(0, Ordering::Relaxed);

        let plugin = make_test_plugin();
        let plugin_ptr: *const clap_plugin = &plugin;

        // Create a pipe — don't write anything
        let mut fds = [0i32; 2];
        let ret = unsafe { libc_pipe(fds.as_mut_ptr()) };
        assert_eq!(ret, 0);

        let read_fd = fds[0];
        let write_fd = fds[1];

        {
            let mut fd_entries = CLAP_FDS.lock().unwrap();
            fd_entries.push(FdEntry {
                plugin_ptr,
                fd: read_fd,
                flags: CLAP_POSIX_FD_READ,
            });
        }

        // Poll — should NOT fire (nothing to read)
        poll_fds();

        assert_eq!(TEST_FD_CALL_COUNT.load(Ordering::Relaxed), 0);

        unsafe {
            libc_close(read_fd);
            libc_close(write_fd);
        }
        clear_registries();
    }

    // ── Thread identity tests ──

    #[test]
    fn test_is_on_clap_main_thread_default() {
        // Before set_clap_main_thread is called (or if OnceLock not set for
        // this test), any thread should be considered the main thread.
        // Note: If another test already called set_clap_main_thread in this
        // process, this may return true or false depending on which thread
        // we're on. We test the function doesn't panic.
        let _ = is_on_clap_main_thread();
    }

    // ── Minimal libc FFI for pipe tests ──

    extern "C" {
        #[link_name = "pipe"]
        fn libc_pipe(pipefd: *mut i32) -> i32;
        #[link_name = "write"]
        fn libc_write(fd: i32, buf: *const std::ffi::c_void, count: usize) -> isize;
        #[link_name = "close"]
        fn libc_close(fd: i32) -> i32;
    }
}
