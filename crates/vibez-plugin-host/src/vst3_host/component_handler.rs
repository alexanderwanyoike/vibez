//! The host side of a VST3 controller: editor parameter changes bound for
//! the processor, and latency and I/O change notifications.

use std::cell::Cell;
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use vst3::Steinberg::Vst::{IComponentHandler, IComponentHandlerTrait};
use vst3::{Class, ComPtr, ComWrapper};

/// Room for every parameter of a large plugin changing twice before the
/// audio thread next drains the ring.
const EDIT_CAPACITY: usize = 4096;

/// Editor changes waiting for the processor: (parameter id, normalized value).
pub(super) type EditorEdits = rtrb::Consumer<(u32, f64)>;

struct Handler {
    requested: Arc<AtomicBool>,
    /// Only editor threads push here, and the audio thread never takes this
    /// lock: it owns the other end of the ring.
    edits: Mutex<rtrb::Producer<(u32, f64)>>,
    controller: *mut c_void,
    param_ids: Vec<u32>,
}

impl Class for Handler {
    type Interfaces = (IComponentHandler,);
}

impl Handler {
    fn push(&self, id: u32, value: f64) -> bool {
        let Ok(mut edits) = self.edits.lock() else {
            return false;
        };
        edits.push((id, value.clamp(0.0, 1.0))).is_ok()
    }

    /// A preset loaded in the editor changes values in the controller and
    /// reports only that they changed; the processor has to be told each one.
    unsafe fn forward_controller_values(&self) {
        if self.controller.is_null() {
            return;
        }
        type GetParamNormalized = unsafe extern "system" fn(*mut c_void, u32) -> f64;
        let vtable = *(self.controller as *const *const *const c_void);
        let get: GetParamNormalized = std::mem::transmute(*vtable.add(14));
        for &id in &self.param_ids {
            if !self.push(id, get(self.controller, id)) {
                return;
            }
        }
    }
}

impl IComponentHandlerTrait for Handler {
    unsafe fn beginEdit(&self, _id: u32) -> i32 {
        0
    }

    unsafe fn performEdit(&self, id: u32, value: f64) -> i32 {
        if self.push(id, value) {
            0
        } else {
            vst3::Steinberg::kResultFalse
        }
    }

    unsafe fn endEdit(&self, _id: u32) -> i32 {
        0
    }

    unsafe fn restartComponent(&self, flags: i32) -> i32 {
        use vst3::Steinberg::Vst::RestartFlags_;
        if flags & RestartFlags_::kParamValuesChanged != 0 {
            self.forward_controller_values();
        }
        if flags & (RestartFlags_::kLatencyChanged | RestartFlags_::kIoChanged) != 0
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
    /// Installs the handler on `controller`, returning the audio thread's end
    /// of the editor change ring. `param_ids` are the controller's parameters.
    pub(super) unsafe fn install(
        controller: *mut c_void,
        param_ids: Vec<u32>,
    ) -> (Self, EditorEdits) {
        let requested = Arc::new(AtomicBool::new(false));
        let (producer, consumer) = rtrb::RingBuffer::new(EDIT_CAPACITY);
        let interface = ComWrapper::new(Handler {
            requested: requested.clone(),
            edits: Mutex::new(producer),
            controller,
            param_ids,
        })
        .to_com_ptr::<IComponentHandler>()
        .expect("handler declares its interface");
        if !controller.is_null() {
            type SetHandler = unsafe extern "system" fn(*mut c_void, *mut IComponentHandler) -> i32;
            let vtable = *(controller as *const *const *const c_void);
            let set: SetHandler = std::mem::transmute(*vtable.add(16));
            if set(controller, interface.as_ptr()) != 0 {
                eprintln!("vibez: VST3 controller declined the component handler");
            }
        }
        (
            Self {
                _interface: interface,
                requested,
            },
            consumer,
        )
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
