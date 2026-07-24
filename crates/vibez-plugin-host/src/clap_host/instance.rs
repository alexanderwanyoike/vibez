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

use crate::buffer::AudioBufferAdapter;
use crate::instance::PluginInstance;

/// A loaded CLAP plugin instance.
pub struct ClapPluginInstance {
    name: String,
    is_instrument: bool,
    plugin_ptr: *const clap_plugin,
    _lib: libloading::Library,
    param_descriptors: Vec<ParamDescriptor>,
    param_values: Vec<f32>,
    /// CLAP param ids and cookies, index-aligned with the
    /// descriptors (cookies stored as usize to stay Send).
    param_ids: Vec<u32>,
    param_cookies: Vec<usize>,
    /// (id, cookie, native value) queued for the next process block.
    pending_params: Vec<(u32, usize, f64)>,
    buffer_adapter: AudioBufferAdapter,
    note_events: Vec<NoteEvent>,
    sample_rate: f64,
    active: bool,
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
            libloading::Library::new(path)
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
            unsafe { (plugin_ref.destroy.unwrap())(plugin_ptr) };
            return Err("Plugin init() failed".into());
        }

        let (param_descriptors, param_values, param_ids, param_cookies) = query_params(plugin_ptr);
        let buffer_adapter = AudioBufferAdapter::new(2, max_buffer_size as usize);

        let mut instance = Self {
            name,
            is_instrument: partial.is_instrument,
            plugin_ptr,
            _lib: partial.lib,
            param_descriptors,
            param_values,
            param_ids,
            param_cookies,
            pending_params: Vec::new(),
            buffer_adapter,
            note_events: Vec::new(),
            sample_rate,
            active: false,
        };

        instance.prepare(sample_rate, max_buffer_size);
        instance.activate();

        Ok(instance)
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
            self.param_values[index] = value;
            // Deliver as a CLAP param event on the next process block.
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

    fn process_audio(&mut self, buffer: &mut [f32], channels: usize) {
        if !self.active || self.plugin_ptr.is_null() {
            return;
        }

        super::host_impl::mark_clap_audio_thread();

        let frames = buffer.len() / channels.max(1);
        if frames == 0 {
            return;
        }

        self.buffer_adapter.deinterleave(buffer, frames);

        // Build note events — sorted by time for CLAP spec compliance
        let mut input_events_storage: Vec<ClapNoteEventWrapper> = Vec::new();
        // Sort events by frame offset so the plugin sees them in order
        self.note_events.sort_by_key(|e| e.time);
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
        let param_events_storage: Vec<clap_sys::events::clap_event_param_value> = self
            .pending_params
            .drain(..)
            .map(
                |(id, cookie, value)| clap_sys::events::clap_event_param_value {
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
                },
            )
            .collect();

        // Merge into one header list: params first (time 0), then the
        // time-sorted notes.
        let mut event_headers: Vec<*const clap_event_header> =
            Vec::with_capacity(param_events_storage.len() + input_events_storage.len());
        for pe in &param_events_storage {
            event_headers.push(&pe.header as *const clap_event_header);
        }
        for ne in &input_events_storage {
            event_headers.push(&ne.0.header as *const clap_event_header);
        }

        // Create input/output event lists
        let input_events = ClapInputEvents {
            events: &event_headers,
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

        // Set up audio buffers
        let bufs = self.buffer_adapter.channel_buffers_mut();
        let mut data_ptrs: Vec<*mut f32> = bufs.iter_mut().map(|b| b.as_mut_ptr()).collect();
        let mut audio_inputs = [clap_sys::audio_buffer::clap_audio_buffer {
            data32: data_ptrs.as_mut_ptr(),
            data64: std::ptr::null_mut(),
            channel_count: channels as u32,
            latency: 0,
            constant_mask: 0,
        }];
        let mut audio_outputs = [clap_sys::audio_buffer::clap_audio_buffer {
            data32: data_ptrs.as_mut_ptr(),
            data64: std::ptr::null_mut(),
            channel_count: channels as u32,
            latency: 0,
            constant_mask: 0,
        }];

        let process = clap_process {
            steady_time: -1,
            frames_count: frames as u32,
            transport: std::ptr::null(),
            audio_inputs: audio_inputs.as_mut_ptr(),
            audio_outputs: audio_outputs.as_mut_ptr(),
            audio_inputs_count: if self.is_instrument { 0 } else { 1 },
            audio_outputs_count: 1,
            in_events: &input_events_clap,
            out_events: &output_events_clap,
        };

        let plugin_ref = unsafe { &*self.plugin_ptr };
        let _status: clap_process_status =
            unsafe { (plugin_ref.process.unwrap())(self.plugin_ptr, &process) };

        self.buffer_adapter.interleave(buffer, frames);
    }

    fn note_on(&mut self, pitch: u8, velocity: u8) {
        self.note_events.push(NoteEvent {
            is_on: true,
            pitch,
            velocity,
            time: 0,
        });
    }

    fn note_off(&mut self, pitch: u8) {
        self.note_events.push(NoteEvent {
            is_on: false,
            pitch,
            velocity: 0,
            time: 0,
        });
    }

    fn note_on_at(&mut self, pitch: u8, velocity: u8, frame_offset: u32) {
        self.note_events.push(NoteEvent {
            is_on: true,
            pitch,
            velocity,
            time: frame_offset,
        });
    }

    fn note_off_at(&mut self, pitch: u8, frame_offset: u32) {
        self.note_events.push(NoteEvent {
            is_on: false,
            pitch,
            velocity: 0,
            time: frame_offset,
        });
    }

    fn reset(&mut self) {
        self.note_events.clear();
    }

    fn is_instrument(&self) -> bool {
        self.is_instrument
    }

    fn prepare(&mut self, sample_rate: f64, _max_buffer_size: u32) {
        self.sample_rate = sample_rate;
    }

    fn activate(&mut self) -> bool {
        if self.plugin_ptr.is_null() {
            return false;
        }
        let plugin_ref = unsafe { &*self.plugin_ptr };
        let ok =
            unsafe { (plugin_ref.activate.unwrap())(self.plugin_ptr, self.sample_rate, 32, 4096) };
        if ok {
            unsafe { (plugin_ref.start_processing.unwrap())(self.plugin_ptr) };
            self.active = true;
        }
        ok
    }

    fn deactivate(&mut self) {
        if self.plugin_ptr.is_null() || !self.active {
            return;
        }
        let plugin_ref = unsafe { &*self.plugin_ptr };
        unsafe {
            (plugin_ref.stop_processing.unwrap())(self.plugin_ptr);
            (plugin_ref.deactivate.unwrap())(self.plugin_ptr);
        }
        self.active = false;
    }
}

impl Drop for ClapPluginInstance {
    fn drop(&mut self) {
        if !self.plugin_ptr.is_null() {
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
