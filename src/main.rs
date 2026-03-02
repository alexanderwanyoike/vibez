fn main() -> iced::Result {
    // Call XInitThreads() before any X11/OpenGL operations.
    // JUCE-based plugins (Dexed, Surge, etc.) call XInitThreads internally
    // during gui.create(). If Xlib has already been used by wgpu/GLX (iced's
    // renderer) before XInitThreads is called, it causes undefined behavior
    // and hangs. Calling it first ensures thread-safe Xlib for all users.
    #[cfg(target_os = "linux")]
    unsafe {
        // dlopen libX11 without adding a build dependency
        if let Ok(lib) = libloading::Library::new("libX11.so.6") {
            if let Ok(init) = lib.get::<unsafe extern "C" fn() -> std::ffi::c_int>(b"XInitThreads")
            {
                init();
            }
            // Keep the library loaded for the process lifetime
            std::mem::forget(lib);
        }
    }

    vibez_ui::run()
}
