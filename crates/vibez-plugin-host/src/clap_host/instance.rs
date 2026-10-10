//! Native CLAP lifecycle and allocation-free declared-port processing.

use std::ffi::CString;
use std::path::Path;

use clap_sys::events::{
    clap_event_header, clap_event_note, clap_input_events, clap_output_events,
    CLAP_CORE_EVENT_SPACE_ID, CLAP_EVENT_NOTE_OFF, CLAP_EVENT_NOTE_ON,
};
use clap_sys::ext::params::{clap_plugin_params, CLAP_EXT_PARAMS};
use clap_sys::plugin::clap_plugin;
use clap_sys::process::{clap_process, clap_process_status};

use vibez_core::effect::ParamDescriptor;

use crate::instance::PluginInstance;

/// A loaded CLAP plugin instance.
pub struct ClapPluginInstance {
    name: String,
    is_instrument: bool,
    plugin_ptr: *const clap_plugin,
    host_ptr: *const clap_sys::host::clap_host,
    _lib: libloading::Library,
    param_descriptors: Vec<ParamDescriptor>,
    param_values: Vec<f32>,
    /// CLAP param ids and cookies, index-aligned with the
    /// descriptors (cookies stored as usize to stay Send).
    param_ids: Vec<u32>,
    param_cookies: Vec<usize>,
    /// (id, cookie, native value) queued for the next process block.
    pending_params: Vec<(u32, usize, f64)>,
    input_ports: Vec<(String, crate::audio_ports::AudioPort)>,
    output_ports: Vec<(String, crate::audio_ports::AudioPort)>,
    input_buffers: Vec<clap_sys::audio_buffer::clap_audio_buffer>,
    output_buffers: Vec<clap_sys::audio_buffer::clap_audio_buffer>,
    external_inputs: Vec<vibez_core::routing::ExternalInputDescriptor>,
    max_frames: u32,
    input_events_storage: Vec<ClapNoteEventWrapper>,
    param_events_storage: Vec<clap_sys::events::clap_event_param_value>,
    event_headers: Vec<*const clap_event_header>,
    note_events: Vec<NoteEvent>,
    sample_rate: f64,
    latency_samples: u32,
    audio_context: Option<vibez_core::audio_context::DeviceAudioContext>,
    main_thread: std::thread::ThreadId,
    active: bool,
    processing: bool,
    processing_error: Option<&'static str>,
    processing_failed: bool,
    configuration_failed: bool,
    recovery_attempts: u8,
}

// Safety: CLAP plugins are expected to be thread-safe for audio processing.
// The plugin is only accessed from one thread at a time.
unsafe impl Send for ClapPluginInstance {}

struct NoteEvent {
    is_on: bool,
    pitch: u8,
    velocity: u8,
    /// Frame offset within the current process buffer for sample-accurate timing.
    time: u32,
}

/// A partially loaded CLAP plugin — DSO loaded on a background thread.
/// Only the shared library is loaded; NO CLAP API calls have been made.
/// `factory.create_plugin()` and `plugin.init()` must both happen on the
/// UI thread (the process main thread), because JUCE-based plugins (via
/// `clap-juce-extensions`) call `ScopedJuceInitialiser_GUI` during
/// `create_plugin()`, which registers the calling thread as JUCE's
/// "message thread". All later GUI calls must happen on that same thread.
pub struct PartialClapPlugin {
    pub is_instrument: bool,
    lib: libloading::Library,
    entry_ptr: *const clap_sys::entry::clap_plugin_entry,
    path: String,
    plugin_id: String,
}

// Safety: The library handle and entry pointer are just integers.
// No CLAP/JUCE code has been called yet — safe to transfer between threads.
unsafe impl Send for PartialClapPlugin {}

impl ClapPluginInstance {
    /// Return the raw plugin pointer (for GUI handle extraction before wrapping).
    pub fn plugin_ptr(&self) -> *const clap_plugin {
        self.plugin_ptr
    }

    /// Extract a GUI handle from this instance.
    /// Must be called on the main thread before wrapping for the audio thread.
    pub fn extract_gui_handle(&self) -> Option<crate::gui::ClapGuiHandle> {
        unsafe { crate::gui::ClapGuiHandle::new(self.plugin_ptr) }
    }

    /// Phase 1: Load the DSO on a background thread.
    /// Only `dlopen()` and symbol lookup happen here — NO CLAP API calls.
    /// `factory.create_plugin()` and `plugin.init()` must both happen on the
    /// UI thread via `init_on_main_thread()`, because JUCE-based plugins
    /// initialize their MessageManager during `create_plugin()`.
    pub fn load_partial(
        path: &Path,
        plugin_id: &str,
        is_instrument: bool,
    ) -> Result<PartialClapPlugin, String> {
        let lib = unsafe {
            libloading::Library::new(super::module_path(path)?)
                .map_err(|e| format!("Failed to load CLAP library: {e}"))?
        };

        let entry: libloading::Symbol<'_, *const clap_sys::entry::clap_plugin_entry> = unsafe {
            lib.get(b"clap_entry\0")
                .map_err(|e| format!("No clap_entry: {e}"))?
        };

        let entry_ptr = *entry;
        if entry_ptr.is_null() {
            return Err("clap_entry is null".into());
        }

        Ok(PartialClapPlugin {
            is_instrument,
            lib,
            entry_ptr,
            path: path.to_str().unwrap_or_default().to_string(),
            plugin_id: plugin_id.to_string(),
        })
    }

    /// Phase 2: Initialize and activate the plugin. MUST be called on the UI
    /// thread (the process main thread) because JUCE-based plugins (via
    /// `clap-juce-extensions`) call `ScopedJuceInitialiser_GUI` during
    /// `create_plugin()`, registering the calling thread as JUCE's "message
    /// thread". All CLAP API calls happen here: `entry.init()`,
    /// `factory.create_plugin()`, `plugin.init()`, params, activate.
    pub fn init_on_main_thread(
        partial: PartialClapPlugin,
        sample_rate: f64,
        max_buffer_size: u32,
    ) -> Result<Self, String> {
        Self::init_on_main_thread_with_state(partial, sample_rate, max_buffer_size, None, false)
    }

    /// Restores state while inactive so ports and latency describe the first activation.
    pub fn init_on_main_thread_with_state(
        partial: PartialClapPlugin,
        sample_rate: f64,
        max_buffer_size: u32,
        saved_state: Option<&[u8]>,
        strict_state_restore: bool,
    ) -> Result<Self, String> {
        let entry_ref = unsafe { &*partial.entry_ptr };

        // entry.init() — lightweight, just returns true for clap-juce-extensions
        let path_cstr = CString::new(partial.path.as_str()).map_err(|e| format!("{e}"))?;
        let init_ok = unsafe { (entry_ref.init.unwrap())(path_cstr.as_ptr()) };
        if !init_ok {
            return Err("clap_entry.init() failed".into());
        }

        // Get factory
        let factory_ptr = unsafe {
            (entry_ref.get_factory.unwrap())(
                clap_sys::factory::plugin_factory::CLAP_PLUGIN_FACTORY_ID.as_ptr(),
            )
        }
            as *const clap_sys::factory::plugin_factory::clap_plugin_factory;

        if factory_ptr.is_null() {
            return Err("No plugin factory".into());
        }

        let factory = unsafe { &*factory_ptr };

        // Create host descriptor (lives on the heap, leaked for plugin lifetime)
        let host = Box::leak(Box::new(super::host_impl::make_clap_host()));

        // create_plugin() — JUCE's ScopedJuceInitialiser_GUI runs HERE,
        // registering THIS thread as the JUCE message thread.
        let id_cstr = CString::new(partial.plugin_id.as_str()).map_err(|e| format!("{e}"))?;
        let plugin_ptr = unsafe {
            (factory.create_plugin.unwrap())(factory_ptr, host as *const _, id_cstr.as_ptr())
        };

        if plugin_ptr.is_null() {
            return Err(format!(
                "Failed to create plugin instance: {}",
                partial.plugin_id
            ));
        }

        // Set host_data so timer/fd callbacks can find the plugin pointer
        unsafe { super::host_impl::set_host_user_data(host, plugin_ptr) };

        // Get plugin name from descriptor
        let plugin_ref = unsafe { &*plugin_ptr };
        let name = if !plugin_ref.desc.is_null() {
            let desc = unsafe { &*plugin_ref.desc };
            if !desc.name.is_null() {
                unsafe { std::ffi::CStr::from_ptr(desc.name) }
                    .to_str()
                    .unwrap_or("Unknown")
                    .to_string()
            } else {
                "Unknown".to_string()
            }
        } else {
            "Unknown".to_string()
        };

        // plugin.init() — on the SAME thread as create_plugin()
        let init_ok = unsafe { (plugin_ref.init.unwrap())(plugin_ptr) };
        if !init_ok {
            super::host_impl::unregister_host_callbacks(host);
            unsafe { (plugin_ref.destroy.unwrap())(plugin_ptr) };
            return Err("Plugin init() failed".into());
        }

        let mut instance = Self {
            name,
            is_instrument: partial.is_instrument,
            plugin_ptr,
            host_ptr: host,
            _lib: partial.lib,
            param_descriptors: Vec::new(),
            param_values: Vec::new(),
            param_ids: Vec::new(),
            param_cookies: Vec::new(),
            pending_params: Vec::with_capacity(2048),
            input_ports: Vec::new(),
            output_ports: Vec::new(),
            input_buffers: Vec::new(),
            output_buffers: Vec::new(),
            external_inputs: Vec::new(),
            max_frames: max_buffer_size,
            input_events_storage: Vec::with_capacity(2048),
            param_events_storage: Vec::with_capacity(2048),
            event_headers: Vec::with_capacity(4096),
            note_events: Vec::with_capacity(2048),
            sample_rate,
            latency_samples: 0,
            audio_context: None,
            main_thread: std::thread::current().id(),
            active: false,
            processing: false,
            processing_error: None,
            processing_failed: false,
            configuration_failed: false,
            recovery_attempts: 0,
        };

        if let Some(state) = saved_state {
            if !instance.load_state(state) {
                if strict_state_restore {
                    return Err(format!("{} rejected its saved state", instance.name));
                }
                eprintln!("vibez: {} rejected saved state", instance.name);
            }
        }
        (
            instance.param_descriptors,
            instance.param_values,
            instance.param_ids,
            instance.param_cookies,
        ) = query_params(plugin_ptr);
        instance.rebuild_ports()?;
        instance.prepare(sample_rate, max_buffer_size);
        if !instance.activate() {
            return Err(format!(
                "{} failed activation or latency preparation",
                instance.name
            ));
        }

        Ok(instance)
    }

    fn rebuild_ports(&mut self) -> Result<(), String> {
        self.input_ports =
            unsafe { super::audio_ports::query(self.plugin_ptr, true, self.max_frames as usize) }?;
        self.output_ports =
            unsafe { super::audio_ports::query(self.plugin_ptr, false, self.max_frames as usize) }?;
        self.external_inputs = crate::audio_ports::descriptors(&self.input_ports);
        self.input_buffers = super::audio_ports::buffers(&mut self.input_ports);
        self.output_buffers = super::audio_ports::buffers(&mut self.output_ports);
        Ok(())
    }

    /// Single-shot load (convenience — calls both phases on the current thread).
    /// For GUI support, use `load_partial` on a background thread +
    /// `init_on_main_thread` on the UI thread.
    pub fn load(
        path: &Path,
        plugin_id: &str,
        is_instrument: bool,
        sample_rate: f64,
        max_buffer_size: u32,
    ) -> Result<Self, String> {
        let partial = Self::load_partial(path, plugin_id, is_instrument)?;
        Self::init_on_main_thread(partial, sample_rate, max_buffer_size)
    }
}

#[allow(clippy::type_complexity)]
fn query_params(
    plugin_ptr: *const clap_plugin,
) -> (Vec<ParamDescriptor>, Vec<f32>, Vec<u32>, Vec<usize>) {
    let plugin_ref = unsafe { &*plugin_ptr };

    let ext_ptr =
        unsafe { (plugin_ref.get_extension.unwrap())(plugin_ptr, CLAP_EXT_PARAMS.as_ptr()) }
            as *const clap_plugin_params;

    if ext_ptr.is_null() {
        return (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    }

    let params_ext = unsafe { &*ext_ptr };
    let count = unsafe { (params_ext.count.unwrap())(plugin_ptr) } as usize;

    let mut descriptors = Vec::with_capacity(count);
    let mut values = Vec::with_capacity(count);
    let mut ids = Vec::with_capacity(count);
    let mut cookies = Vec::with_capacity(count);

    for i in 0..count {
        let mut info = clap_sys::ext::params::clap_param_info {
            id: 0,
            flags: 0,
            cookie: std::ptr::null_mut(),
            name: [0; 256],
            module: [0; 1024],
            min_value: 0.0,
            max_value: 1.0,
            default_value: 0.0,
        };

        let ok = unsafe { (params_ext.get_info.unwrap())(plugin_ptr, i as u32, &mut info) };
        if !ok {
            continue;
        }

        // Extract name from the fixed-size char array
        let name_bytes: Vec<u8> = info
            .name
            .iter()
            .take_while(|&&b| b != 0)
            .map(|&b| b as u8)
            .collect();
        let name_string = String::from_utf8_lossy(&name_bytes).to_string();
        // Leak the string so we get &'static str (small, lives for session duration)
        let name_static: &'static str = Box::leak(name_string.into_boxed_str());

        let mut value = 0.0_f64;
        let _ok = unsafe { (params_ext.get_value.unwrap())(plugin_ptr, info.id, &mut value) };

        descriptors.push(ParamDescriptor {
            name: name_static,
            min: info.min_value as f32,
            max: info.max_value as f32,
            default: info.default_value as f32,
            unit: "",
        });
        values.push(value as f32);
        ids.push(info.id);
        cookies.push(info.cookie as usize);
    }

    (descriptors, values, ids, cookies)
}

impl PluginInstance for ClapPluginInstance {
    fn set_audio_context(&mut self, context: vibez_core::audio_context::DeviceAudioContext) {
        if context.sample_rate as f64 != self.sample_rate {
            self.processing_failed = true;
            self.configuration_failed = true;
        }
        self.audio_context = Some(context);
    }

    fn processing_failure_is_local(&self) -> bool {
        self.active && self.processing_failed && !self.configuration_failed
    }

    fn processing_recovery_requested(&self) -> bool {
        self.active
            && self.processing_failed
            && !self.configuration_failed
            && self.recovery_attempts < crate::instance::MAX_PROCESSING_RECOVERIES
    }

    fn reconfiguration_requested(&self) -> bool {
        let data =
            unsafe { &*((*self.host_ptr).host_data as *const super::host_impl::ClapHostUserData) };
        self.processing_recovery_requested()
            || data
                .restart_requested
                .load(std::sync::atomic::Ordering::Acquire)
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
        let data =
            unsafe { &*((*self.host_ptr).host_data as *const super::host_impl::ClapHostUserData) };
        data.restart_requested
            .store(false, std::sync::atomic::Ordering::Release);
        let _activation = super::host_impl::activation_scope(self.host_ptr);
        if self.processing_recovery_requested() {
            self.recovery_attempts += 1;
        }
        self.deactivate();
        self.latency_samples = 0;
        self.rebuild_ports()?;
        if !self.activate() {
            return Err(format!("{} failed to reactivate", self.name));
        }

        Ok(())
    }

    fn activation_sample_rate(&self) -> Option<u32> {
        Some(self.sample_rate as u32)
    }
    fn processing_configuration_valid(&self) -> bool {
        self.active && !self.processing_failed
    }
    fn latency_samples(&self) -> u32 {
        self.latency_samples
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn save_state(&mut self) -> Option<Vec<u8>> {
        unsafe { crate::state::clap_save_state(self.plugin_ptr) }
    }

    fn load_state(&mut self, data: &[u8]) -> bool {
        unsafe { crate::state::clap_load_state(self.plugin_ptr, data) }
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
            self.pending_params.push((
                self.param_ids[index],
                self.param_cookies[index],
                value as f64,
            ));
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
        if !self.active || self.plugin_ptr.is_null() {
            buffer.fill(0.0);
            return;
        }

        let _audio_role = super::host_impl::audio_role();
        if !self.processing_configuration_valid() {
            self.processing_failed = true;
            buffer.fill(0.0);
            return;
        }
        if !self.processing {
            let plugin_ref = unsafe { &*self.plugin_ptr };
            let started = unsafe { (plugin_ref.start_processing.unwrap())(self.plugin_ptr) };
            if !started {
                self.processing_error = Some("CLAP start_processing failed");
                self.processing_failed = true;
                buffer.fill(0.0);
                return;
            }
            self.processing = true;
        }

        let frames = buffer.len() / channels.max(1);
        if frames == 0 || frames > self.max_frames as usize {
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

        // Build note events — sorted by time for CLAP spec compliance
        self.input_events_storage.clear();
        let input_events_storage = &mut self.input_events_storage;
        // Sort events by frame offset so the plugin sees them in order
        self.note_events
            .sort_unstable_by_key(|event| (event.time, event.is_on));
        for ne in self.note_events.drain(..) {
            let event = clap_event_note {
                header: clap_event_header {
                    size: std::mem::size_of::<clap_event_note>() as u32,
                    time: ne.time,
                    space_id: CLAP_CORE_EVENT_SPACE_ID,
                    type_: if ne.is_on {
                        CLAP_EVENT_NOTE_ON
                    } else {
                        CLAP_EVENT_NOTE_OFF
                    },
                    flags: 0,
                },
                note_id: -1,
                port_index: 0,
                channel: 0,
                key: ne.pitch as i16,
                velocity: ne.velocity as f64 / 127.0,
            };
            input_events_storage.push(ClapNoteEventWrapper(event));
        }

        // Param events (automation): delivered at block start.
        self.param_events_storage.clear();
        for (id, cookie, value) in self.pending_params.drain(..) {
            self.param_events_storage
                .push(clap_sys::events::clap_event_param_value {
                    header: clap_event_header {
                        size: std::mem::size_of::<clap_sys::events::clap_event_param_value>()
                            as u32,
                        time: 0,
                        space_id: CLAP_CORE_EVENT_SPACE_ID,
                        type_: clap_sys::events::CLAP_EVENT_PARAM_VALUE,
                        flags: 0,
                    },
                    param_id: id,
                    cookie: cookie as *mut std::ffi::c_void,
                    note_id: -1,
                    port_index: -1,
                    channel: -1,
                    key: -1,
                    value,
                });
        }
        let param_events_storage = &self.param_events_storage;
        // Merge into one header list: params first (time 0), then the
        // time-sorted notes.
        self.event_headers.clear();
        let event_headers = &mut self.event_headers;
        for pe in param_events_storage {
            event_headers.push(&pe.header);
        }
        for ne in input_events_storage {
            event_headers.push(&ne.0.header);
        }

        // Create input/output event lists
        let input_events = ClapInputEvents {
            events: event_headers,
        };
        let input_events_clap = clap_input_events {
            ctx: &input_events as *const ClapInputEvents as *const std::ffi::c_void
                as *mut std::ffi::c_void,
            size: Some(input_events_size),
            get: Some(input_events_get),
        };

        let output_events_clap = clap_output_events {
            ctx: std::ptr::null_mut(),
            try_push: Some(output_events_try_push),
        };

        let transport = self.audio_context.map(crate::process_context::clap);
        let process = clap_process {
            steady_time: self
                .audio_context
                .map_or(-1, |context| context.continuous_sample as i64),
            frames_count: frames as u32,
            transport: transport
                .as_ref()
                .map_or(std::ptr::null(), |transport| transport as *const _),
            audio_inputs: self.input_buffers.as_ptr(),
            audio_outputs: self.output_buffers.as_mut_ptr(),
            audio_inputs_count: self.input_buffers.len() as u32,
            audio_outputs_count: self.output_buffers.len() as u32,
            in_events: &input_events_clap,
            out_events: &output_events_clap,
        };

        let plugin_ref = unsafe { &*self.plugin_ptr };
        let status: clap_process_status =
            unsafe { (plugin_ref.process.unwrap())(self.plugin_ptr, &process) };

        if status == clap_sys::process::CLAP_PROCESS_ERROR {
            if !self.processing_failed {
                self.processing_error = Some("CLAP process returned failure");
            }
            self.processing_failed = true;
            buffer.fill(0.0);
        } else {
            crate::audio_ports::copy_process_output(&self.output_ports, buffer, channels, frames);
        }
    }

    fn note_on(&mut self, pitch: u8, velocity: u8) {
        if self.note_events.len() == self.note_events.capacity() {
            return;
        }
        self.note_events.push(NoteEvent {
            is_on: true,
            pitch,
            velocity,
            time: 0,
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
            time: 0,
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
            time: frame_offset,
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
            time: frame_offset,
        });
    }

    fn reset(&mut self) {
        let _audio_role = super::host_impl::audio_role();
        self.note_events.clear();
        if self.processing {
            let plugin = unsafe { &*self.plugin_ptr };
            if let Some(reset) = plugin.reset {
                unsafe { reset(self.plugin_ptr) };
            }
        }
    }

    fn is_instrument(&self) -> bool {
        self.is_instrument
    }

    fn prepare(&mut self, sample_rate: f64, max_buffer_size: u32) {
        if self.active || self.processing {
            if sample_rate != self.sample_rate || max_buffer_size != self.max_frames {
                self.processing_failed = true;
                self.configuration_failed = true;
            }
            return;
        }
        self.sample_rate = sample_rate;
        self.max_frames = max_buffer_size;
    }

    fn activate(&mut self) -> bool {
        if self.plugin_ptr.is_null() {
            return false;
        }
        let plugin_ref = unsafe { &*self.plugin_ptr };
        let _activation = super::host_impl::activation_scope(self.host_ptr);
        let ok = unsafe {
            (plugin_ref.activate.unwrap())(self.plugin_ptr, self.sample_rate, 1, self.max_frames)
        };
        if ok {
            self.active = true;
            self.processing_failed = false;
            self.configuration_failed = false;
            match super::latency::query(self.plugin_ptr) {
                Ok(samples) => self.latency_samples = samples,
                Err(_) => {
                    self.deactivate();
                    self.latency_samples = 0;
                    return false;
                }
            }
        }
        if !ok {
            self.latency_samples = 0;
        }
        ok
    }

    fn take_processing_error(&mut self) -> Option<&'static str> {
        self.processing_error.take()
    }

    fn stop_processing(&mut self) {
        // Exclusive &mut ownership proves the previous worker has yielded.
        // CLAP permits the symbolic audio role on main during final teardown.
        let _audio_role = super::host_impl::audio_role();

        if self.plugin_ptr.is_null() || !self.processing {
            return;
        }
        let plugin_ref = unsafe { &*self.plugin_ptr };
        unsafe { (plugin_ref.stop_processing.unwrap())(self.plugin_ptr) };
        self.processing = false;
    }

    fn deactivate(&mut self) {
        if self.plugin_ptr.is_null() || !self.active {
            return;
        }
        self.stop_processing();
        let plugin_ref = unsafe { &*self.plugin_ptr };
        unsafe { (plugin_ref.deactivate.unwrap())(self.plugin_ptr) };
        self.active = false;
    }
}

impl Drop for ClapPluginInstance {
    fn drop(&mut self) {
        if !self.plugin_ptr.is_null() {
            super::host_impl::unregister_host_callbacks(self.host_ptr);
            if self.active {
                self.deactivate();
            }
            let plugin_ref = unsafe { &*self.plugin_ptr };
            unsafe { (plugin_ref.destroy.unwrap())(self.plugin_ptr) };
        }
    }
}

// -- Event list helpers --

#[repr(C)]
struct ClapNoteEventWrapper(clap_event_note);

struct ClapInputEvents<'a> {
    /// Type-erased event headers (notes and param values), sorted by
    /// time. The pointed-to storage outlives the process() call.
    events: &'a [*const clap_event_header],
}

unsafe extern "C" fn input_events_size(list: *const clap_input_events) -> u32 {
    let events = &*((*list).ctx as *const ClapInputEvents);
    events.events.len() as u32
}

unsafe extern "C" fn input_events_get(
    list: *const clap_input_events,
    index: u32,
) -> *const clap_event_header {
    let events = &*((*list).ctx as *const ClapInputEvents);
    if (index as usize) < events.events.len() {
        events.events[index as usize]
    } else {
        std::ptr::null()
    }
}

unsafe extern "C" fn output_events_try_push(
    _list: *const clap_output_events,
    _event: *const clap_event_header,
) -> bool {
    // Silently accept output events (we don't process them yet)
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_load_partial_nonexistent_file() {
        let result = ClapPluginInstance::load_partial(
            &PathBuf::from("/nonexistent/path/to/plugin.clap"),
            "com.test.plugin",
            false,
        );
        match result {
            Ok(_) => panic!("Expected error for nonexistent file"),
            Err(e) => assert!(
                e.contains("Failed to load"),
                "Expected 'Failed to load' in error: {e}"
            ),
        }
    }

    #[test]
    fn test_load_partial_invalid_library() {
        // Create a temporary file that isn't a valid shared library
        let dir = std::env::temp_dir();
        let fake_plugin = dir.join("vibez_test_fake.clap");
        std::fs::write(&fake_plugin, b"not a real shared library").unwrap();

        let result = ClapPluginInstance::load_partial(&fake_plugin, "com.test.plugin", false);
        assert!(result.is_err());

        std::fs::remove_file(&fake_plugin).ok();
    }

    #[test]
    fn test_partial_plugin_is_send() {
        // Compile-time check: PartialClapPlugin implements Send
        fn assert_send<T: Send>() {}
        assert_send::<PartialClapPlugin>();
    }

    #[test]
    fn test_clap_instance_is_send() {
        // Compile-time check: ClapPluginInstance implements Send
        fn assert_send<T: Send>() {}
        assert_send::<ClapPluginInstance>();
    }
}
