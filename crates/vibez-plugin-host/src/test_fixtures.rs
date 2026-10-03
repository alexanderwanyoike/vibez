use crate::gui::GuiApi;
use clap_sys::ext::gui::{clap_plugin_gui, clap_window};
use clap_sys::plugin::clap_plugin;
use std::cell::{Cell, RefCell};
use std::ffi::{c_char, c_void, CStr};
use std::marker::PhantomData;

pub type ParentProbe = unsafe fn(*mut c_void) -> bool;

pub struct State {
    pub api: GuiApi,
    frame: Cell<*mut c_void>,
    pub apis: RefCell<Vec<String>>,
    pub releases: Cell<usize>,
    pub resize_step: Cell<u32>,
    pub resize_calls: RefCell<Vec<&'static str>>,
    pub parent_probe: Cell<Option<ParentProbe>>,
    pub parent: Cell<*mut c_void>,
    pub correct_api: Cell<bool>,
    pub destroys: Cell<usize>,
    pub parent_alive_at_destroy: Cell<bool>,
    pub size: Cell<(u32, u32)>,
}

impl State {
    pub fn new(api: GuiApi) -> Self {
        Self {
            api,
            frame: Cell::new(std::ptr::null_mut()),
            apis: RefCell::default(),
            releases: Cell::new(0),
            resize_step: Cell::new(1),
            resize_calls: RefCell::default(),
            parent_probe: Cell::new(None),
            parent: Cell::new(std::ptr::null_mut()),
            correct_api: Cell::new(false),
            destroys: Cell::new(0),
            parent_alive_at_destroy: Cell::new(false),
            size: Cell::new((320, 240)),
        }
    }

    fn clap_api(&self) -> &str {
        match self.api {
            GuiApi::Cocoa => "cocoa",
            GuiApi::X11 => "x11",
        }
    }
    fn vst3_api(&self) -> &str {
        match self.api {
            GuiApi::Cocoa => "NSView",
            GuiApi::X11 => "X11EmbedWindowID",
        }
    }

    unsafe fn destroyed(&self) {
        if let Some(probe) = self.parent_probe.get() {
            self.parent_alive_at_destroy.set(probe(self.parent.get()));
        }
        self.destroys.set(self.destroys.get() + 1);
    }

    fn adjusted(&self, (w, h): (u32, u32)) -> (u32, u32) {
        self.resize_calls.borrow_mut().push("adjust");
        let step = self.resize_step.get().max(1);
        (w.max(step) / step * step, h.max(step) / step * step)
    }

    fn accept_size(&self, size: (u32, u32)) -> bool {
        self.resize_calls.borrow_mut().push("set_size");
        let step = self.resize_step.get().max(1);
        if size.0 == 0
            || size.1 == 0
            || !size.0.is_multiple_of(step)
            || !size.1.is_multiple_of(step)
        {
            return false;
        }
        self.size.set(size);
        true
    }
}

unsafe fn state(plugin: *const clap_plugin) -> &'static State {
    &*((*plugin).plugin_data as *const State)
}
unsafe extern "C" fn extension(_: *const clap_plugin, _: *const c_char) -> *const c_void {
    (&GUI as *const clap_plugin_gui).cast()
}
unsafe extern "C" fn supports(
    plugin: *const clap_plugin,
    api: *const c_char,
    floating: bool,
) -> bool {
    let api = CStr::from_ptr(api).to_string_lossy().into_owned();
    let supported = api == state(plugin).clap_api() && !floating;
    state(plugin).apis.borrow_mut().push(api);
    supported
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
    state(plugin).accept_size((w, h))
}
unsafe extern "C" fn adjust_size(plugin: *const clap_plugin, w: *mut u32, h: *mut u32) -> bool {
    (*w, *h) = state(plugin).adjusted((*w, *h));
    true
}
unsafe extern "C" fn set_parent(plugin: *const clap_plugin, parent: *const clap_window) -> bool {
    let state = state(plugin);
    let api = CStr::from_ptr((*parent).api).to_string_lossy().into_owned();
    state.correct_api.set(api == state.clap_api());
    state.apis.borrow_mut().push(api);
    state.parent.set(match state.api {
        GuiApi::Cocoa => (*parent).specific.cocoa,
        GuiApi::X11 => (*parent).specific.x11 as usize as *mut c_void,
    });
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
    adjust_size: Some(adjust_size),
    set_size: Some(set_size),
    set_parent: Some(set_parent),
    set_transient: None,
    suggest_title: None,
    show: None,
    hide: None,
};
pub struct Clap<'a> {
    plugin: Box<clap_plugin>,
    _state: PhantomData<&'a State>,
}
impl<'a> Clap<'a> {
    pub fn new(state: &'a State) -> Self {
        Self {
            plugin: Box::new(clap_plugin {
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
            }),
            _state: PhantomData,
        }
    }
    pub fn ptr(&self) -> *const clap_plugin {
        &*self.plugin
    }
    pub fn raw_ptr(&self) -> *const c_void {
        self.ptr().cast()
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
unsafe extern "system" fn release_view(view: *mut View) -> u32 {
    let state = &*(*view).state;
    state.releases.set(state.releases.get() + 1);
    1
}
unsafe extern "system" fn supported(view: *mut View, api: *const c_char) -> i32 {
    let state = &*(*view).state;
    let api = CStr::from_ptr(api).to_string_lossy().into_owned();
    let supported = api == state.vst3_api();
    state.apis.borrow_mut().push(api);
    if supported {
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
    let state = &*(*view).state;
    state.parent.set(parent);
    state.correct_api.set(supported(view, api) == 0);
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
    let accepted = (*(*view).state).accept_size((
        ((*rect).right - (*rect).left) as u32,
        ((*rect).bottom - (*rect).top) as u32,
    ));
    if accepted {
        0
    } else {
        1
    }
}
unsafe extern "system" fn constrain(view: *mut View, rect: *mut Rect) -> i32 {
    let (w, h) = (*(*view).state).adjusted((
        ((*rect).right - (*rect).left) as u32,
        ((*rect).bottom - (*rect).top) as u32,
    ));
    (*rect).right = (*rect).left + w as i32;
    (*rect).bottom = (*rect).top + h as i32;
    0
}
unsafe extern "system" fn frame(view: *mut View, frame: *mut c_void) -> i32 {
    (*(*view).state).frame.set(frame);
    0
}
unsafe extern "system" fn resizable(_: *mut View) -> i32 {
    0
}

pub struct Vst<'a> {
    _state: PhantomData<&'a State>,
    controller: Box<Controller>,
    view: Box<View>,
    _controller_vtbl: Box<[*const c_void; 18]>,
    _view_vtbl: Box<[*const c_void; 15]>,
}
impl<'a> Vst<'a> {
    pub fn new(state: &'a State) -> Self {
        let mut v = Box::new([std::ptr::null(); 15]);
        v[2] = release_view as *const c_void;
        v[3] = supported as *const c_void;
        v[4] = attached as *const c_void;
        v[5] = removed as *const c_void;
        v[9] = get_size as *const c_void;
        v[10] = on_size as *const c_void;
        v[12] = frame as *const c_void;
        v[13] = resizable as *const c_void;
        v[14] = constrain as *const c_void;
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
            _state: PhantomData,
            controller,
            view,
            _controller_vtbl: c,
            _view_vtbl: v,
        }
    }
    pub fn request_resize(&self, width: i32, height: i32) -> bool {
        let state = unsafe { &*self.view.state };
        let frame = state.frame.get();
        if frame.is_null() {
            return false;
        }
        let mut rect = Rect {
            left: 0,
            top: 0,
            right: width,
            bottom: height,
        };
        type Resize = unsafe extern "system" fn(*mut c_void, *mut View, *mut Rect) -> i32;
        unsafe {
            let vtbl = *(frame as *const *const *const c_void);
            let resize: Resize = std::mem::transmute(*vtbl.add(3));
            resize(frame, (&*self.view as *const View).cast_mut(), &mut rect) == 0
        }
    }

    pub fn ptr(&self) -> *mut c_void {
        (&*self.controller as *const Controller).cast_mut().cast()
    }
}
