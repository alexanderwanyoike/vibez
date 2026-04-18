use std::path::Path;

use crate::format::{PluginCategory, PluginFormat, PluginId};
use crate::info::PluginInfo;

/// Scan a `.clap` shared library for plugin metadata.
///
/// CLAP plugins expose a `clap_entry` symbol with `clap_plugin_entry_t`.
/// We load the library, call `init()`, get the factory, and iterate
/// plugin descriptors to extract metadata.
pub fn scan_clap(path: &Path) -> Result<Vec<PluginInfo>, String> {
    // Safety: Loading shared libraries is inherently unsafe.
    // We wrap the entire operation in catch_unwind for robustness.
    let result = std::panic::catch_unwind(|| scan_clap_inner(path));
    match result {
        Ok(r) => r,
        Err(_) => Err(format!("Panic while scanning CLAP plugin: {path:?}")),
    }
}

fn scan_clap_inner(path: &Path) -> Result<Vec<PluginInfo>, String> {
    // Load the shared library
    let lib = unsafe {
        libloading::Library::new(path).map_err(|e| format!("Failed to load library: {e}"))?
    };

    // Look up the clap_entry symbol
    let entry: libloading::Symbol<'_, *const clap_sys::entry::clap_plugin_entry> = unsafe {
        lib.get(b"clap_entry\0")
            .map_err(|e| format!("No clap_entry symbol: {e}"))?
    };

    let entry_ptr = *entry;
    if entry_ptr.is_null() {
        return Err("clap_entry is null".into());
    }

    let entry_ref = unsafe { &*entry_ptr };

    // Initialize the entry
    let path_cstr = std::ffi::CString::new(
        path.to_str().unwrap_or_default(),
    )
    .map_err(|e| format!("Invalid path: {e}"))?;

    let init_ok = unsafe { (entry_ref.init.unwrap())(path_cstr.as_ptr()) };
    if !init_ok {
        return Err("clap_entry.init() failed".into());
    }

    // Get the plugin factory
    let factory_id = clap_sys::factory::plugin_factory::CLAP_PLUGIN_FACTORY_ID;
    let factory_ptr =
        unsafe { (entry_ref.get_factory.unwrap())(factory_id.as_ptr()) } as *const clap_sys::factory::plugin_factory::clap_plugin_factory;

    if factory_ptr.is_null() {
        unsafe { (entry_ref.deinit.unwrap())() };
        drop(lib);
        return Err("No plugin factory".into());
    }

    let factory = unsafe { &*factory_ptr };
    let count = unsafe { (factory.get_plugin_count.unwrap())(factory_ptr) } as usize;

    let mut results = Vec::new();

    for i in 0..count {
        let desc_ptr =
            unsafe { (factory.get_plugin_descriptor.unwrap())(factory_ptr, i as u32) };
        if desc_ptr.is_null() {
            continue;
        }

        let desc = unsafe { &*desc_ptr };

        let id_str = if !desc.id.is_null() {
            unsafe { std::ffi::CStr::from_ptr(desc.id) }
                .to_str()
                .unwrap_or("unknown")
                .to_string()
        } else {
            format!("clap-{i}")
        };

        let name = if !desc.name.is_null() {
            unsafe { std::ffi::CStr::from_ptr(desc.name) }
                .to_str()
                .unwrap_or("Unknown")
                .to_string()
        } else {
            "Unknown".to_string()
        };

        let vendor = if !desc.vendor.is_null() {
            unsafe { std::ffi::CStr::from_ptr(desc.vendor) }
                .to_str()
                .unwrap_or("")
                .to_string()
        } else {
            String::new()
        };

        // Determine category from features
        let category = determine_clap_category(desc);

        results.push(PluginInfo {
            id: PluginId {
                format: PluginFormat::Clap,
                uid: id_str,
            },
            name,
            vendor,
            category,
            format: PluginFormat::Clap,
            path: path.to_path_buf(),
        });
    }

    unsafe { (entry_ref.deinit.unwrap())() };

    // Drop the library so the OS fully unloads it and resets all statics.
    // Plugins like LSP use a singletone (one-shot) init guard — if we leaked
    // the library (mem::forget), the singletone stays "initialized" but the
    // factory pointer is NULL after deinit, causing "No plugin factory" on
    // subsequent loads. Dropping cleanly allows a fresh init later.
    drop(lib);

    Ok(results)
}

fn determine_clap_category(desc: &clap_sys::plugin::clap_plugin_descriptor) -> PluginCategory {
    if desc.features.is_null() {
        return PluginCategory::Effect;
    }

    let mut is_instrument = false;
    let mut is_effect = false;
    let mut idx = 0;

    loop {
        let feature_ptr = unsafe { *desc.features.add(idx) };
        if feature_ptr.is_null() {
            break;
        }
        let feature = unsafe { std::ffi::CStr::from_ptr(feature_ptr) }
            .to_str()
            .unwrap_or("");

        if feature == "instrument" {
            is_instrument = true;
        }
        if feature == "audio-effect" || feature == "effect" {
            is_effect = true;
        }
        idx += 1;
    }

    match (is_instrument, is_effect) {
        (true, true) => PluginCategory::Both,
        (true, false) => PluginCategory::Instrument,
        _ => PluginCategory::Effect,
    }
}
