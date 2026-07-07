//! Host-side IPlugFrame + Linux IRunLoop for VST3 plugin GUIs.
//!
//! On Linux the VST3 GUI contract requires the host to provide an
//! event-loop service: plugins register file-descriptor handlers and
//! timers with the IRunLoop they query from the IPlugFrame passed to
//! IPlugView::setFrame. JUCE-based editors (ZL, Vital) do all their
//! repainting and input dispatch from those callbacks; without a run
//! loop the editor draws once and never responds.
//!
//! One COM object implements both interfaces: IPlugFrame at offset 0
//! and IRunLoop as an embedded subobject one pointer in. Registered
//! handlers land in a shared registry which the plugin window manager
//! services from its UI-thread poll tick.

use std::ffi::c_void;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

// FUnknown IID: {00000000-0000-0000-C000-000000000046}
const FUNKNOWN_IID: [u8; 16] = [
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xC0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46,
];

// IPlugFrame IID: {367FAF01-AFA9-4693-8D4D-A2A0ED0882A3}
pub(crate) const IPLUGFRAME_IID: [u8; 16] = [
    0x36, 0x7F, 0xAF, 0x01, 0xAF, 0xA9, 0x46, 0x93, 0x8D, 0x4D, 0xA2, 0xA0, 0xED, 0x08, 0x82, 0xA3,
];

// Linux IRunLoop IID: {18C35366-9776-4F1A-9C5B-83857A871389}
pub(crate) const IRUNLOOP_IID: [u8; 16] = [
    0x18, 0xC3, 0x53, 0x66, 0x97, 0x76, 0x4F, 0x1A, 0x9C, 0x5B, 0x83, 0x85, 0x7A, 0x87, 0x13, 0x89,
];

const K_RESULT_OK: i32 = 0;
const K_NO_INTERFACE: i32 = -1;
const K_INVALID_ARGUMENT: i32 = 2;

/// FUnknown vtable slots shared by plugin-side handler objects.
type HandlerReleaseFn = unsafe extern "system" fn(*mut c_void) -> u32;
type HandlerAddRefFn = unsafe extern "system" fn(*mut c_void) -> u32;
/// IEventHandler::onFDIsSet(fd) / ITimerHandler::onTimer() - vtable [3]
type OnFdIsSetFn = unsafe extern "system" fn(*mut c_void, i32);
type OnTimerFn = unsafe extern "system" fn(*mut c_void);

unsafe fn handler_vtbl_slot(obj: *mut c_void, slot: usize) -> *const c_void {
    let vtbl = *(obj as *const *const *const c_void);
    *vtbl.add(slot)
}

unsafe fn handler_add_ref(obj: *mut c_void) {
    let f: HandlerAddRefFn = std::mem::transmute(handler_vtbl_slot(obj, 1));
    f(obj);
}

unsafe fn handler_release(obj: *mut c_void) {
    let f: HandlerReleaseFn = std::mem::transmute(handler_vtbl_slot(obj, 2));
    f(obj);
}

struct EventHandlerEntry {
    handler: *mut c_void,
    fd: i32,
}

struct TimerEntry {
    handler: *mut c_void,
    interval: Duration,
    next_fire: Instant,
}

// Safety: handlers are registered and serviced on the UI thread only;
// Send is required because the registry travels inside Arc<Mutex<..>>.
unsafe impl Send for EventHandlerEntry {}
unsafe impl Send for TimerEntry {}

/// Handlers a plugin registered with our run loop.
#[derive(Default)]
pub struct RunLoopRegistry {
    event_handlers: Vec<EventHandlerEntry>,
    timers: Vec<TimerEntry>,
    /// Plugin-initiated view resize (IPlugFrame::resizeView), for the
    /// window manager to apply to the host window.
    pending_resize: Option<(u32, u32)>,
}

impl RunLoopRegistry {
    /// Fire due timers and ready file descriptors. UI thread only.
    fn service(&mut self) {
        let now = Instant::now();
        // Collect first: a callback may re-enter register/unregister
        // through the same mutex if invoked while iterating.
        let due_timers: Vec<*mut c_void> = self
            .timers
            .iter_mut()
            .filter(|t| now >= t.next_fire)
            .map(|t| {
                t.next_fire = now + t.interval;
                t.handler
            })
            .collect();

        let mut ready_fds: Vec<(*mut c_void, i32)> = Vec::new();
        if !self.event_handlers.is_empty() {
            let mut pollfds: Vec<libc::pollfd> = self
                .event_handlers
                .iter()
                .map(|e| libc::pollfd {
                    fd: e.fd,
                    events: libc::POLLIN,
                    revents: 0,
                })
                .collect();
            let rc = unsafe { libc::poll(pollfds.as_mut_ptr(), pollfds.len() as libc::nfds_t, 0) };
            if rc > 0 {
                for (entry, pfd) in self.event_handlers.iter().zip(&pollfds) {
                    if pfd.revents & (libc::POLLIN | libc::POLLHUP | libc::POLLERR) != 0 {
                        ready_fds.push((entry.handler, entry.fd));
                    }
                }
            }
        }

        for handler in due_timers {
            unsafe {
                let on_timer: OnTimerFn = std::mem::transmute(handler_vtbl_slot(handler, 3));
                on_timer(handler);
            }
        }
        for (handler, fd) in ready_fds {
            unsafe {
                let on_fd: OnFdIsSetFn = std::mem::transmute(handler_vtbl_slot(handler, 3));
                on_fd(handler, fd);
            }
        }
    }
}

/// Service a registry outside the lock held during callbacks:
/// take the work under the lock, run callbacks after releasing it.
pub fn service_registry(registry: &Mutex<RunLoopRegistry>) {
    // Callbacks (JUCE paint/input) may call back into
    // register/unregister; a held lock would deadlock. Swap the
    // registry out, service it, merge re-registrations back.
    let mut taken = {
        let Ok(mut guard) = registry.lock() else {
            return;
        };
        std::mem::take(&mut *guard)
    };
    taken.service();
    if let Ok(mut guard) = registry.lock() {
        // Anything the callbacks registered while we serviced.
        let added = std::mem::take(&mut *guard);
        *guard = taken;
        guard.event_handlers.extend(added.event_handlers);
        guard.timers.extend(added.timers);
    }
}

#[repr(C)]
struct FrameVtbl {
    query_interface: unsafe extern "system" fn(*mut Frame, *const u8, *mut *mut c_void) -> i32,
    add_ref: unsafe extern "system" fn(*mut Frame) -> u32,
    release: unsafe extern "system" fn(*mut Frame) -> u32,
    /// IPlugFrame::resizeView(view, newSize)
    resize_view: unsafe extern "system" fn(*mut Frame, *mut c_void, *mut c_void) -> i32,
}

#[repr(C)]
struct RunLoopVtbl {
    query_interface: unsafe extern "system" fn(*mut c_void, *const u8, *mut *mut c_void) -> i32,
    add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
    register_event_handler: unsafe extern "system" fn(*mut c_void, *mut c_void, i32) -> i32,
    unregister_event_handler: unsafe extern "system" fn(*mut c_void, *mut c_void) -> i32,
    register_timer: unsafe extern "system" fn(*mut c_void, *mut c_void, u64) -> i32,
    unregister_timer: unsafe extern "system" fn(*mut c_void, *mut c_void) -> i32,
}

static FRAME_VTBL: FrameVtbl = FrameVtbl {
    query_interface: frame_query_interface,
    add_ref: frame_add_ref,
    release: frame_release,
    resize_view,
};

static RUNLOOP_VTBL: RunLoopVtbl = RunLoopVtbl {
    query_interface: rl_query_interface,
    add_ref: rl_add_ref,
    release: rl_release,
    register_event_handler,
    unregister_event_handler,
    register_timer,
    unregister_timer,
};

#[repr(C)]
struct Frame {
    frame_vtbl: *const FrameVtbl,
    /// IRunLoop subobject: a pointer-sized slot whose address is the
    /// COM pointer handed out for IRunLoop queries.
    runloop_vtbl: *const RunLoopVtbl,
    refcount: AtomicU32,
    registry: *const Mutex<RunLoopRegistry>,
}

const RUNLOOP_OFFSET: usize = std::mem::size_of::<*const c_void>();

unsafe fn frame_from_runloop(this: *mut c_void) -> *mut Frame {
    (this as *mut u8).sub(RUNLOOP_OFFSET) as *mut Frame
}

unsafe fn qi_common(frame: *mut Frame, iid: *const u8, obj: *mut *mut c_void) -> i32 {
    let iid = std::slice::from_raw_parts(iid, 16);
    if iid == FUNKNOWN_IID || iid == IPLUGFRAME_IID {
        frame_add_ref(frame);
        *obj = frame as *mut c_void;
        K_RESULT_OK
    } else if iid == IRUNLOOP_IID {
        frame_add_ref(frame);
        *obj = (frame as *mut u8).add(RUNLOOP_OFFSET) as *mut c_void;
        K_RESULT_OK
    } else {
        *obj = std::ptr::null_mut();
        K_NO_INTERFACE
    }
}

unsafe extern "system" fn frame_query_interface(
    this: *mut Frame,
    iid: *const u8,
    obj: *mut *mut c_void,
) -> i32 {
    qi_common(this, iid, obj)
}

unsafe extern "system" fn frame_add_ref(this: *mut Frame) -> u32 {
    (*this).refcount.fetch_add(1, Ordering::AcqRel) + 1
}

unsafe extern "system" fn frame_release(this: *mut Frame) -> u32 {
    let remaining = (*this).refcount.fetch_sub(1, Ordering::AcqRel) - 1;
    if remaining == 0 {
        // Release any handlers the plugin failed to unregister, then
        // drop our Arc reference to the registry.
        let registry = Arc::from_raw((*this).registry);
        if let Ok(mut reg) = registry.lock() {
            for e in reg.event_handlers.drain(..) {
                handler_release(e.handler);
            }
            for t in reg.timers.drain(..) {
                handler_release(t.handler);
            }
        }
        drop(registry);
        drop(Box::from_raw(this));
    }
    remaining
}

unsafe extern "system" fn resize_view(
    this: *mut Frame,
    view: *mut c_void,
    rect: *mut c_void,
) -> i32 {
    if view.is_null() || rect.is_null() {
        return K_INVALID_ARGUMENT;
    }
    #[repr(C)]
    struct ViewRect {
        left: i32,
        top: i32,
        right: i32,
        bottom: i32,
    }
    // Record the requested size for the window manager to apply to
    // the host window, then accept via IPlugView::onSize (vtable
    // [10]) so the view lays out for what it asked.
    let r = &*(rect as *const ViewRect);
    let w = (r.right - r.left).max(0) as u32;
    let h = (r.bottom - r.top).max(0) as u32;
    if w > 0 && h > 0 {
        if let Ok(mut reg) = (*(*this).registry).lock() {
            reg.pending_resize = Some((w, h));
        }
    }
    type OnSizeFn = unsafe extern "system" fn(*mut c_void, *mut c_void) -> i32;
    let vtbl = *(view as *const *const *const c_void);
    let on_size: OnSizeFn = std::mem::transmute(*vtbl.add(10));
    on_size(view, rect)
}

unsafe extern "system" fn rl_query_interface(
    this: *mut c_void,
    iid: *const u8,
    obj: *mut *mut c_void,
) -> i32 {
    qi_common(frame_from_runloop(this), iid, obj)
}

unsafe extern "system" fn rl_add_ref(this: *mut c_void) -> u32 {
    frame_add_ref(frame_from_runloop(this))
}

unsafe extern "system" fn rl_release(this: *mut c_void) -> u32 {
    frame_release(frame_from_runloop(this))
}

unsafe fn with_registry(this: *mut c_void, f: impl FnOnce(&mut RunLoopRegistry)) -> i32 {
    let frame = frame_from_runloop(this);
    match (*(*frame).registry).lock() {
        Ok(mut reg) => {
            f(&mut reg);
            K_RESULT_OK
        }
        Err(_) => K_INVALID_ARGUMENT,
    }
}

unsafe extern "system" fn register_event_handler(
    this: *mut c_void,
    handler: *mut c_void,
    fd: i32,
) -> i32 {
    if handler.is_null() {
        return K_INVALID_ARGUMENT;
    }
    handler_add_ref(handler);
    with_registry(this, |reg| {
        reg.event_handlers.push(EventHandlerEntry { handler, fd });
    })
}

unsafe extern "system" fn unregister_event_handler(this: *mut c_void, handler: *mut c_void) -> i32 {
    let mut removed: Vec<*mut c_void> = Vec::new();
    let rc = with_registry(this, |reg| {
        reg.event_handlers.retain(|e| {
            if e.handler == handler {
                removed.push(e.handler);
                false
            } else {
                true
            }
        });
    });
    for h in removed {
        handler_release(h);
    }
    rc
}

unsafe extern "system" fn register_timer(
    this: *mut c_void,
    handler: *mut c_void,
    milliseconds: u64,
) -> i32 {
    if handler.is_null() {
        return K_INVALID_ARGUMENT;
    }
    handler_add_ref(handler);
    let interval = Duration::from_millis(milliseconds.max(1));
    with_registry(this, |reg| {
        reg.timers.push(TimerEntry {
            handler,
            interval,
            next_fire: Instant::now() + interval,
        });
    })
}

unsafe extern "system" fn unregister_timer(this: *mut c_void, handler: *mut c_void) -> i32 {
    let mut removed: Vec<*mut c_void> = Vec::new();
    let rc = with_registry(this, |reg| {
        reg.timers.retain(|t| {
            if t.handler == handler {
                removed.push(t.handler);
                false
            } else {
                true
            }
        });
    });
    for h in removed {
        handler_release(h);
    }
    rc
}

/// Owned IPlugFrame/IRunLoop pair for one plugin view. Keep it alive
/// from before `IPlugView::setFrame` until after `IPlugView::removed`.
pub struct HostPlugFrame {
    ptr: *mut Frame,
    registry: Arc<Mutex<RunLoopRegistry>>,
}

// Safety: the frame is created and used on the UI thread; Send lets
// it live inside UI-side structs that iced requires to be Send.
unsafe impl Send for HostPlugFrame {}

impl HostPlugFrame {
    pub fn new() -> Self {
        let registry = Arc::new(Mutex::new(RunLoopRegistry::default()));
        let boxed = Box::new(Frame {
            frame_vtbl: &FRAME_VTBL,
            runloop_vtbl: &RUNLOOP_VTBL,
            refcount: AtomicU32::new(1),
            registry: Arc::into_raw(Arc::clone(&registry)),
        });
        Self {
            ptr: Box::into_raw(boxed),
            registry,
        }
    }

    pub fn as_iplugframe(&self) -> *mut c_void {
        self.ptr as *mut c_void
    }

    /// Fire due timers and ready fds for this view's plugin.
    pub fn service(&self) {
        service_registry(&self.registry);
    }

    /// Take a plugin-initiated resize request, if any.
    pub fn take_pending_resize(&self) -> Option<(u32, u32)> {
        self.registry
            .lock()
            .ok()
            .and_then(|mut reg| reg.pending_resize.take())
    }
}

impl Default for HostPlugFrame {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for HostPlugFrame {
    fn drop(&mut self) {
        unsafe { frame_release(self.ptr) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sdk_bytes(tuid: [::std::os::raw::c_char; 16]) -> Vec<u8> {
        tuid.iter().map(|b| *b as u8).collect()
    }

    #[test]
    fn iplugframe_iid_matches_sdk() {
        assert_eq!(
            IPLUGFRAME_IID.as_slice(),
            sdk_bytes(vst3::Steinberg::IPlugFrame_iid).as_slice()
        );
    }

    #[test]
    fn irunloop_iid_matches_sdk() {
        assert_eq!(
            IRUNLOOP_IID.as_slice(),
            sdk_bytes(vst3::Steinberg::Linux::IRunLoop_iid).as_slice()
        );
    }

    #[test]
    fn qi_runloop_subobject_roundtrip() {
        let frame = HostPlugFrame::new();
        unsafe {
            let mut rl: *mut c_void = std::ptr::null_mut();
            let hr = frame_query_interface(
                frame.as_iplugframe() as *mut Frame,
                IRUNLOOP_IID.as_ptr(),
                &mut rl,
            );
            assert_eq!(hr, K_RESULT_OK);
            assert!(!rl.is_null());
            assert_ne!(rl, frame.as_iplugframe());

            // QI back from the subobject to the frame.
            let mut back: *mut c_void = std::ptr::null_mut();
            let hr = rl_query_interface(rl, IPLUGFRAME_IID.as_ptr(), &mut back);
            assert_eq!(hr, K_RESULT_OK);
            assert_eq!(back, frame.as_iplugframe());

            // Drop the two refs QI added.
            rl_release(rl);
            rl_release(rl);
        }
    }
}
