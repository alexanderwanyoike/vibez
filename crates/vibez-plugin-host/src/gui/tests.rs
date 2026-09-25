use super::*;
use crate::test_fixtures::{Clap, State, Vst};

#[test]
fn cocoa_only_clap_editor_is_accepted() {
    let state = State::new(GuiApi::Cocoa);
    let plugin = Clap::new(&state);
    let handle = unsafe { ClapGuiHandle::new_for_api(plugin.ptr(), GuiApi::Cocoa) };
    assert!(
        handle.is_some(),
        "a Cocoa-only CLAP editor must not be queried for X11"
    );
    assert_eq!(*state.apis.borrow(), ["cocoa"]);
}

#[test]
fn cocoa_only_vst3_editor_receives_nsview_parent() {
    let state = State::new(GuiApi::Cocoa);
    let plugin = Vst::new(&state);
    let mut parent = 0u8;
    let parent = (&mut parent as *mut u8).cast();
    let mut handle = unsafe { Vst3GuiHandle::new(plugin.ptr()) }.unwrap();
    assert!(unsafe { handle.open_in_parent(GuiParent::Cocoa(parent)) });
    assert_eq!(*state.apis.borrow(), ["NSView", "NSView"]);
    assert_eq!(state.parent.get(), parent);
    handle.destroy();
    handle.destroy();
    assert_eq!(state.destroys.get(), 1);
    assert_eq!(state.releases.get(), 1);
}

#[test]
fn clap_cocoa_creation_parent_and_cleanup_use_one_api() {
    let state = State::new(GuiApi::Cocoa);
    let plugin = Clap::new(&state);
    let mut parent = 0u8;
    let parent = (&mut parent as *mut u8).cast();
    {
        let mut handle =
            unsafe { ClapGuiHandle::new_for_api(plugin.ptr(), GuiApi::Cocoa) }.unwrap();
        assert!(handle.create_gui());
        assert!(unsafe { handle.attach_to_parent(GuiParent::Cocoa(parent)) });
        assert!(!unsafe { handle.attach_to_parent(GuiParent::X11(42)) });
        assert_eq!(*state.apis.borrow(), ["cocoa", "cocoa", "cocoa"]);
        assert_eq!(state.parent.get(), parent);
    }
    assert_eq!(state.destroys.get(), 1);
}

#[test]
fn clap_created_gui_is_destroyed_even_before_attachment() {
    let state = State::new(GuiApi::Cocoa);
    let plugin = Clap::new(&state);
    let mut handle = unsafe { ClapGuiHandle::new_for_api(plugin.ptr(), GuiApi::Cocoa) }.unwrap();
    assert!(handle.create_gui());
    handle.destroy();
    drop(handle);
    assert_eq!(state.destroys.get(), 1);
}

#[test]
fn native_api_matches_target() {
    #[cfg(target_os = "macos")]
    assert_eq!(GuiApi::native(), GuiApi::Cocoa);
    #[cfg(not(target_os = "macos"))]
    assert_eq!(GuiApi::native(), GuiApi::X11);
}

#[test]
fn x11_clap_editor_still_receives_its_window_id() {
    let state = State::new(GuiApi::X11);
    let plugin = Clap::new(&state);
    let mut handle = unsafe { ClapGuiHandle::new_for_api(plugin.ptr(), GuiApi::X11) }.unwrap();
    assert!(handle.create_gui());
    assert!(unsafe { handle.attach_to_parent(GuiParent::X11(42)) });
    assert_eq!(*state.apis.borrow(), ["x11", "x11", "x11"]);
    assert_eq!(state.parent.get() as usize, 42);
}

#[test]
fn host_resize_negotiates_plugin_constraints_before_setting_size() {
    for vst3 in [false, true] {
        let state = State::new(GuiApi::Cocoa);
        let clap = Clap::new(&state);
        let vst = Vst::new(&state);
        let mut handle = unsafe {
            if vst3 {
                PluginGuiHandle::Vst3(Vst3GuiHandle::new(vst.ptr()).unwrap())
            } else {
                PluginGuiHandle::Clap(
                    ClapGuiHandle::new_for_api(clap.ptr(), GuiApi::Cocoa).unwrap(),
                )
            }
        };
        let mut parent = 0u8;
        assert!(handle.create_gui());
        assert!(unsafe {
            handle.attach_to_parent(GuiParent::Cocoa((&mut parent as *mut u8).cast()))
        });
        state.resize_step.set(16);
        assert_eq!(handle.resize_from_host(490, 370), Some((480, 368)));
        assert_eq!(state.size.get(), (480, 368));
        assert_eq!(*state.resize_calls.borrow(), ["adjust", "set_size"]);
    }
}

#[test]
fn gui_keys_resolve_their_owning_track() {
    let track_id = TrackId::new();
    assert_eq!(PluginGuiKey::Instrument { track_id }.track_id(), track_id);
    assert_eq!(
        PluginGuiKey::Effect {
            track_id,
            effect_id: EffectId::new()
        }
        .track_id(),
        track_id
    );
}
