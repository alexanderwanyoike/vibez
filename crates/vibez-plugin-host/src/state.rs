//! Plugin state capture and restore.
//!
//! CLAP: the `clap.state` extension with host-provided
//! `clap_ostream`/`clap_istream` callbacks writing into a Vec.
//! VST3: `IComponent::getState`/`setState` (plus
//! `IEditController::setComponentState` for dual-component plugins)
//! through the in-memory IBStream in `vst3_host::bstream`.
//!
//! Capture must happen while the plugin object is alive inside the
//! engine, so the UI keeps a [`PluginStatePtr`] per device (the same
//! pattern the GUI layer uses with its raw pointers) and calls
//! [`capture_plugin_state`] on the UI thread at save time. State
//! functions are main-thread class in both plugin APIs; plugins
//! synchronize internally against their audio thread.

use std::ffi::c_void;

use clap_sys::ext::state::{clap_plugin_state, CLAP_EXT_STATE};
use clap_sys::plugin::clap_plugin;
use clap_sys::stream::{clap_istream, clap_ostream};

/// Raw pointer needed to capture a live plugin's state from the UI
/// thread after the instance itself has moved into the engine.
#[derive(Debug, Clone, Copy)]
pub enum PluginStatePtr {
    /// `*const clap_plugin`
    Clap(*const c_void),
    /// VST3 IComponent pointer (getState/setState live there).
    Vst3Component(*mut c_void),
}

// Safety: the pointers are only dereferenced on the UI thread; the
// enum is Send so it can live in UI state alongside PluginRawPtr.
unsafe impl Send for PluginStatePtr {}

/// Capture the plugin's current state as an opaque blob.
///
/// # Safety
/// The pointer must refer to a still-loaded plugin instance (the
/// device must not have been removed from the engine). UI thread only.
pub unsafe fn capture_plugin_state(ptr: &PluginStatePtr) -> Option<Vec<u8>> {
    match ptr {
        PluginStatePtr::Clap(p) => clap_save_state(*p as *const clap_plugin),
        PluginStatePtr::Vst3Component(p) => vst3_component_get_state(*p),
    }
}

// ── CLAP ──

unsafe extern "C" fn ostream_write(
    stream: *const clap_ostream,
    buffer: *const c_void,
    size: u64,
) -> i64 {
    let out = &mut *((*stream).ctx as *mut Vec<u8>);
    let n = size as usize;
    out.extend_from_slice(std::slice::from_raw_parts(buffer as *const u8, n));
    n as i64
}

struct IstreamCtx<'a> {
    data: &'a [u8],
    pos: usize,
}

unsafe extern "C" fn istream_read(
    stream: *const clap_istream,
    buffer: *mut c_void,
    size: u64,
) -> i64 {
    let ctx = &mut *((*stream).ctx as *mut IstreamCtx);
    let available = ctx.data.len().saturating_sub(ctx.pos);
    let n = (size as usize).min(available);
    if n > 0 {
        std::ptr::copy_nonoverlapping(ctx.data.as_ptr().add(ctx.pos), buffer as *mut u8, n);
        ctx.pos += n;
    }
    n as i64
}

unsafe fn clap_state_ext(plugin: *const clap_plugin) -> Option<*const clap_plugin_state> {
    if plugin.is_null() {
        return None;
    }
    let get_extension = (*plugin).get_extension?;
    let ext = get_extension(plugin, CLAP_EXT_STATE.as_ptr()) as *const clap_plugin_state;
    if ext.is_null() {
        None
    } else {
        Some(ext)
    }
}

/// Save a CLAP plugin's state via the `clap.state` extension.
///
/// # Safety
/// `plugin` must be a valid, initialized clap_plugin. Main thread.
pub(crate) unsafe fn clap_save_state(plugin: *const clap_plugin) -> Option<Vec<u8>> {
    let ext = clap_state_ext(plugin)?;
    let save = (*ext).save?;
    let mut out: Vec<u8> = Vec::new();
    let stream = clap_ostream {
        ctx: &mut out as *mut Vec<u8> as *mut c_void,
        write: Some(ostream_write),
    };
    if save(plugin, &stream) {
        Some(out)
    } else {
        None
    }
}

/// Load previously saved state into a CLAP plugin.
///
/// # Safety
/// `plugin` must be a valid, initialized clap_plugin. Main thread.
pub(crate) unsafe fn clap_load_state(plugin: *const clap_plugin, data: &[u8]) -> bool {
    let Some(ext) = clap_state_ext(plugin) else {
        return false;
    };
    let Some(load) = (*ext).load else {
        return false;
    };
    let mut ctx = IstreamCtx { data, pos: 0 };
    let stream = clap_istream {
        ctx: &mut ctx as *mut IstreamCtx as *mut c_void,
        read: Some(istream_read),
    };
    load(plugin, &stream)
}

// ── VST3 ──

/// IComponent vtable: FUnknown[0..2], IPluginBase[3..4], then
/// getControllerClassId[5], setIoMode[6], getBusCount[7],
/// getBusInfo[8], getRoutingInfo[9], activateBus[10], setActive[11],
/// setState[12], getState[13].
const VST3_COMPONENT_SET_STATE: usize = 12;
const VST3_COMPONENT_GET_STATE: usize = 13;
/// IEditController vtable: FUnknown[0..2], IPluginBase[3..4], then
/// setComponentState[5].
const VST3_CONTROLLER_SET_COMPONENT_STATE: usize = 5;

type StateFn = unsafe extern "system" fn(*mut c_void, *mut c_void) -> i32;

unsafe fn vst3_state_call(obj: *mut c_void, slot: usize, stream: *mut c_void) -> i32 {
    let vtbl = *(obj as *const *const *const c_void);
    let f: StateFn = std::mem::transmute(*vtbl.add(slot));
    f(obj, stream)
}

/// # Safety
/// `component` must be a valid IComponent pointer. Main thread.
pub(crate) unsafe fn vst3_component_get_state(component: *mut c_void) -> Option<Vec<u8>> {
    if component.is_null() {
        return None;
    }
    let stream = crate::vst3_host::bstream::MemoryStream::for_writing();
    let hr = vst3_state_call(component, VST3_COMPONENT_GET_STATE, stream.as_ibstream());
    if hr == 0 {
        Some(stream.data())
    } else {
        None
    }
}

/// Restore component state, mirroring it into the controller when one
/// exists (dual-component plugins keep an independent copy).
///
/// # Safety
/// Pointers must be valid (controller may be null). Main thread.
pub(crate) unsafe fn vst3_set_state(
    component: *mut c_void,
    controller: *mut c_void,
    data: &[u8],
) -> bool {
    if component.is_null() {
        return false;
    }
    let stream = crate::vst3_host::bstream::MemoryStream::with_data(data.to_vec());
    let hr = vst3_state_call(component, VST3_COMPONENT_SET_STATE, stream.as_ibstream());
    if hr != 0 {
        return false;
    }
    if !controller.is_null() && controller != component {
        // Fresh stream so the controller reads from position 0.
        let stream = crate::vst3_host::bstream::MemoryStream::with_data(data.to_vec());
        vst3_state_call(
            controller,
            VST3_CONTROLLER_SET_COMPONENT_STATE,
            stream.as_ibstream(),
        );
    }
    true
}
