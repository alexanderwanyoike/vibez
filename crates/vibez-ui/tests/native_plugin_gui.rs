#[cfg(target_os = "macos")]
#[path = "native_plugin_gui/fixtures.rs"]
mod fixtures;

#[cfg(target_os = "macos")]
fn main() {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSApplication, NSView};
    use objc2_foundation::NSSize;
    use vibez_core::id::TrackId;
    use vibez_plugin_host::PluginGuiKey;
    use vibez_ui::plugin_window::{PluginRawPtr, PluginWindowManager};

    let mtm = MainThreadMarker::new().expect("native GUI test must run on the process main thread");
    let _app = NSApplication::sharedApplication(mtm);
    let mut manager = PluginWindowManager::new().expect("AppKit must not require an X11 display");
    for vst3 in [false, true] {
        let state = fixtures::State::default();
        let clap = fixtures::clap(&state);
        let vst = fixtures::Vst::new(&state);
        let ptr = if vst3 {
            PluginRawPtr::Vst3(vst.ptr())
        } else {
            PluginRawPtr::Clap((&clap as *const clap_sys::plugin::clap_plugin).cast())
        };
        let key = PluginGuiKey::Instrument {
            track_id: TrackId::new(),
        };
        for expected_destroys in 1..=2 {
            assert!(manager.open(key, ptr, "Native plugin test".into()));
            assert!(state.correct_api.get());
            let parent = state.parent.get();
            assert!(!parent.is_null());
            let view = unsafe { &*(parent as *const NSView) };
            let window = view.window().unwrap();
            assert_eq!(view.bounds().size, NSSize::new(320.0, 240.0));
            manager.raise(key);
            window.setContentSize(NSSize::new(480.0, 360.0));
            manager.poll_events();
            assert_eq!(state.size.get(), (480, 360));
            window.performClose(None);
            assert_eq!(manager.poll_events().len(), 1);
            assert!(!manager.is_open(key));
            assert_eq!(state.destroys.get(), expected_destroys);
            assert!(state.parent_alive_at_destroy.get());
        }
        assert!(manager.open(key, ptr, "Track teardown".into()));
        if let PluginGuiKey::Instrument { track_id } = key {
            manager.close_track_effects(track_id);
        }
        assert_eq!(state.destroys.get(), 3);
        assert!(manager.open(key, ptr, "App teardown".into()));
        manager.close_all();
        assert_eq!(state.destroys.get(), 4);
    }
}

#[cfg(not(target_os = "macos"))]
fn main() {}
