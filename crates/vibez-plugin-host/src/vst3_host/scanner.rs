use std::path::Path;

use crate::format::{PluginCategory, PluginFormat, PluginId};
use crate::info::PluginInfo;

/// Scan a `.vst3` bundle for plugin metadata.
pub fn scan_vst3(path: &Path) -> Result<Vec<PluginInfo>, String> {
    let result = std::panic::catch_unwind(|| scan_vst3_inner(path));
    match result {
        Ok(r) => r,
        Err(_) => Err(format!("Panic while scanning VST3 plugin: {path:?}")),
    }
}

/// Run the platform-specific VST3 module entry export. The VST3 spec
/// requires this before `GetPluginFactory` (Linux: `ModuleEntry(handle)`,
/// Windows: `InitDll()`); DPF-based plugins such as Dragonfly Reverb
/// segfault inside the factory if it is skipped. Takes the library by
/// value because extracting the raw handle on unix consumes the wrapper.
pub(crate) fn vst3_module_init(lib: libloading::Library) -> Result<libloading::Library, String> {
    #[cfg(unix)]
    {
        use libloading::os::unix::Library as UnixLibrary;
        let handle = UnixLibrary::from(lib).into_raw();
        let lib = libloading::Library::from(unsafe { UnixLibrary::from_raw(handle) });
        type ModuleEntryFn = unsafe extern "system" fn(*mut std::ffi::c_void) -> bool;
        if let Ok(entry) = unsafe { lib.get::<ModuleEntryFn>(b"ModuleEntry\0") } {
            if !unsafe { entry(handle) } {
                return Err("ModuleEntry returned false".into());
            }
        }
        Ok(lib)
    }
    #[cfg(windows)]
    {
        type InitDllFn = unsafe extern "system" fn() -> bool;
        if let Ok(entry) = unsafe { lib.get::<InitDllFn>(b"InitDll\0") } {
            if !unsafe { entry() } {
                return Err("InitDll returned false".into());
            }
        }
        Ok(lib)
    }
}

/// Counterpart to [`vst3_module_init`]; call before the library is dropped.
pub(crate) fn vst3_module_exit(lib: &libloading::Library) {
    #[cfg(unix)]
    const NAME: &[u8] = b"ModuleExit\0";
    #[cfg(windows)]
    const NAME: &[u8] = b"ExitDll\0";
    type ExitFn = unsafe extern "system" fn() -> bool;
    if let Ok(exit) = unsafe { lib.get::<ExitFn>(NAME) } {
        unsafe { exit() };
    }
}

fn scan_vst3_inner(path: &Path) -> Result<Vec<PluginInfo>, String> {
    let module_path = find_vst3_module(path)?;

    let lib = unsafe {
        libloading::Library::new(&module_path)
            .map_err(|e| format!("Failed to load VST3 module: {e}"))?
    };
    let lib = vst3_module_init(lib)?;

    // GetPluginFactory returns a raw COM pointer to IPluginFactory.
    // We call through the vtable directly since the vst3 crate's traits require smart pointers.
    type GetFactoryFn = unsafe extern "system" fn() -> *mut std::ffi::c_void;

    let get_factory: libloading::Symbol<'_, GetFactoryFn> = unsafe {
        lib.get(b"GetPluginFactory\0")
            .map_err(|e| format!("No GetPluginFactory: {e}"))?
    };

    let factory_ptr = unsafe { get_factory() };
    if factory_ptr.is_null() {
        return Err("GetPluginFactory returned null".into());
    }

    // IPluginFactory vtable layout (COM):
    //   [0] queryInterface
    //   [1] addRef
    //   [2] release
    //   [3] getFactoryInfo
    //   [4] countClasses
    //   [5] getClassInfo
    //   [6] createInstance
    let vtbl_ptr = unsafe { *(factory_ptr as *const *const *const std::ffi::c_void) };

    // countClasses() -> int32
    type CountClassesFn = unsafe extern "system" fn(*mut std::ffi::c_void) -> i32;
    let count_classes: CountClassesFn = unsafe { std::mem::transmute(*vtbl_ptr.add(4)) };
    let count = unsafe { count_classes(factory_ptr) } as usize;

    // getClassInfo(index: int32, info: *mut PClassInfo) -> tresult
    type GetClassInfoFn =
        unsafe extern "system" fn(*mut std::ffi::c_void, i32, *mut PClassInfoRaw) -> i32;
    let get_class_info: GetClassInfoFn = unsafe { std::mem::transmute(*vtbl_ptr.add(5)) };

    let mut results = Vec::new();

    for i in 0..count {
        let mut info = PClassInfoRaw::zeroed();
        let hr = unsafe { get_class_info(factory_ptr, i as i32, &mut info) };
        if hr != 0 {
            continue;
        }

        let name = cstr_from_fixed(&info.name);
        let category_str = cstr_from_fixed(&info.category);

        let uid = format!(
            "{:02X}{:02X}{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}",
            info.cid[0], info.cid[1], info.cid[2], info.cid[3],
            info.cid[4], info.cid[5], info.cid[6], info.cid[7],
            info.cid[8], info.cid[9], info.cid[10], info.cid[11],
            info.cid[12], info.cid[13], info.cid[14], info.cid[15],
        );

        // Only include audio module components
        let is_audio = category_str == "Audio Module Class";
        if !is_audio {
            continue;
        }

        let category = PluginCategory::Effect;

        results.push(PluginInfo {
            id: PluginId {
                format: PluginFormat::Vst3,
                uid,
            },
            name: name.clone(),
            vendor: String::new(),
            category,
            format: PluginFormat::Vst3,
            path: path.to_path_buf(),
        });
    }

    // Release factory
    type ReleaseFn = unsafe extern "system" fn(*mut std::ffi::c_void) -> u32;
    let release: ReleaseFn = unsafe { std::mem::transmute(*vtbl_ptr.add(2)) };
    unsafe { release(factory_ptr) };

    // Drop the library so the OS fully unloads it and resets all statics.
    // Leaking via mem::forget can poison plugins that use one-shot init guards.
    vst3_module_exit(&lib);
    drop(lib);

    Ok(results)
}

/// Raw PClassInfo matching VST3 C layout.
#[repr(C)]
pub(crate) struct PClassInfoRaw {
    pub cid: [u8; 16],
    pub cardinality: i32,
    pub category: [u8; 32],
    pub name: [u8; 64],
}

impl PClassInfoRaw {
    pub fn zeroed() -> Self {
        Self {
            cid: [0; 16],
            cardinality: 0,
            category: [0; 32],
            name: [0; 64],
        }
    }
}

/// Find the actual shared library module inside a .vst3 bundle.
pub(crate) fn find_vst3_module(bundle_path: &Path) -> Result<std::path::PathBuf, String> {
    #[cfg(target_os = "linux")]
    {
        let arch_dir = bundle_path.join("Contents").join("x86_64-linux");
        if arch_dir.exists() {
            if let Ok(entries) = std::fs::read_dir(&arch_dir) {
                for entry in entries.flatten() {
                    let p = entry.path();
                    if p.extension().is_some_and(|e| e == "so") {
                        return Ok(p);
                    }
                }
            }
        }
    }

    #[cfg(target_os = "macos")]
    {
        let macos_dir = bundle_path.join("Contents").join("MacOS");
        if macos_dir.exists() {
            if let Ok(entries) = std::fs::read_dir(&macos_dir) {
                for entry in entries.flatten() {
                    return Ok(entry.path());
                }
            }
        }
    }

    #[cfg(target_os = "windows")]
    {
        let win_dir = bundle_path.join("Contents").join("x86_64-win");
        if win_dir.exists() {
            if let Ok(entries) = std::fs::read_dir(&win_dir) {
                for entry in entries.flatten() {
                    let p = entry.path();
                    if p.extension().is_some_and(|e| e == "vst3") {
                        return Ok(p);
                    }
                }
            }
        }
    }

    Err(format!(
        "Could not find module binary in VST3 bundle: {bundle_path:?}"
    ))
}

fn cstr_from_fixed(buf: &[u8]) -> String {
    let bytes: Vec<u8> = buf.iter().take_while(|&&b| b != 0).copied().collect();
    String::from_utf8_lossy(&bytes).to_string()
}
