use std::ops::Deref;
use std::path::Path;

pub(crate) struct Vst3Module {
    lib: libloading::Library,
    #[cfg(target_os = "macos")]
    _bundle: core_foundation::bundle::CFBundle,
}

impl Vst3Module {
    pub(crate) fn init(lib: libloading::Library, _path: &Path) -> Result<Self, String> {
        #[cfg(target_os = "macos")]
        {
            use core_foundation::base::TCFType;
            let bundle = crate::macos_bundle::open(_path)?;
            type Entry = unsafe extern "system" fn(core_foundation::bundle::CFBundleRef) -> bool;
            let entry = unsafe { lib.get::<Entry>(b"bundleEntry\0") }
                .map_err(|e| format!("Missing VST3 bundleEntry: {e}"))?;
            unsafe { lib.get::<unsafe extern "system" fn() -> bool>(b"bundleExit\0") }
                .map_err(|e| format!("Missing VST3 bundleExit: {e}"))?;
            if !unsafe { entry(bundle.as_concrete_TypeRef()) } {
                return Err("VST3 bundleEntry returned false".into());
            }
            return Ok(Self {
                lib,
                _bundle: bundle,
            });
        }
        #[cfg(all(unix, not(target_os = "macos")))]
        let lib = {
            use libloading::os::unix::Library as UnixLibrary;
            let handle = UnixLibrary::from(lib).into_raw();
            let lib = libloading::Library::from(unsafe { UnixLibrary::from_raw(handle) });
            type Entry = unsafe extern "system" fn(*mut std::ffi::c_void) -> bool;
            if let Ok(entry) = unsafe { lib.get::<Entry>(b"ModuleEntry\0") } {
                if !unsafe { entry(handle) } {
                    return Err("ModuleEntry returned false".into());
                }
            }
            lib
        };
        #[cfg(windows)]
        {
            type Entry = unsafe extern "system" fn() -> bool;
            if let Ok(entry) = unsafe { lib.get::<Entry>(b"InitDll\0") } {
                if !unsafe { entry() } {
                    return Err("InitDll returned false".into());
                }
            }
        }
        #[cfg(not(target_os = "macos"))]
        Ok(Self { lib })
    }
}

impl Deref for Vst3Module {
    type Target = libloading::Library;
    fn deref(&self) -> &Self::Target {
        &self.lib
    }
}

impl Drop for Vst3Module {
    fn drop(&mut self) {
        #[cfg(target_os = "macos")]
        const EXIT: &[u8] = b"bundleExit\0";
        #[cfg(all(unix, not(target_os = "macos")))]
        const EXIT: &[u8] = b"ModuleExit\0";
        #[cfg(windows)]
        const EXIT: &[u8] = b"ExitDll\0";
        type Exit = unsafe extern "system" fn() -> bool;
        // Module teardown must precede unloading the library and releasing
        // the CFBundle, including when factory lookup or initialization fails.
        if let Ok(exit) = unsafe { self.lib.get::<Exit>(EXIT) } {
            unsafe { exit() };
        }
    }
}
