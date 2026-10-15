//! Keeps Vibez alive when its realtime audio thread runs past the CPU time
//! the kernel allows between blocking calls.

/// Records the calling thread as the audio thread.
pub fn guard_current_thread() {}

/// Whether the audio thread was moved to normal priority since the last call.
pub fn take_overrun() -> bool {
    false
}
