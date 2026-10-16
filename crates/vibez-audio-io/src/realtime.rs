//! Keeps Vibez alive when its realtime audio thread runs past the CPU time
//! the kernel allows between sleeps (`RLIMIT_RTTIME`).
//!
//! audio_thread_priority sets the soft limit to one buffer's duration and the
//! hard limit to rtkit's maximum, so the process hears about an overrun
//! through SIGXCPU before the hard limit kills it. Left uncaught, SIGXCPU
//! terminates the process: one slow block, such as a plugin doing first-time
//! work when its editor opens, took Vibez down. An unprivileged thread cannot
//! leave realtime scheduling (RLIMIT_NICE is 0 under rtkit), but any sleep
//! resets the kernel's count, so after an overrun the callback rests briefly
//! after each block instead.

#[cfg(target_os = "linux")]
mod linux {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Once;

    static OVERRUN: AtomicBool = AtomicBool::new(false);
    static INSTALL: Once = Once::new();

    pub fn guard_current_thread() {
        INSTALL.call_once(|| unsafe {
            let mut action: libc::sigaction = std::mem::zeroed();
            action.sa_sigaction = on_overrun as extern "C" fn(libc::c_int) as usize;
            action.sa_flags = libc::SA_RESTART;
            libc::sigemptyset(&mut action.sa_mask);
            if libc::sigaction(libc::SIGXCPU, &action, std::ptr::null_mut()) != 0 {
                eprintln!("vibez: could not handle realtime overruns; a slow block may end Vibez");
            }
        });
    }

    pub fn take_overrun() -> bool {
        OVERRUN.swap(false, Ordering::AcqRel)
    }

    // Async-signal-safe: one atomic store.
    extern "C" fn on_overrun(_signal: libc::c_int) {
        OVERRUN.store(true, Ordering::Release);
    }
}

/// Handles the audio thread's realtime overruns instead of dying of them.
pub fn guard_current_thread() {
    #[cfg(target_os = "linux")]
    linux::guard_current_thread();
}

/// Whether the audio thread overran its realtime budget since the last call.
pub fn take_overrun() -> bool {
    #[cfg(target_os = "linux")]
    return linux::take_overrun();
    #[cfg(not(target_os = "linux"))]
    false
}

/// How long the callback sleeps after each block once it has overrun, enough
/// for the kernel to count the thread as having slept.
pub const OVERRUN_REST: std::time::Duration = std::time::Duration::from_micros(50);
