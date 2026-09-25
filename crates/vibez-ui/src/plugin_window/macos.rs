use std::cell::Cell;
use std::collections::HashMap;

use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{define_class, msg_send, DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{NSBackingStoreType, NSView, NSWindow, NSWindowDelegate, NSWindowStyleMask};
use objc2_foundation::{NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString};
use vibez_plugin_host::gui::{GuiParent, PluginGuiKey};
use vibez_plugin_host::PluginGuiHandle;

use super::{gui_handle, requested_resize, PluginRawPtr, PluginWindowEvent};

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "VibezPluginWindowDelegate"]
    #[thread_kind = MainThreadOnly]
    #[ivars = Cell<bool>]
    struct WindowDelegate;

    unsafe impl NSObjectProtocol for WindowDelegate {}
    unsafe impl NSWindowDelegate for WindowDelegate {
        #[unsafe(method(windowShouldClose:))]
        fn window_should_close(&self, _sender: &NSWindow) -> bool {
            // Keep the parent alive until the UI tick detaches the plugin.
            self.ivars().set(true);
            false
        }
    }
);

struct OpenPluginWindow {
    window: Retained<NSWindow>,
    view: Retained<NSView>,
    delegate: Retained<WindowDelegate>,
    gui_handle: PluginGuiHandle,
    size: (u32, u32),
}

impl Drop for OpenPluginWindow {
    fn drop(&mut self) {
        self.gui_handle.hide();
        self.gui_handle.destroy();
        self.window.setDelegate(None);
        self.window.close();
    }
}

pub struct PluginWindowManager {
    main_thread: MainThreadMarker,
    windows: HashMap<PluginGuiKey, OpenPluginWindow>,
}

impl PluginWindowManager {
    pub fn new() -> Option<Self> {
        Some(Self {
            main_thread: MainThreadMarker::new()?,
            windows: HashMap::new(),
        })
    }

    pub fn open(&mut self, key: PluginGuiKey, raw_ptr: PluginRawPtr, title: String) -> bool {
        if self.is_open(key) {
            self.raise(key);
            return true;
        }
        let Some(mut gui_handle) = (unsafe { gui_handle(raw_ptr) }) else {
            return false;
        };
        if !gui_handle.create_gui() {
            return false;
        }
        let size = gui_handle.get_size().unwrap_or((800, 600));
        let style = NSWindowStyleMask::Titled
            | NSWindowStyleMask::Closable
            | NSWindowStyleMask::Miniaturizable;
        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(self.main_thread),
                NSRect::new(NSPoint::new(0.0, 0.0), ns_size(size)),
                style,
                NSBackingStoreType::Buffered,
                false,
            )
        };
        // Rust owns the window. AppKit must not also release it on close.
        unsafe {
            window.setReleasedWhenClosed(false);
        }
        let Some(view) = window.contentView() else {
            window.close();
            return false;
        };
        let delegate = WindowDelegate::alloc(self.main_thread).set_ivars(Cell::new(false));
        let delegate: Retained<WindowDelegate> = unsafe { msg_send![super(delegate), init] };
        window.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
        window.setTitle(&NSString::from_str(&title));
        let mut opened = OpenPluginWindow {
            window,
            view,
            delegate,
            gui_handle,
            size,
        };
        let parent = GuiParent::Cocoa(Retained::as_ptr(&opened.view) as *mut _);
        if !unsafe { opened.gui_handle.attach_to_parent(parent) } || !opened.gui_handle.show() {
            return false;
        }
        // NSView and both plugin APIs use logical points, including on Retina displays.
        opened.size = opened.gui_handle.get_size().unwrap_or(size);
        opened.window.setContentSize(ns_size(opened.size));
        if opened.gui_handle.can_resize() {
            opened
                .window
                .setStyleMask(style | NSWindowStyleMask::Resizable);
        }
        opened.window.center();
        opened.window.makeKeyAndOrderFront(None);
        self.windows.insert(key, opened);
        true
    }

    pub fn close(&mut self, key: PluginGuiKey) {
        self.windows.remove(&key);
    }

    pub fn raise(&self, key: PluginGuiKey) {
        if let Some(opened) = self.windows.get(&key) {
            opened.window.deminiaturize(None);
            opened.window.makeKeyAndOrderFront(None);
        }
    }

    pub fn close_all(&mut self) {
        self.windows.clear();
    }

    pub fn is_open(&self, key: PluginGuiKey) -> bool {
        self.windows.contains_key(&key)
    }

    pub fn close_track_effects(&mut self, track_id: vibez_core::id::TrackId) {
        self.windows.retain(|key, _| key.track_id() != track_id);
    }

    pub fn poll_events(&mut self) -> Vec<PluginWindowEvent> {
        let closed: Vec<_> = self
            .windows
            .iter()
            .filter(|(_, w)| w.delegate.ivars().get())
            .map(|(key, _)| *key)
            .collect();
        let mut events = Vec::new();
        for key in closed {
            self.close(key);
            events.push(PluginWindowEvent::Closed(key));
        }
        let clap_requests = vibez_plugin_host::clap_host::host_impl::take_pending_gui_resizes();
        for opened in self.windows.values_mut() {
            let requested = requested_resize(&opened.gui_handle, &clap_requests);
            if let Some(size) = requested.filter(|(w, h)| *w > 0 && *h > 0) {
                opened.size = size;
                opened.window.setContentSize(ns_size(size));
            }
            let bounds = opened.view.bounds().size;
            let size = (bounds.width.round() as u32, bounds.height.round() as u32);
            if size != opened.size && size.0 > 0 && size.1 > 0 {
                if let Some(adjusted) = opened.gui_handle.resize_from_host(size.0, size.1) {
                    opened.size = adjusted;
                }
                if size != opened.size {
                    opened.window.setContentSize(ns_size(opened.size));
                }
            }
        }
        events
    }
}

fn ns_size((width, height): (u32, u32)) -> NSSize {
    NSSize::new(width as f64, height as f64)
}
