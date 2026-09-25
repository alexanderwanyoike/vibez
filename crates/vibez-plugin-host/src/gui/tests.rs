use super::*;
use std::cell::RefCell;

#[derive(Default)]
struct Calls {
    apis: Vec<String>,
    parent: usize,
    destroys: usize,
    removed: usize,
    releases: usize,
    x11: bool,
}

unsafe extern "C" fn clap_extension(
    _: *const clap_plugin,
    _: *const std::ffi::c_char,
) -> *const c_void {
    &CLAP_GUI as *const _ as *const c_void
}

unsafe fn clap_calls(plugin: *const clap_plugin) -> &'static RefCell<Calls> {
    &*((*plugin).plugin_data as *const RefCell<Calls>)
}

unsafe extern "C" fn clap_supports(
    plugin: *const clap_plugin,
    api: *const std::ffi::c_char,
    floating: bool,
) -> bool {
    let api = CStr::from_ptr(api).to_string_lossy().into_owned();
    let expected = if clap_calls(plugin).borrow().x11 {
        "x11"
    } else {
        "cocoa"
    };
    let supported = api == expected && !floating;
    clap_calls(plugin).borrow_mut().apis.push(api);
    supported
}

unsafe extern "C" fn clap_destroy(plugin: *const clap_plugin) {
    clap_calls(plugin).borrow_mut().destroys += 1;
}

unsafe extern "C" fn clap_parent(plugin: *const clap_plugin, parent: *const clap_window) -> bool {
    let mut calls = clap_calls(plugin).borrow_mut();
    calls
        .apis
        .push(CStr::from_ptr((*parent).api).to_string_lossy().into_owned());
    calls.parent = if calls.x11 {
        (*parent).specific.x11 as usize
    } else {
        (*parent).specific.cocoa as usize
    };
    true
}

static CLAP_GUI: clap_plugin_gui = clap_plugin_gui {
    is_api_supported: Some(clap_supports),
    get_preferred_api: None,
    create: Some(clap_supports),
    destroy: Some(clap_destroy),
    set_scale: None,
    get_size: None,
    can_resize: None,
    get_resize_hints: None,
    adjust_size: None,
    set_size: None,
    set_parent: Some(clap_parent),
    set_transient: None,
    suggest_title: None,
    show: None,
    hide: None,
};

fn clap_plugin_fixture(calls: &RefCell<Calls>) -> clap_plugin {
    clap_plugin {
        desc: std::ptr::null(),
        plugin_data: calls as *const _ as *mut c_void,
        init: None,
        destroy: None,
        activate: None,
        deactivate: None,
        start_processing: None,
        stop_processing: None,
        reset: None,
        process: None,
        get_extension: Some(clap_extension),
        on_main_thread: None,
    }
}

#[test]
fn cocoa_only_clap_editor_is_accepted() {
    let calls = RefCell::new(Calls::default());
    let plugin = clap_plugin_fixture(&calls);
    let handle = unsafe { ClapGuiHandle::new_for_api(&plugin, GuiApi::Cocoa) };
    assert!(
        handle.is_some(),
        "a Cocoa-only CLAP editor must not be queried for X11"
    );
    assert_eq!(calls.borrow().apis, ["cocoa"]);
}

#[repr(C)]
struct View {
    vtbl: *const *const c_void,
    calls: RefCell<Calls>,
}

#[repr(C)]
struct Controller {
    vtbl: *const *const c_void,
    view: *mut View,
}

unsafe extern "system" fn add_ref(_: *mut c_void) -> u32 {
    1
}
unsafe extern "system" fn release_controller(_: *mut c_void) -> u32 {
    1
}
unsafe extern "system" fn release_view(view: *mut c_void) -> u32 {
    (*(view as *mut View)).calls.borrow_mut().releases += 1;
    1
}
unsafe extern "system" fn create_view(controller: *mut c_void, _: *const u8) -> *mut c_void {
    (*(controller as *mut Controller)).view.cast()
}
unsafe extern "system" fn supports(view: *mut c_void, api: *const u8) -> i32 {
    let api = CStr::from_ptr(api.cast()).to_string_lossy().into_owned();
    let supported = api == "NSView";
    (*(view as *mut View)).calls.borrow_mut().apis.push(api);
    if supported {
        0
    } else {
        1
    }
}
unsafe extern "system" fn attached(view: *mut c_void, parent: *mut c_void, api: *const u8) -> i32 {
    let mut calls = (*(view as *mut View)).calls.borrow_mut();
    calls
        .apis
        .push(CStr::from_ptr(api.cast()).to_string_lossy().into_owned());
    calls.parent = parent as usize;
    0
}
unsafe extern "system" fn removed(view: *mut c_void) -> i32 {
    (*(view as *mut View)).calls.borrow_mut().removed += 1;
    0
}
unsafe extern "system" fn set_frame(_: *mut c_void, _: *mut c_void) -> i32 {
    0
}

#[test]
fn cocoa_only_vst3_editor_receives_nsview_parent() {
    let mut view_vtbl = [std::ptr::null(); 15];
    view_vtbl[2] = release_view as *const c_void;
    view_vtbl[3] = supports as *const c_void;
    view_vtbl[4] = attached as *const c_void;
    view_vtbl[5] = removed as *const c_void;
    view_vtbl[12] = set_frame as *const c_void;
    let mut view = View {
        vtbl: view_vtbl.as_ptr(),
        calls: RefCell::new(Calls::default()),
    };
    let mut controller_vtbl = [std::ptr::null(); 18];
    controller_vtbl[1] = add_ref as *const c_void;
    controller_vtbl[2] = release_controller as *const c_void;
    controller_vtbl[17] = create_view as *const c_void;
    let mut controller = Controller {
        vtbl: controller_vtbl.as_ptr(),
        view: &mut view,
    };
    let mut parent = 0u8;
    let parent = (&mut parent as *mut u8).cast();
    let mut handle =
        unsafe { Vst3GuiHandle::new((&mut controller as *mut Controller).cast()) }.unwrap();
    assert!(
        unsafe { handle.open_in_parent(GuiParent::Cocoa(parent)) },
        "an NSView-only VST3 editor must not be queried for X11"
    );
    assert_eq!(view.calls.borrow().apis, ["NSView", "NSView"]);
    assert_eq!(view.calls.borrow().parent, parent as usize);
    handle.destroy();
    handle.destroy();
    assert_eq!(view.calls.borrow().removed, 1);
    assert_eq!(view.calls.borrow().releases, 1);
}

#[test]
fn clap_cocoa_creation_parent_and_cleanup_use_one_api() {
    let calls = RefCell::new(Calls::default());
    let plugin = clap_plugin_fixture(&calls);
    let mut parent = 0u8;
    let parent = (&mut parent as *mut u8).cast();
    {
        let mut handle = unsafe { ClapGuiHandle::new_for_api(&plugin, GuiApi::Cocoa) }.unwrap();
        assert!(handle.create_gui());
        assert!(handle.attach_to_parent(GuiParent::Cocoa(parent)));
        assert!(!handle.attach_to_parent(GuiParent::X11(42)));
        assert_eq!(calls.borrow().apis, ["cocoa", "cocoa", "cocoa"]);
        assert_eq!(calls.borrow().parent, parent as usize);
    }
    assert_eq!(calls.borrow().destroys, 1);
}

#[test]
fn clap_created_gui_is_destroyed_even_before_attachment() {
    let calls = RefCell::new(Calls::default());
    let plugin = clap_plugin_fixture(&calls);
    let mut handle = unsafe { ClapGuiHandle::new_for_api(&plugin, GuiApi::Cocoa) }.unwrap();
    assert!(handle.create_gui());
    handle.destroy();
    drop(handle);
    assert_eq!(calls.borrow().destroys, 1);
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
    let calls = RefCell::new(Calls {
        x11: true,
        ..Default::default()
    });
    let plugin = clap_plugin_fixture(&calls);
    let mut handle = unsafe { ClapGuiHandle::new_for_api(&plugin, GuiApi::X11) }.unwrap();
    assert!(handle.create_gui());
    assert!(handle.attach_to_x11(42));
    assert_eq!(calls.borrow().apis, ["x11", "x11", "x11"]);
    assert_eq!(calls.borrow().parent, 42);
}
