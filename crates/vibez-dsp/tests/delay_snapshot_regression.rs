//! Delay snapshots expose applied timing without advancing or changing audio.

use vibez_core::audio_context::DeviceAudioContext;
use vibez_dsp::{
    delay::{param, DelayEffect},
    effect::AudioEffect,
};

fn assert_copy_bounded<T: Copy>(_: &T) {
    assert!(std::mem::size_of::<T>() <= 512);
}

#[test]
fn delay_snapshot_observes_applied_crossfade_without_changing_audio() {
    let mut observed = DelayEffect::new(1000.0);
    let mut untouched = DelayEffect::new(1000.0);
    let mut targets = [
        3.0, 0.0, 1.0, 0.0, 4.0, 0.0, 0.0, 1.0, 500.0, 4.0, 0.0, 0.0, 120.0, 0.0, 8000.0, 0.0, 0.0,
    ];
    for effect in [&mut observed, &mut untouched] {
        for (index, value) in targets.into_iter().enumerate() {
            assert!(effect.set_param(index, value));
        }
        effect.set_audio_context(DeviceAudioContext {
            musical_sample: 700,
            continuous_sample: 900,
            sample_rate: 1000,
            bpm: 137.0,
            playing: true,
        });
    }
    assert!(observed.delay_snapshot().is_none());
    observed.process(&mut [], 2);
    assert!(observed.delay_snapshot().is_none());
    assert!(DelayEffect::new(0.0).delay_snapshot().is_none());

    for effect in [&mut observed, &mut untouched] {
        effect.process(&mut [0.25, -0.125], 2);
    }
    let initial = observed
        .delay_snapshot()
        .expect("one complete frame is observable");
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
        let weight = if frame == 2 {
            1.0 - progress
        } else if frame == 4 {
            progress
        } else {
            0.0
        };
        for (value, impulse) in actual.into_iter().zip([0.25, -0.125]) {
            assert!((f64::from(value) - impulse * weight).abs() <= 2e-7);
        }
        let state = observed
            .delay_snapshot()
            .expect("processed timing stays observable");
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
    assert!(restarted
        .lines
        .iter()
        .all(|line| !line.moving && line.old_samples == 5.0 && line.destination_samples == 5.0));
}

#[test]
fn delay_snapshot_reports_applied_controls_and_reference_motion_independently() {
    let mut effect = DelayEffect::new(1000.0);
    for (index, value) in [
        (param::TIME_MS, 3.0),
        (param::FEEDBACK, 0.0),
        (param::MIX, 0.0),
    ] {
        assert!(effect.set_param(index, value));
    }
    effect.process(&mut [0.25; 200], 2);
    for (index, value) in [
        (param::MIX, 1.0),
        (param::DUCK, 1.0),
        (param::FEEDBACK_HP_HZ, 200.0),
    ] {
        assert!(effect.set_param(index, value));
    }
    effect.process(&mut [0.25, 0.125], 2);
    let first = effect.delay_snapshot().unwrap();
    assert_copy_bounded(&first);
    assert!((first.mix - 0.1).abs() <= 1e-6);
    assert_eq!(first.feedback, 0.0);
    assert_eq!(first.routing, 0.0);
    assert!((first.duck_attenuation_db - 2.4).abs() <= 1e-5);
    assert_eq!(first.filters[0].enabled, 0.0);
    assert_eq!(first.filters[0].cutoff_hz, 128.0);
    assert!(first.filters[0].moving);
    assert_eq!(first.filters[1].cutoff_hz, 450.0);
    // Bypassed cutoff, Mix and Duck motion do not change the disclosed wet reference.
    assert!(!first.reference_moving);
    for _ in 0..4 {
        let repeated = effect.delay_snapshot().unwrap();
        assert_eq!(repeated.processed_frames, first.processed_frames);
        assert_eq!(repeated.mix, first.mix);
        assert_eq!(repeated.duck_attenuation_db, first.duck_attenuation_db);
        assert_eq!(repeated.filters[0].cutoff_hz, first.filters[0].cutoff_hz);
    }
    effect.process(&mut [0.25; 18], 2);
    let settled = effect.delay_snapshot().unwrap();
    assert_eq!(settled.mix, 1.0);
    assert_eq!(settled.filters[0].cutoff_hz, 200.0);
    assert!(!settled.filters[0].moving);
    assert!(!settled.reference_moving);
    for (index, value) in [
        (param::FEEDBACK, 0.5),
        (param::ROUTING, 1.0),
        (param::FEEDBACK_HP_ENABLED, 1.0),
        (param::FEEDBACK_LP_ENABLED, 1.0),
    ] {
        assert!(effect.set_param(index, value));
        effect.process(&mut [0.25, 0.125], 2);
        assert!(effect.delay_snapshot().unwrap().reference_moving);
        assert!(effect.set_automated_param(index, 0.0));
        assert!(!effect.delay_snapshot().unwrap().reference_moving);
    }
    assert!(effect.set_automated_param(param::FEEDBACK_HP_ENABLED, 1.0));
    assert!(effect.set_param(param::FEEDBACK_HP_HZ, 400.0));
    effect.process(&mut [0.25, 0.125], 2);
    assert!(effect.delay_snapshot().unwrap().reference_moving);
    assert!(effect.set_automated_param(param::FEEDBACK_HP_HZ, 200.0));
    assert!(effect.set_automated_param(param::FEEDBACK_HP_ENABLED, 0.0));
    assert!(!effect.delay_snapshot().unwrap().reference_moving);

    for (index, value) in [
        (param::TIME_MS, 5.0),
        (param::FEEDBACK, 0.5),
        (param::ROUTING, 1.0),
        (param::FEEDBACK_HP_ENABLED, 1.0),
        (param::FEEDBACK_HP_HZ, 400.0),
        (param::FEEDBACK_LP_ENABLED, 1.0),
        (param::FEEDBACK_LP_HZ, 20000.0),
    ] {
        assert!(effect.set_param(index, value));
    }
    effect.process(&mut [0.25, 0.125], 2);
    let moving = effect.delay_snapshot().unwrap();
    assert_copy_bounded(&moving);
    for (actual, expected) in [
        (moving.feedback, 0.05),
        (moving.mix, 1.0),
        (moving.routing, 0.05),
        (moving.filters[0].enabled, 0.1),
        (moving.filters[1].enabled, 0.1),
    ] {
        assert!((actual - expected).abs() <= 1e-6);
    }
    assert_eq!(moving.filters[0].cutoff_hz, 220.0);
    assert_eq!(moving.filters[1].cutoff_hz, 450.0);
    assert_eq!(moving.targets[param::FEEDBACK_LP_HZ], 20000.0);
    assert!(moving.filters.iter().all(|filter| filter.moving));
    assert!(moving.reference_moving);
    assert!(moving
        .lines
        .iter()
        .all(|line| line.moving && line.progress == 0.0));

    assert!(effect.set_automated_param(param::FEEDBACK, 0.25));
    assert!(effect.set_automated_param(param::MIX, 0.75));
    let automated = effect.delay_snapshot().unwrap();
    assert_eq!(automated.feedback, 0.25);
    assert_eq!(automated.mix, 0.75);
    assert_eq!(automated.routing, moving.routing);
    assert_eq!(automated.filters[0].enabled, moving.filters[0].enabled);
    assert_eq!(automated.filters[0].cutoff_hz, moving.filters[0].cutoff_hz);
    assert!(automated.reference_moving);
    for line in automated.lines {
        assert_eq!(line.old_samples, 3.0);
        assert_eq!(line.destination_samples, 5.0);
        assert_eq!(line.progress, 0.0);
        assert!(line.moving);
    }
    effect.process(&mut [0.25, 0.125], 2);
    let next = effect.delay_snapshot().unwrap();
    assert_eq!(next.feedback, 0.25);
    assert_eq!(next.mix, 0.75);
    assert!((next.routing - 0.1).abs() <= 1e-6);
    assert_eq!(next.filters[0].cutoff_hz, 240.0);
    assert!(next
        .lines
        .iter()
        .all(|line| (line.progress - 1.0 / 19.0).abs() <= 1e-12));

    effect.process(&mut [0.25; 36], 2);
    let final_state = effect.delay_snapshot().unwrap();
    assert_eq!(final_state.feedback, 0.25);
    assert_eq!(final_state.mix, 0.75);
    assert_eq!(final_state.routing, 1.0);
    assert!((final_state.duck_attenuation_db - 24.0).abs() <= 1e-5);
    for filter in final_state.filters {
        assert_eq!(filter.enabled, 1.0);
        assert!(!filter.moving);
    }
    assert_eq!(final_state.filters[0].cutoff_hz, 400.0);
    assert_eq!(final_state.filters[1].cutoff_hz, 450.0);
    assert!(!final_state.reference_moving);
    effect.reset();
    assert!(effect.delay_snapshot().is_none());
}

#[test]
fn snapshot_order_survives_history_reset_so_live_readings_can_reject_stale_packets() {
    let mut effect = DelayEffect::new(1000.0);
    effect.process(&mut [0.0; 20], 2);
    let before = effect.delay_snapshot().unwrap();
    effect.reset();
    assert!(effect.delay_snapshot().is_none());
    effect.process(&mut [0.0; 2], 2);
    let after = effect.delay_snapshot().unwrap();
    assert_eq!(before.processed_frames, 10);
    assert_eq!(after.processed_frames, 1);
    assert!(after.observation_order > before.observation_order);
    assert_eq!(
        effect.delay_snapshot().unwrap().observation_order,
        after.observation_order
    );
}
