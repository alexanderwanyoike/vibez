//! Optional VST3 latency and I/O change notification ownership.

use std::cell::Cell;
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use vst3::Steinberg::Vst::{IComponentHandler, IComponentHandlerTrait};
use vst3::{Class, ComPtr, ComWrapper};

struct Handler {
    requested: Arc<AtomicBool>,
}

impl Class for Handler {
    type Interfaces = (IComponentHandler,);
}

impl IComponentHandlerTrait for Handler {
    unsafe fn beginEdit(&self, _id: u32) -> i32 {
        0
    }

    unsafe fn performEdit(&self, _id: u32, _value: f64) -> i32 {
        0
    }

    unsafe fn endEdit(&self, _id: u32) -> i32 {
        0
    }

    unsafe fn restartComponent(&self, flags: i32) -> i32 {
        if flags
            & (vst3::Steinberg::Vst::RestartFlags_::kLatencyChanged
                | vst3::Steinberg::Vst::RestartFlags_::kIoChanged)
            != 0
            && PREPARING_HANDLER.get() != Arc::as_ptr(&self.requested) as usize
        {
            self.requested.store(true, Ordering::Release);
        }
        0
    }
}

pub(super) struct ComponentHandler {
    _interface: ComPtr<IComponentHandler>,
    requested: Arc<AtomicBool>,
}

impl ComponentHandler {
    pub(super) unsafe fn install(controller: *mut c_void) -> Self {
        let requested = Arc::new(AtomicBool::new(false));
        let interface = ComWrapper::new(Handler {
            requested: requested.clone(),
        })
        .to_com_ptr::<IComponentHandler>()
        .expect("handler declares its interface");
        if !controller.is_null() {
            type SetHandler = unsafe extern "system" fn(*mut c_void, *mut IComponentHandler) -> i32;
            let vtable = *(controller as *const *const *const c_void);
            let set: SetHandler = std::mem::transmute(*vtable.add(16));
            if set(controller, interface.as_ptr()) != 0 {
                eprintln!("vibez: VST3 controller declined optional latency/I/O notifications");
            }
        }
        Self {
            _interface: interface,
            requested,
        }
    }

    pub(super) fn preparing(&self) -> Preparation {
        Preparation(PREPARING_HANDLER.replace(Arc::as_ptr(&self.requested) as usize))
    }

    pub(super) fn requested(&self) -> bool {
        self.requested.load(Ordering::Acquire)
    }

    pub(super) fn clear(&self) {
        self.requested.store(false, Ordering::Release);
    }
}

thread_local! { static PREPARING_HANDLER: Cell<usize> = const { Cell::new(0) }; }

// Synchronous preparation notifications are informational, but unrelated
// requests must remain queued until the next ownership handoff.
pub(super) struct Preparation(usize);
impl Drop for Preparation {
    fn drop(&mut self) {
        PREPARING_HANDLER.set(self.0);
    }
}
