/// Convert a canonical VST3 IID byte string (the GUID's printed hex
/// order) into the platform TUID layout. Windows stores the first
/// three GUID fields little-endian (COM layout); every other platform
/// uses the canonical byte order. Hand-built IIDs MUST go through
/// this or QueryInterface silently fails on Windows.
pub(crate) const fn vst3_tuid(b: [u8; 16]) -> [u8; 16] {
    if cfg!(target_os = "windows") {
        [
            b[3], b[2], b[1], b[0], b[5], b[4], b[7], b[6], b[8], b[9], b[10], b[11], b[12], b[13],
            b[14], b[15],
        ]
    } else {
        b
    }
}

pub mod buffer;
pub mod clap_host;
pub mod format;
pub mod gui;
pub mod info;
pub mod instance;
pub mod paths;
pub mod scan_helper;
pub mod scanner;
pub mod settings;
pub mod state;
pub mod vst3_host;
pub mod wrappers;

pub use clap_host::host_impl::{poll_clap_events, set_clap_main_thread};
pub use format::{PluginCategory, PluginFormat, PluginId};
pub use gui::{PluginGuiHandle, PluginGuiKey};
pub use info::PluginInfo;
pub use instance::PluginInstance;
pub use scanner::{scan_plugins, scan_plugins_sandboxed, ScanReport};
pub use settings::PluginSettings;
pub use state::{capture_plugin_state, PluginStatePtr};
pub use wrappers::effect::PluginEffectWrapper;
pub use wrappers::instrument::PluginInstrumentWrapper;
