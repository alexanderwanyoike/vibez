//! Native VST3 instance lifecycle and process calls.

use std::path::Path;

use super::abi::*;
use vibez_core::effect::ParamDescriptor;

use crate::instance::PluginInstance;

/// A loaded VST3 plugin instance.
///
/// Uses raw vtable calls for COM interop since the `vst3` crate's trait methods
/// require smart pointers. This is standard FFI practice for plugin hosts.
pub struct Vst3PluginInstance {
    name: String,
    is_instrument: bool,
    _lib: super::module::Vst3Module,
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
    component_handler: Option<super::component_handler::ComponentHandler>,
    param_descriptors: Vec<ParamDescriptor>,
    param_values: Vec<f32>,
    /// VST3 ParamIDs, index-aligned with the descriptors.
    param_ids: Vec<u32>,
    /// (id, normalized value) queued for the next process block.
    pending_params: Vec<(u32, f64)>,
    input_ports: Vec<(String, crate::audio_ports::AudioPort)>,
    output_ports: Vec<(String, crate::audio_ports::AudioPort)>,
    input_buses: Vec<AudioBusBuffersRaw>,
    output_buses: Vec<AudioBusBuffersRaw>,
    external_inputs: Vec<vibez_core::routing::ExternalInputDescriptor>,
    max_frames: usize,
    live_queues: Vec<LiveParamQueue>,
    processing_error: Option<&'static str>,
    processing_failed: bool,
    note_events: Vec<NoteEvent>,
    sample_rate: f64,
    latency_samples: u32,
    main_thread: std::thread::ThreadId,
    active: bool,
    processing: bool,
}

unsafe impl Send for Vst3PluginInstance {}

/// Output of [`Vst3PluginInstance::load_partial`]: a dlopen'd module
/// with no plugin code executed yet.
pub struct PartialVst3Plugin {
    path: std::path::PathBuf,
    lib: libloading::Library,
    class_uid: String,
    is_instrument: bool,
}

impl Vst3PluginInstance {
    fn rebuild_ports(&mut self) -> Result<(), String> {
        type GetBusCount = unsafe extern "system" fn(*mut std::ffi::c_void, i32, i32) -> i32;
        let count: GetBusCount = unsafe { std::mem::transmute(*vtbl(self.component).add(7)) };
        type ActivateBus =
            unsafe extern "system" fn(*mut std::ffi::c_void, i32, i32, i32, u8) -> i32;
        let activate: ActivateBus = unsafe { std::mem::transmute(*vtbl(self.component).add(10)) };
        for direction in [0, 1] {
            for index in 0..unsafe { count(self.component, 1, direction) } {
                // Event output is optional for the host, as in initial loading.
                unsafe { activate(self.component, 1, direction, index, 1) };
            }
        }
        self.input_ports = unsafe {
            super::audio_ports::query(
                self.component,
                0,
                count(self.component, 0, 0),
                self.max_frames,
            )
        }?;
        self.output_ports = unsafe {
            super::audio_ports::query(
                self.component,
                1,
                count(self.component, 0, 1),
                self.max_frames,
            )
        }?;
        self.external_inputs = crate::audio_ports::descriptors(&self.input_ports);
        let buses = |ports: &mut Vec<(String, crate::audio_ports::AudioPort)>| {
            ports
                .iter_mut()
                .map(|(_, port)| AudioBusBuffersRaw {
                    num_channels: port.channels as i32,
                    silence_flags: 0,
                    channel_buffers32: port.pointers.as_mut_ptr(),
                })
                .collect()
        };
        self.input_buses = buses(&mut self.input_ports);
        self.output_buses = buses(&mut self.output_ports);
        Ok(())
    }

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
            path: path.to_path_buf(),
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
        Self::init_on_main_thread_with_state(partial, sample_rate, max_buffer_size, None, false)
    }

    /// Restores state while inactive so ports and latency describe the first activation.
    pub fn init_on_main_thread_with_state(
        partial: PartialVst3Plugin,
        sample_rate: f64,
        max_buffer_size: u32,
        saved_state: Option<&[u8]>,
        strict_state_restore: bool,
    ) -> Result<Self, String> {
        let PartialVst3Plugin {
            path,
            lib,
            class_uid,
            is_instrument,
        } = partial;
        let class_uid = class_uid.as_str();
        let lib = super::module::Vst3Module::init(lib, &path)?;

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

        let mut instance = Self {
            name,
            is_instrument,
            _lib: lib,
            component,
            processor,
            controller,
            controller_is_separate,
            component_handler: None,
            param_descriptors: Vec::new(),
            param_values: Vec::new(),
            param_ids: Vec::new(),
            pending_params: Vec::with_capacity(2048),
            input_ports: Vec::new(),
            output_ports: Vec::new(),
            input_buses: Vec::new(),
            output_buses: Vec::new(),
            external_inputs: Vec::new(),
            max_frames: max_buffer_size as usize,
            live_queues: Vec::with_capacity(2048),
            processing_error: None,
            processing_failed: false,
            note_events: Vec::with_capacity(2048),
            sample_rate,
            latency_samples: 0,
            main_thread: std::thread::current().id(),
            active: false,
            processing: false,
        };

        if let Some(state) = saved_state {
            if !instance.load_state(state) {
                if strict_state_restore {
                    return Err(format!("{} rejected its saved state", instance.name));
                }
                eprintln!("vibez: {} rejected saved state", instance.name);
            }
        }
        instance.rebuild_ports()?;

        // Enumerate parameters from the edit controller (VST3 params
        // are always normalized 0..1).
        if !instance.controller.is_null() {
            let (descriptors, values, ids) = query_vst3_params(instance.controller);
            instance.param_descriptors = descriptors;
            instance.param_values = values;
            instance.param_ids = ids;
        }

        instance.component_handler =
            Some(unsafe { super::component_handler::ComponentHandler::install(controller) });
        let _preparation = instance
            .component_handler
            .as_ref()
            .map(|handler| handler.preparing());
        instance.prepare(sample_rate, max_buffer_size);
        if !instance.activate() {
            return Err(format!("{} failed activation", instance.name));
        }

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
    fn reconfiguration_requested(&self) -> bool {
        self.component_handler
            .as_ref()
            .is_some_and(|handler| handler.requested())
    }

    fn stop_for_reconfiguration(&mut self) {
        self.stop_processing();
    }

    fn reconfigure_on_main_thread(&mut self) -> Result<(), String> {
        if self.main_thread != std::thread::current().id() {
            return Err(format!("{} requires its owning main thread", self.name));
        }
        if self.processing {
            return Err(format!("{} is still processing", self.name));
        }
        if let Some(handler) = &self.component_handler {
            handler.clear();
        }
        let _preparation = self
            .component_handler
            .as_ref()
            .map(|handler| handler.preparing());
        self.deactivate();
        self.latency_samples = 0;
        self.rebuild_ports()?;
        if !self.activate() {
            return Err(format!("{} failed to reactivate", self.name));
        }
        Ok(())
    }

    fn latency_samples(&self) -> u32 {
        self.latency_samples
    }

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
            if self.pending_params.len() == self.pending_params.capacity() {
                return false;
            }
            self.param_values[index] = value;
            self.pending_params
                .push((self.param_ids[index], value.clamp(0.0, 1.0) as f64));
            true
        } else {
            false
        }
    }

    fn get_param(&self, index: usize) -> f32 {
        self.param_values.get(index).copied().unwrap_or(0.0)
    }

    fn external_inputs(&self) -> &[vibez_core::routing::ExternalInputDescriptor] {
        &self.external_inputs
    }
    fn process_audio(&mut self, buffer: &mut [f32], channels: usize) {
        self.process_with_inputs(buffer, channels, &[]);
    }
    fn process_with_inputs(
        &mut self,
        buffer: &mut [f32],
        channels: usize,
        inputs: &[vibez_core::routing::ExternalInputBlock<'_>],
    ) {
        if !self.active || self.processor.is_null() {
            buffer.fill(0.0);
            return;
        }
        if !self.processing {
            type SetProcessingFn = unsafe extern "system" fn(*mut std::ffi::c_void, i32) -> i32;
            let proc_vtbl = unsafe { vtbl(self.processor) };
            let set_processing: SetProcessingFn = unsafe { std::mem::transmute(*proc_vtbl.add(8)) };
            let result = unsafe { set_processing(self.processor, 1) };
            if result != 0 {
                self.processing_error = Some("VST3 setProcessing(true) failed");
                buffer.fill(0.0);
                return;
            }
            self.processing = true;
        }

        let frames = buffer.len() / channels.max(1);
        if frames == 0 || frames > self.max_frames {
            return;
        }

        crate::audio_ports::prepare_process_ports(
            &mut self.input_ports,
            &mut self.output_ports,
            buffer,
            channels,
            inputs,
            frames,
        );
        self.live_queues.clear();
        for (id, value) in self.pending_params.drain(..) {
            self.live_queues.push(LiveParamQueue {
                vtbl: &LIVE_PARAM_QUEUE_VTBL,
                id,
                value,
            });
        }
        let live_queues = &self.live_queues;
        let live_changes = LiveParamChanges {
            vtbl: &LIVE_PARAM_CHANGES_VTBL,
            queues: live_queues.as_ptr(),
            len: live_queues.len(),
        };
        // EngineTrack submits note-offs before note-ons to keep its active-note
        // mask correct. VST3 requires chronological input; at one offset,
        // release before retrigger. Unstable sorting allocates no scratch buffer.
        self.note_events
            .sort_unstable_by_key(|event| (event.frame_offset, event.is_on));
        let mut live_events = LiveEventList::new(&self.note_events);

        let mut process_data = ProcessDataRaw {
            process_mode: 0,
            symbolic_sample_size: 0,
            num_samples: frames as i32,
            num_inputs: self.input_buses.len() as i32,
            num_outputs: self.output_buses.len() as i32,
            inputs: self.input_buses.as_mut_ptr(),
            outputs: self.output_buses.as_mut_ptr(),
            input_parameter_changes: if live_queues.is_empty() {
                param_changes_stub()
            } else {
                &live_changes as *const LiveParamChanges as *mut std::ffi::c_void
            },
            output_parameter_changes: param_changes_stub(),
            input_events: live_events.as_raw_mut(),
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
            crate::audio_ports::copy_process_output(&self.output_ports, buffer, channels, frames);
        } else {
            if !self.processing_failed {
                self.processing_error = Some("VST3 process returned failure");
            }
            self.processing_failed = true;
            buffer.fill(0.0);
        }
        self.note_events.clear();
    }

    fn note_on(&mut self, pitch: u8, velocity: u8) {
        if self.note_events.len() == self.note_events.capacity() {
            return;
        }
        self.note_events.push(NoteEvent {
            is_on: true,
            pitch,
            velocity,
            frame_offset: 0,
        });
    }

    fn note_off(&mut self, pitch: u8) {
        if self.note_events.len() == self.note_events.capacity() {
            return;
        }
        self.note_events.push(NoteEvent {
            is_on: false,
            pitch,
            velocity: 0,
            frame_offset: 0,
        });
    }

    fn note_on_at(&mut self, pitch: u8, velocity: u8, frame_offset: u32) {
        if self.note_events.len() == self.note_events.capacity() {
            return;
        }
        self.note_events.push(NoteEvent {
            is_on: true,
            pitch,
            velocity,
            frame_offset,
        });
    }

    fn note_off_at(&mut self, pitch: u8, frame_offset: u32) {
        if self.note_events.len() == self.note_events.capacity() {
            return;
        }
        self.note_events.push(NoteEvent {
            is_on: false,
            pitch,
            velocity: 0,
            frame_offset,
        });
    }

    fn reset(&mut self) {
        self.note_events.clear();
        // VST3 exposes the processing boundary instead of a separate reset
        // entry point. The next render starts processing on this same thread.
        self.stop_processing();
    }

    fn is_instrument(&self) -> bool {
        self.is_instrument
    }

    fn prepare(&mut self, sample_rate: f64, max_buffer_size: u32) {
        if self.active || self.processing {
            return;
        }
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
        type SetActiveFn = unsafe extern "system" fn(*mut std::ffi::c_void, i32) -> i32;
        let comp_vtbl = unsafe { vtbl(self.component) };
        let set_active: SetActiveFn = unsafe { std::mem::transmute(*comp_vtbl.add(11)) };
        let _preparation = self
            .component_handler
            .as_ref()
            .map(|handler| handler.preparing());
        let hr = unsafe { set_active(self.component, 1) };
        if hr != 0 {
            eprintln!("vibez: {}: setActive(1) failed (hr={hr})", self.name);
        }
        if hr == 0 {
            self.active = true;
            self.processing_failed = false;
            self.latency_samples = unsafe { super::latency::query(self.processor) };
            true
        } else {
            self.latency_samples = 0;
            false
        }
    }

    fn take_processing_error(&mut self) -> Option<&'static str> {
        self.processing_error.take()
    }

    fn stop_processing(&mut self) {
        // VST3 permits this exclusive boundary on UI or audio after the last process call.

        if self.processor.is_null() || !self.processing {
            return;
        }
        type SetProcessingFn = unsafe extern "system" fn(*mut std::ffi::c_void, i32) -> i32;
        let proc_vtbl = unsafe { vtbl(self.processor) };
        let set_processing: SetProcessingFn = unsafe { std::mem::transmute(*proc_vtbl.add(8)) };
        unsafe { set_processing(self.processor, 0) };
        self.processing = false;
    }

    fn deactivate(&mut self) {
        if !self.active {
            return;
        }
        self.stop_processing();
        if !self.component.is_null() {
            type SetActiveFn = unsafe extern "system" fn(*mut std::ffi::c_void, i32) -> i32;
            let comp_vtbl = unsafe { vtbl(self.component) };
            let set_active: SetActiveFn = unsafe { std::mem::transmute(*comp_vtbl.add(11)) };
            unsafe { set_active(self.component, 0) };
        }
        self.active = false;
    }
}

#[path = "instance_lifecycle.rs"]
mod lifecycle;

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
