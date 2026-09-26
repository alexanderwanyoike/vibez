use std::ffi::c_void;
use vibez_plugin_host::gui::{ClapGuiHandle, PluginGuiKey, Vst3GuiHandle};
use vibez_plugin_host::PluginGuiHandle;

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

/// # Safety
/// The plugin must stay loaded until the returned GUI handle is destroyed.
/// All handle operations, including creation and destruction, require the UI thread.
unsafe fn gui_handle(raw: PluginRawPtr) -> Option<PluginGuiHandle> {
    match raw {
        PluginRawPtr::Clap(ptr) => ClapGuiHandle::from_raw(ptr).map(PluginGuiHandle::Clap),
        PluginRawPtr::Vst3(ptr) => Vst3GuiHandle::new(ptr).map(PluginGuiHandle::Vst3),
    }
}

fn requested_resize(
    handle: &PluginGuiHandle,
    clap_requests: &[(usize, u32, u32)],
) -> Option<(u32, u32)> {
    handle.take_pending_resize().or_else(|| {
        handle.clap_plugin_ptr().and_then(|ptr| {
            clap_requests
                .iter()
                .rev()
                .find(|(p, _, _)| *p == ptr)
                .map(|&(_, w, h)| (w, h))
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use vibez_plugin_host::{
        gui::{GuiApi, GuiParent},
        test_fixtures::{Clap, State, Vst},
    };

    #[test]
    fn clap_resize_uses_latest_request_for_this_plugin() {
        let state = State::new(GuiApi::native());
        let plugin = Clap::new(&state);
        let handle = unsafe { gui_handle(PluginRawPtr::Clap(plugin.raw_ptr())) }.unwrap();
        let ptr = plugin.raw_ptr() as usize;
        let requests = [(ptr, 320, 240), (ptr, 640, 480), (0, 800, 600)];
        assert_eq!(requested_resize(&handle, &requests), Some((640, 480)));
        assert_eq!(requested_resize(&handle, &[(0, 800, 600)]), None);
    }

    #[test]
    fn vst3_resize_comes_from_its_frame_and_is_consumed_once() {
        let state = State::new(GuiApi::X11);
        let plugin = Vst::new(&state);
        let mut handle = unsafe { gui_handle(PluginRawPtr::Vst3(plugin.ptr())) }.unwrap();
        assert!(unsafe { handle.attach_to_parent(GuiParent::X11(42)) });
        assert!(plugin.request_resize(640, 480));
        assert_eq!(
            requested_resize(&handle, &[(0, 800, 600)]),
            Some((640, 480))
        );
        assert_eq!(requested_resize(&handle, &[]), None);
    }
}
