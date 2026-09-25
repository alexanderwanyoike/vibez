use clap_sys::ext::gui::{clap_plugin_gui, clap_window};
use clap_sys::plugin::clap_plugin;
use objc2_app_kit::NSView;
use std::cell::Cell;
use std::ffi::{c_char, c_void, CStr};

#[derive(Default)]
pub struct State {
    pub parent: Cell<*mut c_void>,
    pub correct_api: Cell<bool>,
    pub destroys: Cell<usize>,
    pub parent_alive_at_destroy: Cell<bool>,
    pub size: Cell<(u32, u32)>,
}

impl State {
    unsafe fn destroyed(&self) {
        self.parent_alive_at_destroy.set(
            !self.parent.get().is_null()
                && (&*(self.parent.get() as *const NSView)).window().is_some(),
        );
        self.destroys.set(self.destroys.get() + 1);
    }
}

unsafe fn state(plugin: *const clap_plugin) -> &'static State {
    &*((*plugin).plugin_data as *const State)
}
unsafe extern "C" fn extension(_: *const clap_plugin, _: *const c_char) -> *const c_void {
    (&GUI as *const clap_plugin_gui).cast()
}
unsafe extern "C" fn supports(_: *const clap_plugin, api: *const c_char, floating: bool) -> bool {
    CStr::from_ptr(api).to_bytes() == b"cocoa" && !floating
}
unsafe extern "C" fn create(
    plugin: *const clap_plugin,
    api: *const c_char,
    floating: bool,
) -> bool {
    state(plugin).size.set((320, 240));
    supports(plugin, api, floating)
}
unsafe extern "C" fn destroy(plugin: *const clap_plugin) {
    state(plugin).destroyed();
}
unsafe extern "C" fn size(plugin: *const clap_plugin, w: *mut u32, h: *mut u32) -> bool {
    (*w, *h) = state(plugin).size.get();
    true
}
unsafe extern "C" fn can_resize(_: *const clap_plugin) -> bool {
    true
}
unsafe extern "C" fn set_size(plugin: *const clap_plugin, w: u32, h: u32) -> bool {
    state(plugin).size.set((w, h));
    true
}
unsafe extern "C" fn set_parent(plugin: *const clap_plugin, parent: *const clap_window) -> bool {
    state(plugin)
        .correct_api
        .set(CStr::from_ptr((*parent).api).to_bytes() == b"cocoa");
    state(plugin).parent.set((*parent).specific.cocoa);
    true
}
static GUI: clap_plugin_gui = clap_plugin_gui {
    is_api_supported: Some(supports),
    get_preferred_api: None,
    create: Some(create),
    destroy: Some(destroy),
    set_scale: None,
    get_size: Some(size),
    can_resize: Some(can_resize),
    get_resize_hints: None,
    adjust_size: None,
    set_size: Some(set_size),
    set_parent: Some(set_parent),
    set_transient: None,
    suggest_title: None,
    show: None,
    hide: None,
};
pub fn clap(state: &State) -> clap_plugin {
    clap_plugin {
        desc: std::ptr::null(),
        plugin_data: (state as *const State).cast_mut().cast(),
        init: None,
        destroy: None,
        activate: None,
        deactivate: None,
        start_processing: None,
        stop_processing: None,
        reset: None,
        process: None,
        get_extension: Some(extension),
        on_main_thread: None,
    }
}

#[repr(C)]
struct View {
    vtbl: *const *const c_void,
    state: *const State,
}
#[repr(C)]
struct Controller {
    vtbl: *const *const c_void,
    view: *mut View,
}
#[repr(C)]
struct Rect {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}
unsafe extern "system" fn reference(_: *mut c_void) -> u32 {
    1
}
unsafe extern "system" fn create_view(controller: *mut Controller, _: *const u8) -> *mut c_void {
    let view = (*controller).view;
    (*(*view).state).size.set((320, 240));
    view.cast()
}
unsafe extern "system" fn supported(_: *mut View, api: *const c_char) -> i32 {
    if CStr::from_ptr(api).to_bytes() == b"NSView" {
        0
    } else {
        1
    }
}
unsafe extern "system" fn attached(
    view: *mut View,
    parent: *mut c_void,
    api: *const c_char,
) -> i32 {
    (*(*view).state).parent.set(parent);
    (*(*view).state).correct_api.set(supported(view, api) == 0);
    0
}
unsafe extern "system" fn removed(view: *mut View) -> i32 {
    (*(*view).state).destroyed();
    0
}
unsafe extern "system" fn get_size(view: *mut View, rect: *mut Rect) -> i32 {
    let (w, h) = (*(*view).state).size.get();
    *rect = Rect {
        left: 0,
        top: 0,
        right: w as i32,
        bottom: h as i32,
    };
    0
}
unsafe extern "system" fn on_size(view: *mut View, rect: *mut Rect) -> i32 {
    (*(*view).state).size.set((
        ((*rect).right - (*rect).left) as u32,
        ((*rect).bottom - (*rect).top) as u32,
    ));
    0
}
unsafe extern "system" fn frame(_: *mut View, _: *mut c_void) -> i32 {
    0
}
unsafe extern "system" fn resizable(_: *mut View) -> i32 {
    0
}

pub struct Vst {
    controller: Box<Controller>,
    _view: Box<View>,
    _controller_vtbl: Box<[*const c_void; 18]>,
    _view_vtbl: Box<[*const c_void; 15]>,
}
impl Vst {
    pub fn new(state: &State) -> Self {
        let mut v = Box::new([std::ptr::null(); 15]);
        v[2] = reference as *const c_void;
        v[3] = supported as *const c_void;
        v[4] = attached as *const c_void;
        v[5] = removed as *const c_void;
        v[9] = get_size as *const c_void;
        v[10] = on_size as *const c_void;
        v[12] = frame as *const c_void;
        v[13] = resizable as *const c_void;
        let mut view = Box::new(View {
            vtbl: v.as_ptr(),
            state,
        });
        let mut c = Box::new([std::ptr::null(); 18]);
        c[1] = reference as *const c_void;
        c[2] = reference as *const c_void;
        c[17] = create_view as *const c_void;
        let controller = Box::new(Controller {
            vtbl: c.as_ptr(),
            view: &mut *view,
        });
        Self {
            controller,
            _view: view,
            _controller_vtbl: c,
            _view_vtbl: v,
        }
    }
    pub fn ptr(&self) -> *mut c_void {
        (&*self.controller as *const Controller).cast_mut().cast()
    }
}
