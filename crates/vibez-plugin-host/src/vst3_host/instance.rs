use std::path::Path;

use vibez_core::effect::ParamDescriptor;

use crate::buffer::AudioBufferAdapter;
use crate::instance::PluginInstance;

/// A loaded VST3 plugin instance.
///
/// Uses raw vtable calls for COM interop since the `vst3` crate's trait methods
/// require smart pointers. This is standard FFI practice for plugin hosts.
pub struct Vst3PluginInstance {
    name: String,
    is_instrument: bool,
    _lib: libloading::Library,
    /// Raw COM pointer to IComponent (also IPluginBase)
    component: *mut std::ffi::c_void,
    /// Raw COM pointer to IAudioProcessor
    processor: *mut std::ffi::c_void,
    /// Raw COM pointer to IEditController. For single-component
    /// plugins (DPF) this is the component itself; for dual-component
    /// plugins (JUCE) it is a separate object created from the factory.
    controller: *mut std::ffi::c_void,
    /// True when `controller` is a separate object that we created and
    /// initialized, and therefore must terminate on drop.
    controller_is_separate: bool,
    param_descriptors: Vec<ParamDescriptor>,
    param_values: Vec<f32>,
    buffer_adapter: AudioBufferAdapter,
    /// Separate output buffers: VST3 process() is out-of-place by
    /// default and JUCE plugins do not reliably write in-place.
    out_adapter: AudioBufferAdapter,
    /// Audio bus counts reported by the component; process() must
    /// present an AudioBusBuffers entry per bus, active or not.
    audio_in_buses: i32,
    audio_out_buses: i32,
    /// One-shot latch so a failing process() logs once, not per block.
    process_error_logged: bool,
    note_events: Vec<NoteEvent>,
    sample_rate: f64,
    active: bool,
}

unsafe impl Send for Vst3PluginInstance {}

// ── Stub IParameterChanges ──
// DPF-based plugins assert on null input/outputParameterChanges and
// JUCE tolerates but prefers them. This is a stateless, static COM
// object: no parameters in, additions rejected.

#[repr(C)]
struct ParamChangesVtbl {
    query_interface: unsafe extern "system" fn(
        *mut std::ffi::c_void,
        *const u8,
        *mut *mut std::ffi::c_void,
    ) -> i32,
    add_ref: unsafe extern "system" fn(*mut std::ffi::c_void) -> u32,
    release: unsafe extern "system" fn(*mut std::ffi::c_void) -> u32,
    get_parameter_count: unsafe extern "system" fn(*mut std::ffi::c_void) -> i32,
    get_parameter_data:
        unsafe extern "system" fn(*mut std::ffi::c_void, i32) -> *mut std::ffi::c_void,
    add_parameter_data: unsafe extern "system" fn(
        *mut std::ffi::c_void,
        *const u32,
        *mut i32,
    ) -> *mut std::ffi::c_void,
}

unsafe extern "system" fn pc_query_interface(
    this: *mut std::ffi::c_void,
    _iid: *const u8,
    obj: *mut *mut std::ffi::c_void,
) -> i32 {
    // Static object: answer everything with ourselves; refcounting is
    // a no-op so over-answering is harmless.
    unsafe { *obj = this };
    0
}
unsafe extern "system" fn pc_add_ref(_this: *mut std::ffi::c_void) -> u32 {
    1
}
unsafe extern "system" fn pc_release(_this: *mut std::ffi::c_void) -> u32 {
    1
}
unsafe extern "system" fn pc_count(_this: *mut std::ffi::c_void) -> i32 {
    0
}
unsafe extern "system" fn pc_get_data(
    _this: *mut std::ffi::c_void,
    _index: i32,
) -> *mut std::ffi::c_void {
    std::ptr::null_mut()
}
unsafe extern "system" fn pc_add_data(
    _this: *mut std::ffi::c_void,
    _id: *const u32,
    index: *mut i32,
) -> *mut std::ffi::c_void {
    // Hand back a discard-queue: DPF asserts on a null return and
    // then writes its output points into whatever it gets.
    if !index.is_null() {
        unsafe { *index = 0 };
    }
    &PARAM_QUEUE_STUB as *const ParamQueueStub as *mut std::ffi::c_void
}

// IParamValueQueue stub: identifies as parameter 0, holds no points,
// accepts (and discards) added points.
#[repr(C)]
struct ParamQueueVtbl {
    query_interface: unsafe extern "system" fn(
        *mut std::ffi::c_void,
        *const u8,
        *mut *mut std::ffi::c_void,
    ) -> i32,
    add_ref: unsafe extern "system" fn(*mut std::ffi::c_void) -> u32,
    release: unsafe extern "system" fn(*mut std::ffi::c_void) -> u32,
    get_parameter_id: unsafe extern "system" fn(*mut std::ffi::c_void) -> u32,
    get_point_count: unsafe extern "system" fn(*mut std::ffi::c_void) -> i32,
    get_point: unsafe extern "system" fn(*mut std::ffi::c_void, i32, *mut i32, *mut f64) -> i32,
    add_point: unsafe extern "system" fn(*mut std::ffi::c_void, i32, f64, *mut i32) -> i32,
}

unsafe extern "system" fn pq_parameter_id(_this: *mut std::ffi::c_void) -> u32 {
    0
}
unsafe extern "system" fn pq_point_count(_this: *mut std::ffi::c_void) -> i32 {
    0
}
unsafe extern "system" fn pq_get_point(
    _this: *mut std::ffi::c_void,
    _index: i32,
    _offset: *mut i32,
    _value: *mut f64,
) -> i32 {
    1 // kResultFalse
}
unsafe extern "system" fn pq_add_point(
    _this: *mut std::ffi::c_void,
    _offset: i32,
    _value: f64,
    index: *mut i32,
) -> i32 {
    if !index.is_null() {
        unsafe { *index = 0 };
    }
    0
}

static PARAM_QUEUE_VTBL: ParamQueueVtbl = ParamQueueVtbl {
    query_interface: pc_query_interface,
    add_ref: pc_add_ref,
    release: pc_release,
    get_parameter_id: pq_parameter_id,
    get_point_count: pq_point_count,
    get_point: pq_get_point,
    add_point: pq_add_point,
};

#[repr(C)]
struct ParamQueueStub {
    vtbl: *const ParamQueueVtbl,
}
unsafe impl Sync for ParamQueueStub {}
static PARAM_QUEUE_STUB: ParamQueueStub = ParamQueueStub {
    vtbl: &PARAM_QUEUE_VTBL,
};

static PARAM_CHANGES_VTBL: ParamChangesVtbl = ParamChangesVtbl {
    query_interface: pc_query_interface,
    add_ref: pc_add_ref,
    release: pc_release,
    get_parameter_count: pc_count,
    get_parameter_data: pc_get_data,
    add_parameter_data: pc_add_data,
};

#[repr(C)]
struct ParamChangesStub {
    vtbl: *const ParamChangesVtbl,
}
unsafe impl Sync for ParamChangesStub {}
static PARAM_CHANGES_STUB: ParamChangesStub = ParamChangesStub {
    vtbl: &PARAM_CHANGES_VTBL,
};

fn param_changes_stub() -> *mut std::ffi::c_void {
    &PARAM_CHANGES_STUB as *const ParamChangesStub as *mut std::ffi::c_void
}

/// Output of [`Vst3PluginInstance::load_partial`]: a dlopen'd module
/// with no plugin code executed yet.
pub struct PartialVst3Plugin {
    lib: libloading::Library,
    class_uid: String,
    is_instrument: bool,
}

#[allow(dead_code)]
struct NoteEvent {
    is_on: bool,
    pitch: u8,
    velocity: u8,
}

// VST3 IComponent IID: {E831FF31-F2D5-4301-928E-BBEE25697802}
const ICOMPONENT_IID: [u8; 16] = crate::vst3_tuid([
    0xE8, 0x31, 0xFF, 0x31, 0xF2, 0xD5, 0x43, 0x01, 0x92, 0x8E, 0xBB, 0xEE, 0x25, 0x69, 0x78, 0x02,
]);

// VST3 IAudioProcessor IID: {42043F99-B7DA-453C-A569-E79D9AAEC33D}
const IAUDIOPROCESSOR_IID: [u8; 16] = crate::vst3_tuid([
    0x42, 0x04, 0x3F, 0x99, 0xB7, 0xDA, 0x45, 0x3C, 0xA5, 0x69, 0xE7, 0x9D, 0x9A, 0xAE, 0xC3, 0x3D,
]);

// VST3 IConnectionPoint IID: {70A4156F-6E6E-4026-9891-48BFAA60D8D1}
const ICONNECTIONPOINT_IID: [u8; 16] = crate::vst3_tuid([
    0x70, 0xA4, 0x15, 0x6F, 0x6E, 0x6E, 0x40, 0x26, 0x98, 0x91, 0x48, 0xBF, 0xAA, 0x60, 0xD8, 0xD1,
]);

/// Raw ProcessSetup matching VST3 C layout.
#[repr(C)]
struct ProcessSetupRaw {
    process_mode: i32,
    symbolic_sample_size: i32,
    max_samples_per_block: i32,
    sample_rate: f64,
}

/// Raw AudioBusBuffers matching VST3 C layout.
#[repr(C)]
struct AudioBusBuffersRaw {
    num_channels: i32,
    silence_flags: u64,
    channel_buffers32: *mut *mut f32,
}

/// Raw ProcessData matching VST3 C layout.
#[repr(C)]
struct ProcessDataRaw {
    process_mode: i32,
    symbolic_sample_size: i32,
    num_samples: i32,
    num_inputs: i32,
    num_outputs: i32,
    inputs: *mut AudioBusBuffersRaw,
    outputs: *mut AudioBusBuffersRaw,
    input_parameter_changes: *mut std::ffi::c_void,
    output_parameter_changes: *mut std::ffi::c_void,
    input_events: *mut std::ffi::c_void,
    output_events: *mut std::ffi::c_void,
    process_context: *mut std::ffi::c_void,
}

/// Helper: get vtable pointer from COM object.
unsafe fn vtbl(obj: *mut std::ffi::c_void) -> *const *const std::ffi::c_void {
    *(obj as *const *const *const std::ffi::c_void)
}

impl Vst3PluginInstance {
    /// Return the raw IComponent COM pointer (for GUI handle extraction).
    pub fn component_ptr(&self) -> *mut std::ffi::c_void {
        self.component
    }

    /// Raw IEditController pointer resolved at load time, or null when
    /// the plugin exposed none (GUI unavailable).
    pub fn controller_ptr(&self) -> *mut std::ffi::c_void {
        self.controller
    }

    /// Extract a GUI handle (IEditController) from this instance.
    /// Must be called on the main thread before wrapping for the audio thread.
    pub fn extract_gui_handle(&self) -> Option<crate::gui::Vst3GuiHandle> {
        unsafe { crate::gui::Vst3GuiHandle::new(self.controller) }
    }

    /// Load and instantiate a VST3 plugin by its class ID from a `.vst3` bundle.
    ///
    /// Everything after the dlopen runs plugin code that may pin
    /// itself to the calling thread (JUCE creates its MessageManager
    /// on the instantiating thread); call this on the UI thread, or
    /// use [`Self::load_partial`] + [`Self::init_on_main_thread`].
    pub fn load(
        path: &Path,
        class_uid: &str,
        is_instrument: bool,
        sample_rate: f64,
        max_buffer_size: u32,
    ) -> Result<Self, String> {
        let partial = Self::load_partial(path, class_uid, is_instrument)?;
        Self::init_on_main_thread(partial, sample_rate, max_buffer_size)
    }

    /// Phase 1: locate and dlopen the module. Safe on a background
    /// thread; runs no VST3 API calls (mirrors the CLAP two-phase
    /// load that exists for JUCE MessageManager thread affinity).
    pub fn load_partial(
        path: &Path,
        class_uid: &str,
        is_instrument: bool,
    ) -> Result<PartialVst3Plugin, String> {
        let module_path = super::scanner::find_vst3_module(path)?;
        let lib = unsafe {
            libloading::Library::new(&module_path)
                .map_err(|e| format!("Failed to load VST3 module: {e}"))?
        };
        Ok(PartialVst3Plugin {
            lib,
            class_uid: class_uid.to_string(),
            is_instrument,
        })
    }

    /// Phase 2: run module init, factory, instantiation, and
    /// activation. MUST run on the UI thread: JUCE plugins bind their
    /// MessageManager to this thread, and the GUI plus teardown must
    /// happen on the same one.
    pub fn init_on_main_thread(
        partial: PartialVst3Plugin,
        sample_rate: f64,
        max_buffer_size: u32,
    ) -> Result<Self, String> {
        let PartialVst3Plugin {
            lib,
            class_uid,
            is_instrument,
        } = partial;
        let class_uid = class_uid.as_str();
        let lib = super::scanner::vst3_module_init(lib)?;

        type GetFactoryFn = unsafe extern "system" fn() -> *mut std::ffi::c_void;

        let get_factory: libloading::Symbol<'_, GetFactoryFn> = unsafe {
            lib.get(b"GetPluginFactory\0")
                .map_err(|e| format!("No GetPluginFactory: {e}"))?
        };

        let factory_ptr = unsafe { get_factory() };
        if factory_ptr.is_null() {
            return Err("GetPluginFactory returned null".into());
        }

        // Give the factory our IHostApplication (IPluginFactory3).
        // DPF-based plugins fall back to this context when initialize
        // received none; best effort, older factories lack it.
        unsafe {
            type QueryInterfaceFn = unsafe extern "system" fn(
                *mut std::ffi::c_void,
                *const u8,
                *mut *mut std::ffi::c_void,
            ) -> i32;
            let qi: QueryInterfaceFn =
                std::mem::transmute(**(factory_ptr as *const *const *const std::ffi::c_void));
            let mut f3: *mut std::ffi::c_void = std::ptr::null_mut();
            if qi(
                factory_ptr,
                super::host_context::IPLUGINFACTORY3_IID.as_ptr(),
                &mut f3,
            ) == 0
                && !f3.is_null()
            {
                // IPluginFactory3::setHostContext - vtable [10]
                // (FUnknown 0-2, IPluginFactory 3-6, IPluginFactory2
                // getClassInfo2 [7], IPluginFactory3
                // getClassInfoUnicode [8]... setHostContext [9]).
                type SetHostContextFn =
                    unsafe extern "system" fn(*mut std::ffi::c_void, *mut std::ffi::c_void) -> i32;
                let f3_vtbl = vtbl(f3);
                let set_host_context: SetHostContextFn = std::mem::transmute(*f3_vtbl.add(9));
                set_host_context(f3, super::host_context::host_application());
                type ReleaseFn = unsafe extern "system" fn(*mut std::ffi::c_void) -> u32;
                let release: ReleaseFn = std::mem::transmute(*f3_vtbl.add(2));
                release(f3);
            }
        }

        let cid_bytes = parse_uid(class_uid)?;

        // IPluginFactory::createInstance(cid, iid, &mut obj)
        // vtable offset: [6]
        type CreateInstanceFn = unsafe extern "system" fn(
            *mut std::ffi::c_void,
            *const u8,
            *const u8,
            *mut *mut std::ffi::c_void,
        ) -> i32;

        let factory_vtbl = unsafe { vtbl(factory_ptr) };
        let create_instance: CreateInstanceFn =
            unsafe { std::mem::transmute(*factory_vtbl.add(6)) };

        let mut component: *mut std::ffi::c_void = std::ptr::null_mut();
        let hr = unsafe {
            create_instance(
                factory_ptr,
                cid_bytes.as_ptr(),
                ICOMPONENT_IID.as_ptr(),
                &mut component,
            )
        };
        if hr != 0 || component.is_null() {
            return Err(format!("Failed to create VST3 component (hr={hr})"));
        }

        // IPluginBase::initialize(context) - vtable offset: [3] (after FUnknown[0..2])
        type InitializeFn =
            unsafe extern "system" fn(*mut std::ffi::c_void, *mut std::ffi::c_void) -> i32;
        let comp_vtbl = unsafe { vtbl(component) };
        let initialize: InitializeFn = unsafe { std::mem::transmute(*comp_vtbl.add(3)) };
        let hr = unsafe { initialize(component, super::host_context::host_application()) };
        if hr != 0 {
            return Err(format!("Component initialize failed (hr={hr})"));
        }

        // FUnknown::queryInterface(iid, &mut obj) - vtable offset: [0]
        type QueryInterfaceFn = unsafe extern "system" fn(
            *mut std::ffi::c_void,
            *const u8,
            *mut *mut std::ffi::c_void,
        ) -> i32;
        let query_interface: QueryInterfaceFn = unsafe { std::mem::transmute(*comp_vtbl.add(0)) };

        let mut processor: *mut std::ffi::c_void = std::ptr::null_mut();
        let hr =
            unsafe { query_interface(component, IAUDIOPROCESSOR_IID.as_ptr(), &mut processor) };
        if hr != 0 || processor.is_null() {
            // Clean up: terminate and release the component before returning,
            // otherwise dropping `lib` unloads the DSO while the COM object
            // is still alive, causing a segfault.
            type TerminateFn = unsafe extern "system" fn(*mut std::ffi::c_void) -> i32;
            let terminate: TerminateFn = unsafe { std::mem::transmute(*comp_vtbl.add(4)) };
            unsafe { terminate(component) };

            type ReleaseFn = unsafe extern "system" fn(*mut std::ffi::c_void) -> u32;
            let release: ReleaseFn = unsafe { std::mem::transmute(*comp_vtbl.add(2)) };
            unsafe { release(component) };

            return Err("Component does not implement IAudioProcessor".into());
        }

        // Resolve the edit controller while the factory is still alive.
        // Single-component plugins (DPF: Dragonfly, Surge) expose it on
        // the component; dual-component plugins (JUCE: ZL, Vital) hand
        // out a separate class id that must be instantiated from the
        // factory, initialized, and connected to the component.
        let mut controller: *mut std::ffi::c_void = std::ptr::null_mut();
        let mut controller_is_separate = false;
        let hr = unsafe {
            query_interface(
                component,
                crate::gui::IEDIT_CONTROLLER_IID.as_ptr(),
                &mut controller,
            )
        };
        if hr != 0 || controller.is_null() {
            controller = std::ptr::null_mut();
            // IComponent::getControllerClassId(&mut TUID) - vtable [5]
            type GetControllerClassIdFn =
                unsafe extern "system" fn(*mut std::ffi::c_void, *mut u8) -> i32;
            let get_ctrl_cid: GetControllerClassIdFn =
                unsafe { std::mem::transmute(*comp_vtbl.add(5)) };
            let mut ctrl_cid = [0u8; 16];
            if unsafe { get_ctrl_cid(component, ctrl_cid.as_mut_ptr()) } == 0 {
                let hr = unsafe {
                    create_instance(
                        factory_ptr,
                        ctrl_cid.as_ptr(),
                        crate::gui::IEDIT_CONTROLLER_IID.as_ptr(),
                        &mut controller,
                    )
                };
                if hr == 0 && !controller.is_null() {
                    let ctrl_vtbl = unsafe { vtbl(controller) };
                    let ctrl_init: InitializeFn = unsafe { std::mem::transmute(*ctrl_vtbl.add(3)) };
                    if unsafe { ctrl_init(controller, std::ptr::null_mut()) } == 0 {
                        controller_is_separate = true;
                        unsafe { connect_component_and_controller(component, controller) };
                        // TODO: sync state (component getState ->
                        // controller setComponentState) once we have an
                        // IBStream implementation.
                    } else {
                        type ReleaseFn = unsafe extern "system" fn(*mut std::ffi::c_void) -> u32;
                        let release: ReleaseFn = unsafe { std::mem::transmute(*ctrl_vtbl.add(2)) };
                        unsafe { release(controller) };
                        controller = std::ptr::null_mut();
                    }
                } else {
                    controller = std::ptr::null_mut();
                }
            }
        }

        // Get name
        let name = get_class_name_raw(factory_ptr, &cid_bytes);
        if controller.is_null() {
            eprintln!("vibez: no IEditController for {name}: plugin GUI unavailable");
        }

        // Release factory
        type ReleaseFn = unsafe extern "system" fn(*mut std::ffi::c_void) -> u32;
        let release: ReleaseFn = unsafe { std::mem::transmute(*factory_vtbl.add(2)) };
        unsafe { release(factory_ptr) };

        let buffer_adapter = AudioBufferAdapter::new(2, max_buffer_size as usize);
        let out_adapter = AudioBufferAdapter::new(2, max_buffer_size as usize);

        // Discover audio buses and activate the main ones. JUCE
        // plugins skip processing entirely while their buses are
        // inactive, and process() must present one AudioBusBuffers
        // per reported bus (aux/sidechain entries stay empty).
        // IComponent: getBusCount [7], activateBus [10].
        type GetBusCountFn = unsafe extern "system" fn(*mut std::ffi::c_void, i32, i32) -> i32;
        type ActivateBusFn =
            unsafe extern "system" fn(*mut std::ffi::c_void, i32, i32, i32, u8) -> i32;
        const K_AUDIO: i32 = 0;
        const K_EVENT: i32 = 1;
        const K_INPUT: i32 = 0;
        const K_OUTPUT: i32 = 1;
        let get_bus_count: GetBusCountFn = unsafe { std::mem::transmute(*comp_vtbl.add(7)) };
        let activate_bus: ActivateBusFn = unsafe { std::mem::transmute(*comp_vtbl.add(10)) };
        let audio_in_buses = unsafe { get_bus_count(component, K_AUDIO, K_INPUT) };
        let audio_out_buses = unsafe { get_bus_count(component, K_AUDIO, K_OUTPUT) };
        unsafe {
            if audio_in_buses > 0 {
                activate_bus(component, K_AUDIO, K_INPUT, 0, 1);
            }
            if audio_out_buses > 0 {
                activate_bus(component, K_AUDIO, K_OUTPUT, 0, 1);
            }
            // Event buses (MIDI in/out for instruments).
            if get_bus_count(component, K_EVENT, K_INPUT) > 0 {
                activate_bus(component, K_EVENT, K_INPUT, 0, 1);
            }
            if get_bus_count(component, K_EVENT, K_OUTPUT) > 0 {
                activate_bus(component, K_EVENT, K_OUTPUT, 0, 1);
            }
        }

        let mut instance = Self {
            name,
            is_instrument,
            _lib: lib,
            component,
            processor,
            controller,
            controller_is_separate,
            param_descriptors: Vec::new(),
            param_values: Vec::new(),
            buffer_adapter,
            out_adapter,
            audio_in_buses,
            audio_out_buses,
            process_error_logged: false,
            note_events: Vec::new(),
            sample_rate,
            active: false,
        };

        instance.prepare(sample_rate, max_buffer_size);
        instance.activate();

        Ok(instance)
    }
}

fn get_class_name_raw(factory: *mut std::ffi::c_void, target_cid: &[u8; 16]) -> String {
    let factory_vtbl = unsafe { vtbl(factory) };

    type CountClassesFn = unsafe extern "system" fn(*mut std::ffi::c_void) -> i32;
    let count_classes: CountClassesFn = unsafe { std::mem::transmute(*factory_vtbl.add(4)) };
    let count = unsafe { count_classes(factory) } as usize;

    type GetClassInfoFn = unsafe extern "system" fn(
        *mut std::ffi::c_void,
        i32,
        *mut super::scanner::PClassInfoRaw,
    ) -> i32;
    let get_class_info: GetClassInfoFn = unsafe { std::mem::transmute(*factory_vtbl.add(5)) };

    for i in 0..count {
        let mut info = super::scanner::PClassInfoRaw::zeroed();
        let hr = unsafe { get_class_info(factory, i as i32, &mut info) };
        if hr == 0 && info.cid == *target_cid {
            let bytes: Vec<u8> = info.name.iter().take_while(|&&b| b != 0).copied().collect();
            return String::from_utf8_lossy(&bytes).to_string();
        }
    }
    "Unknown".to_string()
}

fn parse_uid(uid_str: &str) -> Result<[u8; 16], String> {
    let hex: String = uid_str.chars().filter(|c| c.is_ascii_hexdigit()).collect();
    if hex.len() != 32 {
        return Err(format!("Invalid UID: {uid_str}"));
    }
    let mut bytes = [0u8; 16];
    for i in 0..16 {
        bytes[i] = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16)
            .map_err(|e| format!("UID parse: {e}"))?;
    }
    Ok(bytes)
}

impl PluginInstance for Vst3PluginInstance {
    fn name(&self) -> &str {
        &self.name
    }

    fn save_state(&mut self) -> Option<Vec<u8>> {
        unsafe { crate::state::vst3_component_get_state(self.component) }
    }

    fn load_state(&mut self, data: &[u8]) -> bool {
        unsafe { crate::state::vst3_set_state(self.component, self.controller, data) }
    }

    fn param_count(&self) -> usize {
        self.param_descriptors.len()
    }

    fn param_descriptors_vec(&self) -> Vec<ParamDescriptor> {
        self.param_descriptors.clone()
    }

    fn set_param(&mut self, index: usize, value: f32) -> bool {
        if index < self.param_values.len() {
            self.param_values[index] = value;
            true
        } else {
            false
        }
    }

    fn get_param(&self, index: usize) -> f32 {
        self.param_values.get(index).copied().unwrap_or(0.0)
    }

    fn process_audio(&mut self, buffer: &mut [f32], channels: usize) {
        if !self.active || self.processor.is_null() {
            return;
        }

        let frames = buffer.len() / channels.max(1);
        if frames == 0 {
            return;
        }

        self.buffer_adapter.deinterleave(buffer, frames);
        for out in self.out_adapter.channel_buffers_mut() {
            out[..frames].fill(0.0);
        }

        let mut in_ptrs: Vec<*mut f32> = self
            .buffer_adapter
            .channel_buffers_mut()
            .iter_mut()
            .map(|b| b.as_mut_ptr())
            .collect();
        let mut out_ptrs: Vec<*mut f32> = self
            .out_adapter
            .channel_buffers_mut()
            .iter_mut()
            .map(|b| b.as_mut_ptr())
            .collect();

        // One AudioBusBuffers entry per reported bus. Bus 0 carries
        // the real audio; aux/sidechain buses are present but empty
        // (inactive, zero channels), which JUCE maps to silence.
        let empty_bus = || AudioBusBuffersRaw {
            num_channels: 0,
            silence_flags: u64::MAX,
            channel_buffers32: std::ptr::null_mut(),
        };
        let num_inputs = if self.is_instrument {
            self.audio_in_buses.max(0)
        } else {
            self.audio_in_buses.max(1)
        };
        let num_outputs = self.audio_out_buses.max(1);
        let mut input_buses: Vec<AudioBusBuffersRaw> = (0..num_inputs)
            .map(|i| {
                if i == 0 && !self.is_instrument {
                    AudioBusBuffersRaw {
                        num_channels: channels as i32,
                        silence_flags: 0,
                        channel_buffers32: in_ptrs.as_mut_ptr(),
                    }
                } else {
                    empty_bus()
                }
            })
            .collect();
        let mut output_buses: Vec<AudioBusBuffersRaw> = (0..num_outputs)
            .map(|i| {
                if i == 0 {
                    AudioBusBuffersRaw {
                        num_channels: channels as i32,
                        silence_flags: 0,
                        channel_buffers32: out_ptrs.as_mut_ptr(),
                    }
                } else {
                    empty_bus()
                }
            })
            .collect();

        let mut process_data = ProcessDataRaw {
            process_mode: 0,
            symbolic_sample_size: 0,
            num_samples: frames as i32,
            num_inputs,
            num_outputs,
            inputs: input_buses.as_mut_ptr(),
            outputs: output_buses.as_mut_ptr(),
            input_parameter_changes: param_changes_stub(),
            output_parameter_changes: param_changes_stub(),
            input_events: std::ptr::null_mut(),
            output_events: std::ptr::null_mut(),
            process_context: std::ptr::null_mut(),
        };

        // IAudioProcessor::process - vtable layout:
        //   FUnknown[0..2], then IAudioProcessor methods
        //   [3] setBusArrangements
        //   [4] getBusArrangement
        //   [5] canProcessSampleSize
        //   [6] getLatencySamples
        //   [7] setupProcessing
        //   [8] setProcessing
        //   [9] process
        type ProcessFn =
            unsafe extern "system" fn(*mut std::ffi::c_void, *mut ProcessDataRaw) -> i32;
        let proc_vtbl = unsafe { vtbl(self.processor) };
        let process: ProcessFn = unsafe { std::mem::transmute(*proc_vtbl.add(9)) };
        let hr = unsafe { process(self.processor, &mut process_data) };

        if hr == 0 {
            self.out_adapter.interleave(buffer, frames);
        } else if !self.process_error_logged {
            self.process_error_logged = true;
            eprintln!("vibez: {}: process() failed (hr={hr})", self.name);
        }
        // On failure the interleaved buffer keeps the dry signal.
        self.note_events.clear();
    }

    fn note_on(&mut self, pitch: u8, velocity: u8) {
        self.note_events.push(NoteEvent {
            is_on: true,
            pitch,
            velocity,
        });
    }

    fn note_off(&mut self, pitch: u8) {
        self.note_events.push(NoteEvent {
            is_on: false,
            pitch,
            velocity: 0,
        });
    }

    fn reset(&mut self) {
        self.note_events.clear();
    }

    fn is_instrument(&self) -> bool {
        self.is_instrument
    }

    fn prepare(&mut self, sample_rate: f64, max_buffer_size: u32) {
        self.sample_rate = sample_rate;
        if !self.processor.is_null() {
            let mut setup = ProcessSetupRaw {
                process_mode: 0,
                symbolic_sample_size: 0,
                max_samples_per_block: max_buffer_size as i32,
                sample_rate,
            };
            // IAudioProcessor::setupProcessing - vtable [7]
            type SetupProcessingFn =
                unsafe extern "system" fn(*mut std::ffi::c_void, *mut ProcessSetupRaw) -> i32;
            let proc_vtbl = unsafe { vtbl(self.processor) };
            let setup_processing: SetupProcessingFn =
                unsafe { std::mem::transmute(*proc_vtbl.add(7)) };
            let hr = unsafe { setup_processing(self.processor, &mut setup) };
            if hr != 0 {
                eprintln!("vibez: {}: setupProcessing failed (hr={hr})", self.name);
            }
        }
    }

    fn activate(&mut self) -> bool {
        if self.component.is_null() {
            return false;
        }
        // IComponent::setActive - IComponent inherits IPluginBase which inherits FUnknown
        // FUnknown[0..2], IPluginBase[3..4], IComponent[5..13]
        // setActive is IComponentTrait method index 6 => vtable[5+6] = [11]
        // Actually: FUnknown(3) + IPluginBase(2) + IComponent methods
        // setActive is the 7th method of IComponent... let me count:
        //   getControllerClassId, setIoMode, getBusCount, getBusInfo, getRoutingInfo,
        //   activateBus, setActive
        // That's index 6 (0-based) within IComponent methods
        // Total: 3 (FUnknown) + 2 (IPluginBase) + 6 = offset 11
        type SetActiveFn = unsafe extern "system" fn(*mut std::ffi::c_void, i32) -> i32;
        let comp_vtbl = unsafe { vtbl(self.component) };
        let set_active: SetActiveFn = unsafe { std::mem::transmute(*comp_vtbl.add(11)) };
        let hr = unsafe { set_active(self.component, 1) };
        if hr != 0 {
            eprintln!("vibez: {}: setActive(1) failed (hr={hr})", self.name);
        }
        if hr == 0 {
            if !self.processor.is_null() {
                // IAudioProcessor::setProcessing - vtable [8]
                type SetProcessingFn = unsafe extern "system" fn(*mut std::ffi::c_void, i32) -> i32;
                let proc_vtbl = unsafe { vtbl(self.processor) };
                let set_processing: SetProcessingFn =
                    unsafe { std::mem::transmute(*proc_vtbl.add(8)) };
                unsafe { set_processing(self.processor, 1) };
            }
            self.active = true;
            true
        } else {
            false
        }
    }

    fn deactivate(&mut self) {
        if !self.active {
            return;
        }
        if !self.processor.is_null() {
            type SetProcessingFn = unsafe extern "system" fn(*mut std::ffi::c_void, i32) -> i32;
            let proc_vtbl = unsafe { vtbl(self.processor) };
            let set_processing: SetProcessingFn = unsafe { std::mem::transmute(*proc_vtbl.add(8)) };
            unsafe { set_processing(self.processor, 0) };
        }
        if !self.component.is_null() {
            type SetActiveFn = unsafe extern "system" fn(*mut std::ffi::c_void, i32) -> i32;
            let comp_vtbl = unsafe { vtbl(self.component) };
            let set_active: SetActiveFn = unsafe { std::mem::transmute(*comp_vtbl.add(11)) };
            unsafe { set_active(self.component, 0) };
        }
        self.active = false;
    }
}

impl Drop for Vst3PluginInstance {
    fn drop(&mut self) {
        if self.active {
            self.deactivate();
        }
        if !self.controller.is_null() {
            let ctrl_vtbl = unsafe { vtbl(self.controller) };
            if self.controller_is_separate {
                // IPluginBase::terminate - vtable [4]
                type TerminateFn = unsafe extern "system" fn(*mut std::ffi::c_void) -> i32;
                let terminate: TerminateFn = unsafe { std::mem::transmute(*ctrl_vtbl.add(4)) };
                unsafe { terminate(self.controller) };
            }
            type ReleaseFn = unsafe extern "system" fn(*mut std::ffi::c_void) -> u32;
            let release: ReleaseFn = unsafe { std::mem::transmute(*ctrl_vtbl.add(2)) };
            unsafe { release(self.controller) };
        }
        // Release the processor interface before terminating the
        // component: DPF warns (and may misbehave) if the audio
        // processor ref is still held at component teardown.
        if !self.processor.is_null() {
            type ReleaseFn = unsafe extern "system" fn(*mut std::ffi::c_void) -> u32;
            let proc_vtbl = unsafe { vtbl(self.processor) };
            let release: ReleaseFn = unsafe { std::mem::transmute(*proc_vtbl.add(2)) };
            unsafe { release(self.processor) };
        }
        if !self.component.is_null() {
            // IPluginBase::terminate - vtable [4]
            type TerminateFn = unsafe extern "system" fn(*mut std::ffi::c_void) -> i32;
            let comp_vtbl = unsafe { vtbl(self.component) };
            let terminate: TerminateFn = unsafe { std::mem::transmute(*comp_vtbl.add(4)) };
            unsafe { terminate(self.component) };

            // Release component
            type ReleaseFn = unsafe extern "system" fn(*mut std::ffi::c_void) -> u32;
            let release: ReleaseFn = unsafe { std::mem::transmute(*comp_vtbl.add(2)) };
            unsafe { release(self.component) };
        }
        super::scanner::vst3_module_exit(&self._lib);
    }
}

/// Best-effort IConnectionPoint wiring between a dual-component
/// plugin's processor and controller. JUCE plugins use this channel to
/// keep the editor in sync with the processor; opening the GUI works
/// without it, but parameter changes would not propagate.
///
/// # Safety
/// Both pointers must be valid COM objects.
unsafe fn connect_component_and_controller(
    component: *mut std::ffi::c_void,
    controller: *mut std::ffi::c_void,
) {
    type QueryInterfaceFn = unsafe extern "system" fn(
        *mut std::ffi::c_void,
        *const u8,
        *mut *mut std::ffi::c_void,
    ) -> i32;
    type ReleaseFn = unsafe extern "system" fn(*mut std::ffi::c_void) -> u32;
    // IConnectionPoint::connect(other) - vtable [3]
    type ConnectFn = unsafe extern "system" fn(*mut std::ffi::c_void, *mut std::ffi::c_void) -> i32;

    let get_cp = |obj: *mut std::ffi::c_void| -> Option<*mut std::ffi::c_void> {
        let v = vtbl(obj);
        let qi: QueryInterfaceFn = std::mem::transmute(*v.add(0));
        let mut cp: *mut std::ffi::c_void = std::ptr::null_mut();
        if qi(obj, ICONNECTIONPOINT_IID.as_ptr(), &mut cp) == 0 && !cp.is_null() {
            Some(cp)
        } else {
            None
        }
    };

    let (Some(comp_cp), Some(ctrl_cp)) = (get_cp(component), get_cp(controller)) else {
        return;
    };
    let comp_connect: ConnectFn = std::mem::transmute(*vtbl(comp_cp).add(3));
    let ctrl_connect: ConnectFn = std::mem::transmute(*vtbl(ctrl_cp).add(3));
    comp_connect(comp_cp, ctrl_cp);
    ctrl_connect(ctrl_cp, comp_cp);
    let release_comp: ReleaseFn = std::mem::transmute(*vtbl(comp_cp).add(2));
    let release_ctrl: ReleaseFn = std::mem::transmute(*vtbl(ctrl_cp).add(2));
    release_comp(comp_cp);
    release_ctrl(ctrl_cp);
}

#[cfg(test)]
mod tests {
    /// Hand-written IIDs must match the SDK-generated constants in the
    /// vst3 crate. A single wrong byte makes every plugin reject the
    /// queryInterface call (a 0x3F-for-0x3D typo in IAudioProcessor
    /// once broke loading of ALL VST3 plugins).
    fn assert_iid(ours: [u8; 16], sdk: [::std::os::raw::c_char; 16]) {
        let sdk_bytes: Vec<u8> = sdk.iter().map(|b| *b as u8).collect();
        assert_eq!(ours.as_slice(), sdk_bytes.as_slice());
    }

    #[test]
    fn icomponent_iid_matches_sdk() {
        assert_iid(super::ICOMPONENT_IID, vst3::Steinberg::Vst::IComponent_iid);
    }

    #[test]
    fn iconnectionpoint_iid_matches_sdk() {
        assert_iid(
            super::ICONNECTIONPOINT_IID,
            vst3::Steinberg::Vst::IConnectionPoint_iid,
        );
    }

    #[test]
    fn iaudioprocessor_iid_matches_sdk() {
        assert_iid(
            super::IAUDIOPROCESSOR_IID,
            vst3::Steinberg::Vst::IAudioProcessor_iid,
        );
    }
}
