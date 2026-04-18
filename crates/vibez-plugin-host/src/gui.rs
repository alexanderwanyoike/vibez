use std::ffi::c_void;

use clap_sys::ext::gui::{
    clap_plugin_gui, clap_window, clap_window_handle, CLAP_EXT_GUI, CLAP_WINDOW_API_X11,
};
use clap_sys::plugin::clap_plugin;

use vibez_core::id::{EffectId, TrackId};

/// Identifies a specific plugin GUI (for use as HashMap key).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PluginGuiKey {
    Effect {
        track_id: TrackId,
        effect_id: EffectId,
    },
    Instrument {
        track_id: TrackId,
    },
}

/// A handle to a plugin's native GUI, kept on the UI thread.
pub enum PluginGuiHandle {
    Clap(ClapGuiHandle),
    Vst3(Vst3GuiHandle),
}

// Safety: GUI handles are only used from the UI thread.
// The raw pointers within point to plugin objects that remain valid
// for the plugin's lifetime and are only accessed from the main thread.
unsafe impl Send for PluginGuiHandle {}

impl PluginGuiHandle {
    /// Whether the plugin actually supports a GUI.
    pub fn has_gui(&self) -> bool {
        match self {
            PluginGuiHandle::Clap(h) => !h.gui_ext.is_null(),
            PluginGuiHandle::Vst3(h) => !h.edit_controller.is_null(),
        }
    }

    /// Query the preferred GUI size from the plugin.
    pub fn get_size(&self) -> Option<(u32, u32)> {
        match self {
            PluginGuiHandle::Clap(h) => h.get_size(),
            PluginGuiHandle::Vst3(h) => h.get_size(),
        }
    }

    /// Create the plugin GUI (call before attach/show).
    pub fn create_gui(&self) -> bool {
        match self {
            PluginGuiHandle::Clap(h) => h.create_gui(),
            PluginGuiHandle::Vst3(_h) => _h.create_view(),
        }
    }

    /// Attach the plugin GUI to an X11 window.
    pub fn attach_to_x11(&self, window_id: u32) -> bool {
        match self {
            PluginGuiHandle::Clap(h) => h.attach_to_x11(window_id),
            PluginGuiHandle::Vst3(h) => h.attach_to_x11(window_id),
        }
    }

    /// Show the plugin GUI.
    pub fn show(&self) -> bool {
        match self {
            PluginGuiHandle::Clap(h) => h.show(),
            PluginGuiHandle::Vst3(_) => true, // VST3 shows on attach
        }
    }

    /// Hide the plugin GUI.
    pub fn hide(&self) -> bool {
        match self {
            PluginGuiHandle::Clap(h) => h.hide(),
            PluginGuiHandle::Vst3(_) => true,
        }
    }

    /// Destroy the plugin GUI (call when closing window).
    pub fn destroy(&mut self) {
        match self {
            PluginGuiHandle::Clap(h) => h.destroy(),
            PluginGuiHandle::Vst3(h) => h.destroy(),
        }
    }

    pub fn is_open(&self) -> bool {
        match self {
            PluginGuiHandle::Clap(h) => h.open,
            PluginGuiHandle::Vst3(h) => h.open,
        }
    }
}

// ── CLAP GUI Handle ──

pub struct ClapGuiHandle {
    plugin_ptr: *const clap_plugin,
    gui_ext: *const clap_plugin_gui,
    pub open: bool,
}

unsafe impl Send for ClapGuiHandle {}

impl ClapGuiHandle {
    /// Create a new CLAP GUI handle from a raw `*const c_void` pointer.
    /// This is the safe-to-call-from-anywhere entry point that avoids
    /// exposing `clap_plugin` types to crates that don't depend on `clap-sys`.
    ///
    /// # Safety
    /// `raw_ptr` must be a valid `*const clap_plugin` cast to `*const c_void`.
    pub unsafe fn from_raw(raw_ptr: *const c_void) -> Option<Self> {
        Self::new(raw_ptr as *const clap_plugin)
    }

    /// Create a new CLAP GUI handle by querying the plugin's GUI extension.
    ///
    /// # Safety
    /// `plugin_ptr` must be a valid CLAP plugin pointer that remains valid.
    pub unsafe fn new(plugin_ptr: *const clap_plugin) -> Option<Self> {
        if plugin_ptr.is_null() {
            eprintln!("vibez: ClapGuiHandle::new — plugin_ptr is null");
            return None;
        }
        let plugin_ref = &*plugin_ptr;
        let ext_ptr = (plugin_ref.get_extension.unwrap())(plugin_ptr, CLAP_EXT_GUI.as_ptr())
            as *const clap_plugin_gui;
        if ext_ptr.is_null() {
            eprintln!("vibez: ClapGuiHandle::new — plugin has no GUI extension");
            return None;
        }
        // Check if X11 is supported
        let gui = &*ext_ptr;
        let supported = (gui.is_api_supported.unwrap())(
            plugin_ptr,
            CLAP_WINDOW_API_X11.as_ptr(),
            false, // embedded, not floating
        );
        if !supported {
            eprintln!("vibez: ClapGuiHandle::new — X11 API not supported");
            return None;
        }
        eprintln!("vibez: ClapGuiHandle::new — GUI handle created OK");
        Some(Self {
            plugin_ptr,
            gui_ext: ext_ptr,
            open: false,
        })
    }

    fn get_size(&self) -> Option<(u32, u32)> {
        if self.gui_ext.is_null() {
            return None;
        }
        let mut w: u32 = 0;
        let mut h: u32 = 0;
        let gui = unsafe { &*self.gui_ext };
        let ok = unsafe { (gui.get_size.unwrap())(self.plugin_ptr, &mut w, &mut h) };
        if ok && w > 0 && h > 0 {
            Some((w, h))
        } else {
            None
        }
    }

    fn create_gui(&self) -> bool {
        if self.gui_ext.is_null() {
            return false;
        }
        let gui = unsafe { &*self.gui_ext };
        unsafe {
            (gui.create.unwrap())(self.plugin_ptr, CLAP_WINDOW_API_X11.as_ptr(), false)
        }
    }

    fn attach_to_x11(&self, window_id: u32) -> bool {
        if self.gui_ext.is_null() {
            return false;
        }
        let gui = unsafe { &*self.gui_ext };
        let window = clap_window {
            api: CLAP_WINDOW_API_X11.as_ptr(),
            specific: clap_window_handle {
                x11: window_id as clap_sys::ext::gui::clap_xwnd,
            },
        };
        unsafe { (gui.set_parent.unwrap())(self.plugin_ptr, &window) }
    }

    fn show(&self) -> bool {
        if self.gui_ext.is_null() {
            return false;
        }
        let gui = unsafe { &*self.gui_ext };
        if let Some(show_fn) = gui.show {
            unsafe { show_fn(self.plugin_ptr) }
        } else {
            true
        }
    }

    fn hide(&self) -> bool {
        if self.gui_ext.is_null() {
            return false;
        }
        let gui = unsafe { &*self.gui_ext };
        if let Some(hide_fn) = gui.hide {
            unsafe { hide_fn(self.plugin_ptr) }
        } else {
            true
        }
    }

    fn destroy(&mut self) {
        if self.gui_ext.is_null() || !self.open {
            return;
        }
        let gui = unsafe { &*self.gui_ext };
        unsafe { (gui.destroy.unwrap())(self.plugin_ptr) };
        self.open = false;
    }
}

// ── VST3 GUI Handle ──

/// VST3 IEditController IID: {DCD7BBE3-7742-448D-A874-AACC979C759E}
const IEDIT_CONTROLLER_IID: [u8; 16] = [
    0xDC, 0xD7, 0xBB, 0xE3, 0x77, 0x42, 0x44, 0x8D, 0xA8, 0x74, 0xAA, 0xCC, 0x97, 0x9C, 0x75,
    0x9E,
];

pub struct Vst3GuiHandle {
    /// IEditController COM pointer (own refcount, stays on UI thread).
    edit_controller: *mut c_void,
    /// IPlugView COM pointer (created on open, destroyed on close).
    plug_view: *mut c_void,
    pub open: bool,
}

unsafe impl Send for Vst3GuiHandle {}

impl Vst3GuiHandle {
    /// Extract an IEditController from a VST3 component via queryInterface.
    ///
    /// # Safety
    /// `component` must be a valid IComponent COM pointer.
    pub unsafe fn new(component: *mut c_void) -> Option<Self> {
        if component.is_null() {
            eprintln!("vibez: Vst3GuiHandle::new — component is null");
            return None;
        }
        // FUnknown::queryInterface(iid, &mut obj) - vtable[0]
        type QueryInterfaceFn = unsafe extern "system" fn(
            *mut c_void,
            *const u8,
            *mut *mut c_void,
        ) -> i32;
        let vtbl = *(component as *const *const *const c_void);
        let query_interface: QueryInterfaceFn = std::mem::transmute(*vtbl.add(0));

        let mut edit_controller: *mut c_void = std::ptr::null_mut();
        let hr = query_interface(
            component,
            IEDIT_CONTROLLER_IID.as_ptr(),
            &mut edit_controller,
        );
        if hr != 0 || edit_controller.is_null() {
            eprintln!(
                "vibez: Vst3GuiHandle::new — queryInterface(IEditController) failed (hr={hr})"
            );
            return None;
        }
        eprintln!("vibez: Vst3GuiHandle::new — got IEditController OK");
        // edit_controller now has its own refcount from queryInterface
        Some(Self {
            edit_controller,
            plug_view: std::ptr::null_mut(),
            open: false,
        })
    }

    fn get_size(&self) -> Option<(u32, u32)> {
        if self.plug_view.is_null() {
            return None;
        }
        // IPlugView::getSize(rect) - vtable[9]
        // rect is ViewRect: { left: i32, top: i32, right: i32, bottom: i32 }
        #[repr(C)]
        struct ViewRect {
            left: i32,
            top: i32,
            right: i32,
            bottom: i32,
        }
        type GetSizeFn = unsafe extern "system" fn(*mut c_void, *mut ViewRect) -> i32;
        let vtbl = unsafe { *(self.plug_view as *const *const *const c_void) };
        let get_size: GetSizeFn = unsafe { std::mem::transmute(*vtbl.add(9)) };
        let mut rect = ViewRect {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        let hr = unsafe { get_size(self.plug_view, &mut rect) };
        if hr == 0 {
            let w = (rect.right - rect.left).max(0) as u32;
            let h = (rect.bottom - rect.top).max(0) as u32;
            if w > 0 && h > 0 {
                return Some((w, h));
            }
        }
        None
    }

    fn create_view(&self) -> bool {
        if self.edit_controller.is_null() {
            return false;
        }
        // We create the view on open, not here
        true
    }

    fn attach_to_x11(&self, window_id: u32) -> bool {
        if self.plug_view.is_null() {
            return false;
        }
        // IPlugView::attached(parent, type) - vtable[4]
        type AttachedFn =
            unsafe extern "system" fn(*mut c_void, *mut c_void, *const u8) -> i32;
        let vtbl = unsafe { *(self.plug_view as *const *const *const c_void) };
        let attached: AttachedFn = unsafe { std::mem::transmute(*vtbl.add(4)) };
        let platform_type = b"X11EmbedWindowID\0";
        let hr = unsafe {
            attached(
                self.plug_view,
                window_id as usize as *mut c_void,
                platform_type.as_ptr(),
            )
        };
        hr == 0
    }

    fn destroy(&mut self) {
        if !self.plug_view.is_null() {
            // IPlugView::removed() - vtable[5]
            type RemovedFn = unsafe extern "system" fn(*mut c_void) -> i32;
            let vtbl = unsafe { *(self.plug_view as *const *const *const c_void) };
            let removed: RemovedFn = unsafe { std::mem::transmute(*vtbl.add(5)) };
            unsafe { removed(self.plug_view) };

            // Release IPlugView
            type ReleaseFn = unsafe extern "system" fn(*mut c_void) -> u32;
            let release: ReleaseFn = unsafe { std::mem::transmute(*vtbl.add(2)) };
            unsafe { release(self.plug_view) };
            self.plug_view = std::ptr::null_mut();
        }
        self.open = false;
    }

    /// Create the IPlugView from IEditController and attach to window.
    /// Call this when opening the GUI for the first time.
    pub fn open_view(&mut self, window_id: u32) -> bool {
        if self.edit_controller.is_null() {
            eprintln!("vibez: open_view — edit_controller is null");
            return false;
        }

        // If we already have a plug_view, destroy it first
        if !self.plug_view.is_null() {
            self.destroy();
        }

        // IEditController::createView(name) -> IPlugView*
        // IEditController vtable: FUnknown[0-2] + IPluginBase[3-4] + IEditController[5-17]
        // createView is at index 17 in the vtable
        type CreateViewFn =
            unsafe extern "system" fn(*mut c_void, *const u8) -> *mut c_void;
        let vtbl = unsafe { *(self.edit_controller as *const *const *const c_void) };
        let create_view: CreateViewFn = unsafe { std::mem::transmute(*vtbl.add(17)) };
        let editor_name = b"editor\0";
        eprintln!("vibez: open_view — calling createView(\"editor\")");
        let view = unsafe { create_view(self.edit_controller, editor_name.as_ptr()) };
        if view.is_null() {
            eprintln!("vibez: open_view — createView returned null");
            return false;
        }
        eprintln!("vibez: open_view — got IPlugView OK");
        self.plug_view = view;

        // Check if X11 is supported: IPlugView::isPlatformTypeSupported(type) - vtable[3]
        type IsPlatformSupportedFn =
            unsafe extern "system" fn(*mut c_void, *const u8) -> i32;
        let view_vtbl = unsafe { *(self.plug_view as *const *const *const c_void) };
        let is_supported: IsPlatformSupportedFn =
            unsafe { std::mem::transmute(*view_vtbl.add(3)) };
        let platform_type = b"X11EmbedWindowID\0";
        let hr = unsafe { is_supported(self.plug_view, platform_type.as_ptr()) };
        eprintln!("vibez: open_view — isPlatformTypeSupported(X11) = {hr}");
        if hr != 0 {
            eprintln!("vibez: open_view — X11 embedding not supported by plugin");
            // Not supported, clean up
            type ReleaseFn = unsafe extern "system" fn(*mut c_void) -> u32;
            let release: ReleaseFn = unsafe { std::mem::transmute(*view_vtbl.add(2)) };
            unsafe { release(self.plug_view) };
            self.plug_view = std::ptr::null_mut();
            return false;
        }

        // Attach to X11 window
        eprintln!("vibez: open_view — attaching to X11 window {window_id}");
        if !self.attach_to_x11(window_id) {
            eprintln!("vibez: open_view — attach_to_x11 failed");
            type ReleaseFn = unsafe extern "system" fn(*mut c_void) -> u32;
            let release: ReleaseFn = unsafe { std::mem::transmute(*view_vtbl.add(2)) };
            unsafe { release(self.plug_view) };
            self.plug_view = std::ptr::null_mut();
            return false;
        }
        eprintln!("vibez: open_view — attached successfully");

        self.open = true;
        true
    }
}

impl Drop for Vst3GuiHandle {
    fn drop(&mut self) {
        self.destroy();
        if !self.edit_controller.is_null() {
            // Release IEditController
            type ReleaseFn = unsafe extern "system" fn(*mut c_void) -> u32;
            let vtbl = unsafe { *(self.edit_controller as *const *const *const c_void) };
            let release: ReleaseFn = unsafe { std::mem::transmute(*vtbl.add(2)) };
            unsafe { release(self.edit_controller) };
            self.edit_controller = std::ptr::null_mut();
        }
    }
}
