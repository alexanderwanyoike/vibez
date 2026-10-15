//! Delay snapshots expose applied timing without advancing or changing audio.

use vibez_core::audio_context::DeviceAudioContext;
use vibez_dsp::{delay::{param, DelayEffect}, effect::AudioEffect};

fn assert_copy_bounded<T: Copy>(_: &T) {
    assert!(std::mem::size_of::<T>() <= 512);
}

#[test]
fn delay_snapshot_observes_applied_crossfade_without_changing_audio() {
    let mut observed = DelayEffect::new(1000.0);
    let mut untouched = DelayEffect::new(1000.0);
    let mut targets = [
        3.0, 0.0, 1.0, 0.0, 4.0, 0.0, 0.0, 1.0, 500.0,
        4.0, 0.0, 0.0, 120.0, 0.0, 8000.0, 0.0, 0.0,
    ];
    for effect in [&mut observed, &mut untouched] {
        for (index, value) in targets.into_iter().enumerate() {
            assert!(effect.set_param(index, value));
        }
        effect.set_audio_context(DeviceAudioContext {
            musical_sample: 700, continuous_sample: 900,
            sample_rate: 1000, bpm: 137.0, playing: true,
        });
    }
    assert!(observed.delay_snapshot().is_none());
    observed.process(&mut [], 2);
    assert!(observed.delay_snapshot().is_none());
    assert!(DelayEffect::new(0.0).delay_snapshot().is_none());

    for effect in [&mut observed, &mut untouched] {
        effect.process(&mut [0.25, -0.125], 2);
    }
    let initial = observed.delay_snapshot().expect("one complete frame is observable");
    assert_copy_bounded(&initial);
    assert_eq!(initial.sample_rate, 1000);
    assert_eq!(initial.bpm, 137.0);
    assert_eq!(initial.processed_frames, 1);
    assert_eq!(initial.targets, targets);
    for line in initial.lines {
        assert_eq!(line.old_samples, 3.0);
        assert_eq!(line.destination_samples, 3.0);
        assert_eq!(line.target_samples, 3.0);
        assert_eq!(line.pending_samples, None);
        assert_eq!(line.remaining_frames, 0);
        assert!(!line.moving);
    }

    for effect in [&mut observed, &mut untouched] {
        assert!(effect.set_param(param::TIME_MS, 5.0));
        assert_eq!(effect.get_param(param::TIME_MS), 5.0);
    }
    targets[param::TIME_MS] = 5.0;
    for frame in 0..20 {
        let mut actual = [0.0; 2];
        let mut reference = actual;
        observed.process(&mut actual, 2);
        untouched.process(&mut reference, 2);
        assert_eq!(actual.map(f32::to_bits), reference.map(f32::to_bits));
        let progress = frame as f64 / 19.0;
        // The original impulse reaches fixed old/new taps at different fade frames.
        let weight = if frame == 2 { 1.0 - progress }
            else if frame == 4 { progress } else { 0.0 };
        for (value, impulse) in actual.into_iter().zip([0.25, -0.125]) {
            assert!((f64::from(value) - impulse * weight).abs() <= 2e-7);
        }
        let state = observed.delay_snapshot().expect("processed timing stays observable");
        assert_copy_bounded(&state);
        assert_eq!(state.sample_rate, 1000);
        assert_eq!(state.bpm, 137.0);
        assert_eq!(state.processed_frames, frame as u64 + 2);
        assert_eq!(state.targets, targets);
        for line in state.lines {
            assert_eq!(line.target_samples, 5.0);
            assert_eq!(line.pending_samples, None);
            assert_eq!(line.remaining_frames, 19 - frame);
            assert_eq!(line.moving, frame < 19);
            if frame < 19 {
                assert_eq!(line.old_samples, 3.0);
                assert_eq!(line.destination_samples, 5.0);
                assert!((line.progress - progress).abs() <= 1e-12);
            } else {
                assert_eq!(line.old_samples, 5.0);
                assert_eq!(line.destination_samples, 5.0);
            }
        }
        for _ in 0..4 {
            let repeated = observed.delay_snapshot().unwrap();
            assert_eq!(repeated.processed_frames, state.processed_frames);
            assert_eq!(repeated.targets, state.targets);
        }
    }
    let mut actual = [0.0625, -0.03125, 0.125, -0.0625, 0.0, 0.0];
    let mut reference = actual;
    observed.process(&mut actual, 2);
    untouched.process(&mut reference, 2);
    assert_eq!(actual.map(f32::to_bits), reference.map(f32::to_bits));

    observed.reset();
    assert!(observed.delay_snapshot().is_none());
    observed.process(&mut [0.0, 0.0], 2);
    let restarted = observed.delay_snapshot().unwrap();
    assert_eq!(restarted.processed_frames, 1);
    assert_eq!(restarted.targets, targets);
    assert!(restarted.lines.iter().all(|line| !line.moving
        && line.old_samples == 5.0 && line.destination_samples == 5.0));
}
