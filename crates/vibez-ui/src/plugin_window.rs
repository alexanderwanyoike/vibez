use std::collections::HashMap;
use std::ffi::c_void;

use vibez_plugin_host::gui::PluginGuiKey;
use vibez_plugin_host::PluginGuiHandle;
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{self, ConnectionExt as _};
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;

/// Events emitted by the plugin window manager during poll_events().
#[derive(Debug, Clone)]
pub enum PluginWindowEvent {
    /// The user closed a plugin window via the window manager (X button).
    Closed(PluginGuiKey),
}

/// Raw plugin pointer — transferred from the background loader thread to the
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

/// Tracks an open plugin GUI window.
struct OpenPluginWindow {
    x11_window_id: u32,
    gui_handle: PluginGuiHandle,
    #[allow(dead_code)]
    title: String,
}

/// Synchronous plugin window manager that runs on the UI thread.
/// All X11 and plugin GUI calls happen directly on the caller's thread
/// (the iced UI thread, which is the process main thread).
pub struct PluginWindowManager {
    conn: RustConnection,
    screen_num: usize,
    wm_protocols: u32,
    wm_delete_window: u32,
    windows: HashMap<PluginGuiKey, OpenPluginWindow>,
}

impl PluginWindowManager {
    /// Create a new window manager on the current thread.
    /// Returns None if no X11 display is available.
    pub fn new() -> Option<Self> {
        let (conn, screen_num) = match RustConnection::connect(None) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("vibez: no X11 display for plugin GUIs: {e}");
                return None;
            }
        };

        let wm_protocols = intern_atom(&conn, "WM_PROTOCOLS").unwrap_or(0);
        let wm_delete_window = intern_atom(&conn, "WM_DELETE_WINDOW").unwrap_or(0);

        Some(Self {
            conn,
            screen_num,
            wm_protocols,
            wm_delete_window,
            windows: HashMap::new(),
        })
    }

    /// Open a plugin GUI window. Synchronous — creates the GUI handle from
    /// the raw pointer, creates the X11 window, attaches, and shows.
    /// Returns true on success.
    pub fn open(&mut self, key: PluginGuiKey, raw_ptr: PluginRawPtr, title: String) -> bool {
        // If already open, raise
        if let Some(existing) = self.windows.get(&key) {
            let _ = self.conn.configure_window(
                existing.x11_window_id,
                &xproto::ConfigureWindowAux::new().stack_mode(xproto::StackMode::ABOVE),
            );
            let _ = self.conn.map_window(existing.x11_window_id);
            let _ = self.conn.flush();
            return true;
        }

        // Create the GUI handle from the raw pointer on THIS thread (UI thread).
        eprintln!("vibez: open_window — creating GUI handle on UI thread");
        let mut handle: PluginGuiHandle = match raw_ptr {
            PluginRawPtr::Clap(ptr) => {
                match unsafe { vibez_plugin_host::gui::ClapGuiHandle::from_raw(ptr) } {
                    Some(h) => PluginGuiHandle::Clap(h),
                    None => {
                        eprintln!("vibez: open_window — CLAP GUI handle creation failed");
                        return false;
                    }
                }
            }
            PluginRawPtr::Vst3(ptr) => {
                match unsafe { vibez_plugin_host::gui::Vst3GuiHandle::new(ptr) } {
                    Some(h) => PluginGuiHandle::Vst3(h),
                    None => {
                        eprintln!("vibez: open_window — VST3 GUI handle creation failed");
                        return false;
                    }
                }
            }
        };

        // Create the plugin GUI.
        // Log a warning if it takes more than 2s (JUCE plugins may hang here
        // if their MessageManager cannot initialize properly).
        eprintln!("vibez: open_window — calling create_gui()");
        let create_start = std::time::Instant::now();
        let ok = handle.create_gui();
        let elapsed = create_start.elapsed();
        eprintln!("vibez: open_window — create_gui() returned {ok} in {elapsed:?}");
        if !ok {
            eprintln!("vibez: plugin GUI create() failed");
            return false;
        }

        let (width, height) = handle.get_size().unwrap_or((800, 600));
        eprintln!("vibez: open_window — size: {width}x{height}");

        let screen = &self.conn.setup().roots[self.screen_num];
        let window_id = match self.conn.generate_id() {
            Ok(id) => id,
            Err(e) => {
                eprintln!("vibez: failed to generate X11 window id: {e}");
                return false;
            }
        };

        // Create the X11 window
        if let Err(e) = self.conn.create_window(
            x11rb::COPY_DEPTH_FROM_PARENT,
            window_id,
            screen.root,
            100,
            100,
            width as u16,
            height as u16,
            0,
            xproto::WindowClass::INPUT_OUTPUT,
            0,
            &xproto::CreateWindowAux::new().event_mask(xproto::EventMask::STRUCTURE_NOTIFY),
        ) {
            eprintln!("vibez: failed to create X11 window: {e}");
            return false;
        }

        // Set window title
        let _ = self.conn.change_property8(
            xproto::PropMode::REPLACE,
            window_id,
            xproto::AtomEnum::WM_NAME,
            xproto::AtomEnum::STRING,
            title.as_bytes(),
        );

        // Set WM_DELETE_WINDOW protocol
        if self.wm_protocols != 0 && self.wm_delete_window != 0 {
            let _ = self.conn.change_property32(
                xproto::PropMode::REPLACE,
                window_id,
                self.wm_protocols,
                xproto::AtomEnum::ATOM,
                &[self.wm_delete_window],
            );
        }

        // Map the window and flush BEFORE plugin attachment
        let _ = self.conn.map_window(window_id);
        let _ = self.conn.flush();

        // Attach plugin GUI to the window
        eprintln!("vibez: open_window — calling attach_to_x11({window_id})");
        let attached = handle.attach_to_x11(window_id);
        eprintln!("vibez: open_window — attach_to_x11 returned {attached}");
        if !attached {
            if let PluginGuiHandle::Vst3(ref mut vst3) = handle {
                eprintln!("vibez: open_window — VST3 fallback: calling open_view");
                if !vst3.open_view(window_id) {
                    eprintln!("vibez: failed to attach plugin GUI to X11 window");
                    let _ = self.conn.destroy_window(window_id);
                    let _ = self.conn.flush();
                    return false;
                }
            } else {
                eprintln!("vibez: failed to attach CLAP plugin GUI to X11 window");
                let _ = self.conn.destroy_window(window_id);
                let _ = self.conn.flush();
                return false;
            }
        }

        eprintln!("vibez: open_window — calling show()");
        handle.show();
        let _ = self.conn.flush();

        // Mark as open
        match &mut handle {
            PluginGuiHandle::Clap(h) => h.open = true,
            PluginGuiHandle::Vst3(h) => h.open = true,
        }

        self.windows.insert(
            key,
            OpenPluginWindow {
                x11_window_id: window_id,
                gui_handle: handle,
                title,
            },
        );

        eprintln!("vibez: open_window — SUCCESS, window {window_id} opened for {key:?}");
        true
    }

    /// Close a specific plugin GUI window.
    pub fn close(&mut self, key: PluginGuiKey) {
        if let Some(mut win) = self.windows.remove(&key) {
            win.gui_handle.destroy();
            let _ = self.conn.destroy_window(win.x11_window_id);
            let _ = self.conn.flush();
        }
    }

    /// Raise an existing window to the front.
    pub fn raise(&self, key: PluginGuiKey) {
        if let Some(win) = self.windows.get(&key) {
            let _ = self.conn.configure_window(
                win.x11_window_id,
                &xproto::ConfigureWindowAux::new().stack_mode(xproto::StackMode::ABOVE),
            );
            let _ = self.conn.map_window(win.x11_window_id);
            let _ = self.conn.flush();
        }
    }

    /// Close all plugin GUI windows.
    pub fn close_all(&mut self) {
        let keys: Vec<PluginGuiKey> = self.windows.keys().copied().collect();
        for key in keys {
            self.close(key);
        }
    }

    /// Check if a specific plugin GUI window is open.
    pub fn is_open(&self, key: PluginGuiKey) -> bool {
        self.windows.contains_key(&key)
    }

    /// Poll for X11 events (non-blocking). Returns events for closed windows.
    pub fn poll_events(&mut self) -> Vec<PluginWindowEvent> {
        // VST3 Linux GUIs live off the host run loop: fire their
        // timers and fd handlers every tick or JUCE editors freeze.
        for window in self.windows.values() {
            window.gui_handle.service_runloop();
        }

        let mut events = Vec::new();
        loop {
            match self.conn.poll_for_event() {
                Ok(Some(event)) => {
                    if let x11rb::protocol::Event::ClientMessage(cm) = event {
                        if cm.format == 32 && cm.data.as_data32()[0] == self.wm_delete_window {
                            let key = self
                                .windows
                                .iter()
                                .find(|(_, w)| w.x11_window_id == cm.window)
                                .map(|(k, _)| *k);
                            if let Some(key) = key {
                                self.close(key);
                                events.push(PluginWindowEvent::Closed(key));
                            }
                        }
                    }
                }
                Ok(None) => break,
                Err(e) => {
                    eprintln!("vibez: X11 event error: {e}");
                    break;
                }
            }
        }
        events
    }

    /// Close windows for all effects/instruments on a given track.
    pub fn close_track_effects(&mut self, track_id: vibez_core::id::TrackId) {
        let keys: Vec<PluginGuiKey> = self
            .windows
            .keys()
            .filter(|k| match k {
                PluginGuiKey::Effect { track_id: tid, .. } => *tid == track_id,
                PluginGuiKey::Instrument { track_id: tid } => *tid == track_id,
            })
            .copied()
            .collect();
        for key in keys {
            self.close(key);
        }
    }
}

impl Drop for PluginWindowManager {
    fn drop(&mut self) {
        self.close_all();
    }
}

/// Intern an X11 atom by name.
fn intern_atom(conn: &RustConnection, name: &str) -> Option<u32> {
    conn.intern_atom(false, name.as_bytes())
        .ok()?
        .reply()
        .ok()
        .map(|r| r.atom)
}
