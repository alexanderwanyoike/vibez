use std::ffi::c_void;
use vibez_plugin_host::gui::PluginGuiKey;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(not(target_os = "macos"))]
mod x11;
#[cfg(target_os = "macos")]
pub use macos::PluginWindowManager;
#[cfg(not(target_os = "macos"))]
pub use x11::PluginWindowManager;

/// Events emitted by the plugin window manager during poll_events().
#[derive(Debug, Clone)]
pub enum PluginWindowEvent {
    /// The user closed a plugin window via the window manager (X button).
    Closed(PluginGuiKey),
}

/// Raw plugin pointer - transferred from the background loader thread to the
/// UI thread. The GUI handle is created on the UI thread (the process main
/// thread) so that all plugin GUI calls happen on the same OS thread.
#[derive(Clone, Copy)]
pub enum PluginRawPtr {
    Clap(*const c_void),
    Vst3(*mut c_void),
}

// Safety: raw pointers are just integers being transferred between threads.
// All actual dereferences happen exclusively on the UI thread.
unsafe impl Send for PluginRawPtr {}
