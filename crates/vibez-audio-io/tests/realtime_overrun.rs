//! A realtime audio thread that runs past its CPU budget keeps running
//! instead of the kernel killing Vibez.

#[cfg(target_os = "linux")]
#[test]
fn an_overrun_signal_is_caught_instead_of_killing_vibez() {
    vibez_audio_io::realtime::guard_current_thread();
    assert!(!vibez_audio_io::realtime::take_overrun());
    unsafe { libc::raise(libc::SIGXCPU) };
    assert!(vibez_audio_io::realtime::take_overrun());
    assert!(!vibez_audio_io::realtime::take_overrun(), "reported once");
}

/// Busy blocks of nearly a buffer each, back to back, as an overloaded
/// callback runs. Needs rtkit, so machines without it skip the check.
#[cfg(target_os = "linux")]
#[test]
fn a_realtime_thread_that_rests_after_overrunning_survives_sustained_overload() {
    let survived = std::thread::spawn(|| {
        let Ok(_handle) = audio_thread_priority::promote_current_thread_to_real_time(512, 44100)
        else {
            eprintln!("skipped: this machine cannot grant realtime priority");
            return true;
        };
        vibez_audio_io::realtime::guard_current_thread();
        let mut overran = false;
        let start = std::time::Instant::now();
        while start.elapsed() < std::time::Duration::from_millis(600) {
            let block = std::time::Instant::now();
            while block.elapsed() < std::time::Duration::from_millis(9) {
                std::hint::spin_loop();
            }
            overran |= vibez_audio_io::realtime::take_overrun();
            if overran {
                std::thread::sleep(vibez_audio_io::realtime::OVERRUN_REST);
            }
        }
        overran
    })
    .join()
    .unwrap();
    assert!(survived, "the thread overran its budget and kept running");
}
