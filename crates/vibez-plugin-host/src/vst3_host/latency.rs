//! Main-thread cached VST3 latency reporting after activation.

use std::ffi::c_void;

pub(super) unsafe fn query(processor: *mut c_void) -> u32 {
    // The component has just become active and processing has not started.
    // IAudioProcessor's latency is therefore from the activated algorithm.
    type GetLatency = unsafe extern "system" fn(*mut c_void) -> u32;
    let vtable = *(processor as *const *const *const c_void);
    let get: GetLatency = std::mem::transmute(*vtable.add(6));
    get(processor)
}
