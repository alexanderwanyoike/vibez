//! Background plugin loading service.
//!
//! Plugins load in two phases: phase 1 (dlopen + factory lookup) on a
//! background thread, phase 2 (init + state restore) on the UI thread
//! because JUCE binds its MessageManager to the initializing thread.
//! This module owns both phases plus the project-reload thread;
//! app.rs only applies the finished device to state and the engine.

use vibez_core::effect::PluginDeviceInfo;
use vibez_core::id::{EffectId, TrackId};
use vibez_plugin_host::{PluginFormat, PluginInfo};

use crate::plugin_window::PluginRawPtr;

/// Result of loading a plugin on a background thread.
/// For CLAP plugins, `clap_partial` carries an un-initialized plugin that
/// must be finished on the UI thread (for JUCE MessageManager compatibility).
pub(crate) struct PluginLoadResult {
    pub(crate) track_id: TrackId,
    pub(crate) effect_id: EffectId,
    pub(crate) plugin_name: String,
    /// Fully-loaded effect (VST3) or None (CLAP — see `clap_partial`).
    pub(crate) effect: Option<Box<dyn vibez_dsp::effect::AudioEffect>>,
    pub(crate) gui_raw_ptr: Option<PluginRawPtr>,
    /// CLAP two-phase: partially loaded plugin to be finished on UI thread.
    pub(crate) clap_partial: Option<vibez_plugin_host::clap_host::instance::PartialClapPlugin>,
    /// VST3 two-phase: dlopen'd module to be instantiated on the UI
    /// thread (JUCE MessageManager binds to the instantiating thread).
    pub(crate) vst3_partial: Option<vibez_plugin_host::vst3_host::instance::PartialVst3Plugin>,
    pub(crate) sample_rate: f64,
    /// Persistent identity for project save.
    pub(crate) device_ref: vibez_core::effect::PluginDeviceInfo,
    /// Pointer for live state capture at save time.
    pub(crate) state_ptr: Option<vibez_plugin_host::PluginStatePtr>,
    /// Saved state to restore (project reload), applied on the UI
    /// thread after phase-2 init.
    pub(crate) pending_state: Option<Vec<u8>>,
    /// Chain position to restore (project reload).
    pub(crate) position: Option<usize>,
}

/// Result of loading a plugin instrument on a background thread.
pub(crate) struct PluginInstrumentLoadResult {
    pub(crate) track_id: TrackId,
    pub(crate) plugin_name: String,
    /// Fully-loaded instrument (VST3) or None (CLAP — see `clap_partial`).
    pub(crate) instrument: Option<Box<dyn vibez_instruments::Instrument>>,
    pub(crate) gui_raw_ptr: Option<PluginRawPtr>,
    /// CLAP two-phase: partially loaded plugin to be finished on UI thread.
    pub(crate) clap_partial: Option<vibez_plugin_host::clap_host::instance::PartialClapPlugin>,
    /// VST3 two-phase: dlopen'd module to be instantiated on the UI thread.
    pub(crate) vst3_partial: Option<vibez_plugin_host::vst3_host::instance::PartialVst3Plugin>,
    pub(crate) sample_rate: f64,
    /// Persistent identity for project save.
    pub(crate) device_ref: vibez_core::effect::PluginDeviceInfo,
    /// Pointer for live state capture at save time.
    pub(crate) state_ptr: Option<vibez_plugin_host::PluginStatePtr>,
    /// Saved state to restore (project reload).
    pub(crate) pending_state: Option<Vec<u8>>,
}

/// Persistent identity for a plugin device, built from scan info.
pub(crate) fn plugin_device_ref(info: &PluginInfo) -> vibez_core::effect::PluginDeviceInfo {
    vibez_core::effect::PluginDeviceInfo {
        format: match info.format {
            PluginFormat::Clap => "clap".to_string(),
            PluginFormat::Vst3 => "vst3".to_string(),
        },
        uid: info.id.uid.clone(),
        path: info.path.clone(),
        name: info.name.clone(),
        state_b64: None,
    }
}

/// Phase 1 of plugin loading (runs on background thread).
/// For CLAP: only loads the DSO — NO CLAP API calls (not even create_plugin).
/// For VST3: fully loads (VST3 doesn't have JUCE MessageManager issues).
pub(crate) fn load_plugin_effect_bg(
    info: &PluginInfo,
    sample_rate: f64,
    saved_state: Option<Vec<u8>>,
) -> Result<PluginLoadResult, String> {
    match info.format {
        PluginFormat::Clap => {
            let partial = vibez_plugin_host::clap_host::instance::ClapPluginInstance::load_partial(
                &info.path,
                &info.id.uid,
                false,
            )?;
            Ok(PluginLoadResult {
                track_id: TrackId::default(), // filled in by caller
                effect_id: EffectId::new(),
                plugin_name: info.name.clone(),
                effect: None,
                gui_raw_ptr: None,
                clap_partial: Some(partial),
                vst3_partial: None,
                sample_rate,
                device_ref: plugin_device_ref(info),
                state_ptr: None,
                // CLAP state must be applied after init_on_main_thread.
                pending_state: saved_state,
                position: None,
            })
        }
        PluginFormat::Vst3 => {
            let partial = vibez_plugin_host::vst3_host::instance::Vst3PluginInstance::load_partial(
                &info.path,
                &info.id.uid,
                false,
            )?;
            Ok(PluginLoadResult {
                track_id: TrackId::default(),
                effect_id: EffectId::new(),
                plugin_name: info.name.clone(),
                effect: None,
                gui_raw_ptr: None,
                clap_partial: None,
                vst3_partial: Some(partial),
                sample_rate,
                device_ref: plugin_device_ref(info),
                state_ptr: None,
                pending_state: saved_state,
                position: None,
            })
        }
    }
}

/// Phase 1 of instrument loading (runs on background thread).
pub(crate) fn load_plugin_instrument_bg(
    info: &PluginInfo,
    sample_rate: f64,
    saved_state: Option<Vec<u8>>,
) -> Result<PluginInstrumentLoadResult, String> {
    match info.format {
        PluginFormat::Clap => {
            let partial = vibez_plugin_host::clap_host::instance::ClapPluginInstance::load_partial(
                &info.path,
                &info.id.uid,
                true,
            )?;
            Ok(PluginInstrumentLoadResult {
                track_id: TrackId::default(),
                plugin_name: info.name.clone(),
                instrument: None,
                gui_raw_ptr: None,
                clap_partial: Some(partial),
                vst3_partial: None,
                sample_rate,
                device_ref: plugin_device_ref(info),
                state_ptr: None,
                pending_state: saved_state,
            })
        }
        PluginFormat::Vst3 => {
            let partial = vibez_plugin_host::vst3_host::instance::Vst3PluginInstance::load_partial(
                &info.path,
                &info.id.uid,
                true,
            )?;
            Ok(PluginInstrumentLoadResult {
                track_id: TrackId::default(),
                plugin_name: info.name.clone(),
                instrument: None,
                gui_raw_ptr: None,
                clap_partial: None,
                vst3_partial: Some(partial),
                sample_rate,
                device_ref: plugin_device_ref(info),
                state_ptr: None,
                pending_state: saved_state,
            })
        }
    }
}

/// Reload persisted plugin devices on one background thread, in
/// file order, so results arrive in order and chain positions
/// restore deterministically.
pub(crate) fn spawn_device_reloads(
    effect_requests: Vec<(TrackId, EffectId, usize, PluginDeviceInfo)>,
    instrument_requests: Vec<(TrackId, PluginDeviceInfo)>,
    effect_tx: std::sync::mpsc::Sender<PluginLoadResult>,
    instrument_tx: std::sync::mpsc::Sender<PluginInstrumentLoadResult>,
    sample_rate: f64,
) {
    std::thread::spawn(move || {
        use base64::Engine;
        let decode = |dev: &vibez_core::effect::PluginDeviceInfo| {
            dev.state_b64.as_ref().and_then(|b64| {
                base64::engine::general_purpose::STANDARD
                    .decode(b64)
                    .map_err(|e| eprintln!("vibez: bad plugin state blob for {}: {e}", dev.name))
                    .ok()
            })
        };
        let scan_info = |dev: &vibez_core::effect::PluginDeviceInfo,
                         category: vibez_plugin_host::PluginCategory| {
            let format = match dev.format.as_str() {
                "clap" => PluginFormat::Clap,
                _ => PluginFormat::Vst3,
            };
            PluginInfo {
                id: vibez_plugin_host::PluginId {
                    format,
                    uid: dev.uid.clone(),
                },
                name: dev.name.clone(),
                vendor: String::new(),
                category,
                format,
                path: dev.path.clone(),
            }
        };

        for (track_id, effect_id, chain_pos, dev) in effect_requests {
            let info = scan_info(&dev, vibez_plugin_host::PluginCategory::Effect);
            match load_plugin_effect_bg(&info, sample_rate, decode(&dev)) {
                Ok(mut result) => {
                    result.track_id = track_id;
                    result.effect_id = effect_id;
                    result.position = Some(chain_pos);
                    let _ = effect_tx.send(result);
                }
                Err(e) => {
                    eprintln!("vibez: failed to reload plugin {}: {e}", dev.name);
                }
            }
        }
        for (track_id, dev) in instrument_requests {
            let info = scan_info(&dev, vibez_plugin_host::PluginCategory::Instrument);
            match load_plugin_instrument_bg(&info, sample_rate, decode(&dev)) {
                Ok(mut result) => {
                    result.track_id = track_id;
                    let _ = instrument_tx.send(result);
                }
                Err(e) => {
                    eprintln!("vibez: failed to reload plugin {}: {e}", dev.name);
                }
            }
        }
    });
}

/// Phase 2 for effects, on the UI thread: finish init, restore saved
/// state, and capture GUI/state pointers. `Ok(None)` means the result
/// carried nothing to apply.
#[allow(clippy::type_complexity)]
pub(crate) fn finish_effect_init(
    result: &mut PluginLoadResult,
) -> Result<
    Option<(
        Box<dyn vibez_dsp::effect::AudioEffect>,
        Option<PluginRawPtr>,
    )>,
    String,
> {
    let plugin_name = result.plugin_name.clone();
    if let Some(partial) = result.clap_partial.take() {
        let mut clap_inst =
            vibez_plugin_host::clap_host::instance::ClapPluginInstance::init_on_main_thread(
                partial,
                result.sample_rate,
                4096,
            )
            .map_err(|e| format!("CLAP init failed on UI thread: {e}"))?;
        if let Some(ref data) = result.pending_state {
            use vibez_plugin_host::PluginInstance;
            if !clap_inst.load_state(data) {
                eprintln!("vibez: {plugin_name} rejected saved state");
            }
        }
        let raw_ptr = Some(PluginRawPtr::Clap(
            clap_inst.plugin_ptr() as *const std::ffi::c_void
        ));
        result.state_ptr = Some(vibez_plugin_host::PluginStatePtr::Clap(
            clap_inst.plugin_ptr() as *const std::ffi::c_void,
        ));
        let wrapper = vibez_plugin_host::PluginEffectWrapper::new(Box::new(clap_inst));
        return Ok(Some((Box::new(wrapper), raw_ptr)));
    }
    if let Some(partial) = result.vst3_partial.take() {
        let mut vst3_inst =
            vibez_plugin_host::vst3_host::instance::Vst3PluginInstance::init_on_main_thread(
                partial,
                result.sample_rate,
                4096,
            )
            .map_err(|e| format!("VST3 init failed on UI thread: {e}"))?;
        if let Some(ref data) = result.pending_state {
            use vibez_plugin_host::PluginInstance;
            if !vst3_inst.load_state(data) {
                eprintln!("vibez: {plugin_name} rejected saved state");
            }
        }
        let ctrl = vst3_inst.controller_ptr();
        let raw_ptr = if ctrl.is_null() {
            None
        } else {
            Some(PluginRawPtr::Vst3(ctrl))
        };
        result.state_ptr = Some(vibez_plugin_host::PluginStatePtr::Vst3Component(
            vst3_inst.component_ptr(),
        ));
        let wrapper = vibez_plugin_host::PluginEffectWrapper::new(Box::new(vst3_inst));
        return Ok(Some((Box::new(wrapper), raw_ptr)));
    }
    if let Some(effect) = result.effect.take() {
        return Ok(Some((effect, result.gui_raw_ptr.take())));
    }
    Ok(None)
}

/// Phase 2 for instruments, on the UI thread.
#[allow(clippy::type_complexity)]
pub(crate) fn finish_instrument_init(
    result: &mut PluginInstrumentLoadResult,
) -> Result<Option<(Box<dyn vibez_instruments::Instrument>, Option<PluginRawPtr>)>, String> {
    let plugin_name = result.plugin_name.clone();
    if let Some(partial) = result.clap_partial.take() {
        let mut clap_inst =
            vibez_plugin_host::clap_host::instance::ClapPluginInstance::init_on_main_thread(
                partial,
                result.sample_rate,
                4096,
            )
            .map_err(|e| format!("CLAP instrument init failed on UI thread: {e}"))?;
        if let Some(ref data) = result.pending_state {
            use vibez_plugin_host::PluginInstance;
            if !clap_inst.load_state(data) {
                eprintln!("vibez: {plugin_name} rejected saved state");
            }
        }
        let raw_ptr = Some(PluginRawPtr::Clap(
            clap_inst.plugin_ptr() as *const std::ffi::c_void
        ));
        result.state_ptr = Some(vibez_plugin_host::PluginStatePtr::Clap(
            clap_inst.plugin_ptr() as *const std::ffi::c_void,
        ));
        let wrapper = vibez_plugin_host::PluginInstrumentWrapper::new(Box::new(clap_inst));
        return Ok(Some((Box::new(wrapper), raw_ptr)));
    }
    if let Some(partial) = result.vst3_partial.take() {
        let mut vst3_inst =
            vibez_plugin_host::vst3_host::instance::Vst3PluginInstance::init_on_main_thread(
                partial,
                result.sample_rate,
                4096,
            )
            .map_err(|e| format!("VST3 instrument init failed on UI thread: {e}"))?;
        if let Some(ref data) = result.pending_state {
            use vibez_plugin_host::PluginInstance;
            if !vst3_inst.load_state(data) {
                eprintln!("vibez: {plugin_name} rejected saved state");
            }
        }
        let ctrl = vst3_inst.controller_ptr();
        let raw_ptr = if ctrl.is_null() {
            None
        } else {
            Some(PluginRawPtr::Vst3(ctrl))
        };
        result.state_ptr = Some(vibez_plugin_host::PluginStatePtr::Vst3Component(
            vst3_inst.component_ptr(),
        ));
        let wrapper = vibez_plugin_host::PluginInstrumentWrapper::new(Box::new(vst3_inst));
        return Ok(Some((Box::new(wrapper), raw_ptr)));
    }
    if let Some(instrument) = result.instrument.take() {
        return Ok(Some((instrument, result.gui_raw_ptr.take())));
    }
    Ok(None)
}
