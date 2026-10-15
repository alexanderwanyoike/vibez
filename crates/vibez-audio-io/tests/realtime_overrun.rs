//! A realtime audio thread that runs past its CPU budget is moved to normal
//! priority instead of the kernel's signal killing Vibez.

#[cfg(target_os = "linux")]
#[test]
fn an_overrun_signal_demotes_the_audio_thread_instead_of_killing_vibez() {
    vibez_audio_io::realtime::guard_current_thread();
    assert!(!vibez_audio_io::realtime::take_overrun());
    unsafe { libc::raise(libc::SIGXCPU) };
    assert!(vibez_audio_io::realtime::take_overrun());
    assert!(!vibez_audio_io::realtime::take_overrun(), "reported once");
}
