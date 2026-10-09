use clap_sys::ext::latency::{clap_plugin_latency, CLAP_EXT_LATENCY};
use clap_sys::plugin::clap_plugin;

pub(super) fn query(plugin: *const clap_plugin) -> Result<u32, &'static str> {
    // CLAP permits this query on the main thread after activation. Rendering
    // consumes the cached result instead of invoking the extension itself.
    unsafe {
        let extension = ((*plugin).get_extension.unwrap())(plugin, CLAP_EXT_LATENCY.as_ptr())
            as *const clap_plugin_latency;
        if extension.is_null() {
            Ok(0)
        } else {
            (*extension)
                .get
                .map(|get| get(plugin))
                .ok_or("CLAP latency extension has no query callback")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::{c_char, c_void};

    unsafe extern "C" fn absent(_: *const clap_plugin, _: *const c_char) -> *const c_void {
        std::ptr::null()
    }
    unsafe extern "C" fn invalid(_: *const clap_plugin, _: *const c_char) -> *const c_void {
        static EXTENSION: clap_plugin_latency = clap_plugin_latency { get: None };
        (&EXTENSION as *const clap_plugin_latency).cast()
    }

    #[test]
    fn absent_optional_report_is_zero_and_invalid_extension_is_an_error() {
        let mut plugin: clap_plugin = unsafe { std::mem::zeroed() };
        plugin.get_extension = Some(absent);
        assert_eq!(query(&plugin), Ok(0));
        plugin.get_extension = Some(invalid);
        assert!(query(&plugin).is_err());
    }
}
