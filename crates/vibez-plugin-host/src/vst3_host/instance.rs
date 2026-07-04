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
    param_descriptors: Vec<ParamDescriptor>,
    param_values: Vec<f32>,
    buffer_adapter: AudioBufferAdapter,
    note_events: Vec<NoteEvent>,
    sample_rate: f64,
    active: bool,
}

unsafe impl Send for Vst3PluginInstance {}

#[allow(dead_code)]
struct NoteEvent {
    is_on: bool,
    pitch: u8,
    velocity: u8,
}

// VST3 IComponent IID: {E831FF31-F2D5-4301-928E-BBEE25697802}
const ICOMPONENT_IID: [u8; 16] = [
    0xE8, 0x31, 0xFF, 0x31, 0xF2, 0xD5, 0x43, 0x01, 0x92, 0x8E, 0xBB, 0xEE, 0x25, 0x69, 0x78, 0x02,
];

// VST3 IAudioProcessor IID: {42043F99-B7DA-453C-A569-E79D9AAEC33F}
const IAUDIOPROCESSOR_IID: [u8; 16] = [
    0x42, 0x04, 0x3F, 0x99, 0xB7, 0xDA, 0x45, 0x3C, 0xA5, 0x69, 0xE7, 0x9D, 0x9A, 0xAE, 0xC3, 0x3F,
];

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

    /// Extract a GUI handle (IEditController) from this instance.
    /// Must be called on the main thread before wrapping for the audio thread.
    pub fn extract_gui_handle(&self) -> Option<crate::gui::Vst3GuiHandle> {
        unsafe { crate::gui::Vst3GuiHandle::new(self.component) }
    }

    /// Load and instantiate a VST3 plugin by its class ID from a `.vst3` bundle.
    pub fn load(
        path: &Path,
        class_uid: &str,
        is_instrument: bool,
        sample_rate: f64,
        max_buffer_size: u32,
    ) -> Result<Self, String> {
        let module_path = super::scanner::find_vst3_module(path)?;

        let lib = unsafe {
            libloading::Library::new(&module_path)
                .map_err(|e| format!("Failed to load VST3 module: {e}"))?
        };
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
        let hr = unsafe { initialize(component, std::ptr::null_mut()) };
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

        // Get name
        let name = get_class_name_raw(factory_ptr, &cid_bytes);

        // Release factory
        type ReleaseFn = unsafe extern "system" fn(*mut std::ffi::c_void) -> u32;
        let release: ReleaseFn = unsafe { std::mem::transmute(*factory_vtbl.add(2)) };
        unsafe { release(factory_ptr) };

        let buffer_adapter = AudioBufferAdapter::new(2, max_buffer_size as usize);

        let mut instance = Self {
            name,
            is_instrument,
            _lib: lib,
            component,
            processor,
            param_descriptors: Vec::new(),
            param_values: Vec::new(),
            buffer_adapter,
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

        let bufs = self.buffer_adapter.channel_buffers_mut();
        let mut data_ptrs: Vec<*mut f32> = bufs.iter_mut().map(|b| b.as_mut_ptr()).collect();

        let mut input_bus = AudioBusBuffersRaw {
            num_channels: channels as i32,
            silence_flags: 0,
            channel_buffers32: data_ptrs.as_mut_ptr(),
        };

        let mut output_bus = AudioBusBuffersRaw {
            num_channels: channels as i32,
            silence_flags: 0,
            channel_buffers32: data_ptrs.as_mut_ptr(),
        };

        let mut process_data = ProcessDataRaw {
            process_mode: 0,
            symbolic_sample_size: 0,
            num_samples: frames as i32,
            num_inputs: if self.is_instrument { 0 } else { 1 },
            num_outputs: 1,
            inputs: &mut input_bus,
            outputs: &mut output_bus,
            input_parameter_changes: std::ptr::null_mut(),
            output_parameter_changes: std::ptr::null_mut(),
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
        let _hr = unsafe { process(self.processor, &mut process_data) };

        self.buffer_adapter.interleave(buffer, frames);
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
            unsafe { setup_processing(self.processor, &mut setup) };
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
        if !self.processor.is_null() {
            type ReleaseFn = unsafe extern "system" fn(*mut std::ffi::c_void) -> u32;
            let proc_vtbl = unsafe { vtbl(self.processor) };
            let release: ReleaseFn = unsafe { std::mem::transmute(*proc_vtbl.add(2)) };
            unsafe { release(self.processor) };
        }
        super::scanner::vst3_module_exit(&self._lib);
    }
}
