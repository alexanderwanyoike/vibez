//! Main-thread VST3 component and controller destruction ordering.

use super::*;

impl Drop for Vst3PluginInstance {
    fn drop(&mut self) {
        if self.active {
            self.deactivate();
        }
        unsafe {
            release_interfaces(
                self.component,
                self.processor,
                self.controller,
                self.controller_is_separate,
            );
        }
    }
}

pub(super) unsafe fn release_interfaces(
    component: *mut std::ffi::c_void,
    processor: *mut std::ffi::c_void,
    controller: *mut std::ffi::c_void,
    controller_is_separate: bool,
) {
    if !controller.is_null() {
        let ctrl_vtbl = unsafe { vtbl(controller) };
        if controller_is_separate {
            // IPluginBase::terminate - vtable [4]
            type TerminateFn = unsafe extern "system" fn(*mut std::ffi::c_void) -> i32;
            let terminate: TerminateFn = unsafe { std::mem::transmute(*ctrl_vtbl.add(4)) };
            unsafe { terminate(controller) };
        }
        type ReleaseFn = unsafe extern "system" fn(*mut std::ffi::c_void) -> u32;
        let release: ReleaseFn = unsafe { std::mem::transmute(*ctrl_vtbl.add(2)) };
        unsafe { release(controller) };
    }
    // Release the processor interface before terminating the
    // component: DPF warns (and may misbehave) if the audio
    // processor ref is still held at component teardown.
    if !processor.is_null() {
        type ReleaseFn = unsafe extern "system" fn(*mut std::ffi::c_void) -> u32;
        let proc_vtbl = unsafe { vtbl(processor) };
        let release: ReleaseFn = unsafe { std::mem::transmute(*proc_vtbl.add(2)) };
        unsafe { release(processor) };
    }
    if !component.is_null() {
        // IPluginBase::terminate - vtable [4]
        type TerminateFn = unsafe extern "system" fn(*mut std::ffi::c_void) -> i32;
        let comp_vtbl = unsafe { vtbl(component) };
        let terminate: TerminateFn = unsafe { std::mem::transmute(*comp_vtbl.add(4)) };
        unsafe { terminate(component) };

        // Release component
        type ReleaseFn = unsafe extern "system" fn(*mut std::ffi::c_void) -> u32;
        let release: ReleaseFn = unsafe { std::mem::transmute(*comp_vtbl.add(2)) };
        unsafe { release(component) };
    }
}
