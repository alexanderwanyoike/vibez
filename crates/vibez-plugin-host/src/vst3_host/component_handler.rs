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
        if flags & vst3::Steinberg::Vst::RestartFlags_::kLatencyChanged != 0 {
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
    pub(super) unsafe fn install(controller: *mut c_void) -> Result<Self, String> {
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
                return Err("VST3 controller rejected the component handler".into());
            }
        }
        Ok(Self {
            _interface: interface,
            requested,
        })
    }

    pub(super) fn requested(&self) -> bool {
        self.requested.load(Ordering::Acquire)
    }

    pub(super) fn clear(&self) {
        self.requested.store(false, Ordering::Release);
    }
}
