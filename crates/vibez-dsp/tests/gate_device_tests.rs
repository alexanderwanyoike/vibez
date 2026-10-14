//! Gate behavior must depend on its first two channels without corrupting wider buses.

use vibez_dsp::{effect::AudioEffect, gate::GateEffect};

#[test]
fn four_channel_gate_preserves_extra_channels_and_matches_stereo_processing() {
    for rate in [44_100.0, 48_000.0, 96_000.0] {
        let mut wider = Vec::new();
        let mut stereo = Vec::new();
        for frame in 0..4096 {
            let left = if (1024..2048).contains(&frame) { 0.5 } else { 0.001 };
            let right = -left * 0.6;
            wider.extend_from_slice(&[left, right, 0.9, -0.8]);
            stereo.extend_from_slice(&[left, right]);
        }
        let mut gate = GateEffect::new(rate);
        let mut reference = GateEffect::new(rate);
        reference.process(&mut stereo, 2);
        gate.process(&mut wider, 4);
        for (frame, (actual, expected)) in wider.chunks_exact(4).zip(stereo.chunks_exact(2)).enumerate() {
            assert_eq!(actual[2], 0.9, "extra channel2 changed at{rate}Hz/frame{frame}");
            assert_eq!(actual[3], -0.8, "extra channel3 changed at{rate}Hz/frame{frame}");
            assert_eq!(&actual[..2], expected,
                "first-two detection or gain changed at{rate}Hz/frame{frame}");
        }
    }
}

#[test]
fn enabled_range_applies_a_closed_floor_while_disabled_range_preserves_legacy_zero() {
    let rate = 48_000.0;
    let make_gate = |depth: f32, enabled: bool| {
        let mut gate = GateEffect::new(rate);
        assert!(gate.set_param(0, -20.0));
        assert!(gate.set_param(1, 0.1));
        assert!(gate.set_param(2, 5.0));
        assert!(gate.set_param(3, 0.0));
        assert!(gate.set_param(6, depth), "the final Range index6 must be supported");
        assert!(gate.set_param(5, if enabled { 1.0 } else { 0.0 }),
            "the final Range Enabled index5 must be supported");
        gate
    };
    let mut disabled = make_gate(12.0, false);
    let mut dry_closed = vec![0.01; 4800];
    disabled.process(&mut dry_closed, 1);
    assert!(dry_closed.iter().all(|sample| *sample == 0.0),
        "disabled Range changed the original zero closed target");
    for depth in [0.0_f32, 12.0, 60.0] {
        let mut enabled = make_gate(depth, true);
        let mut signal = vec![0.01; 4800];
        enabled.process(&mut signal, 1);
        let expected = 0.01 * 10.0_f32.powf(-depth / 20.0);
        let tolerance = 2e-8 + expected * 2e-6;
        for sample in &signal[signal.len() - 128..] {
            assert!((*sample - expected).abs() <= tolerance,
                "Range {depth}dB settled at {sample}, expected {expected}");
        }
    }
}

#[test]
fn canonical_gate_parameters_round_trip_and_legacy_projects_get_neutral_appended_defaults() {
    let expected = [
        (-80.0, 0.0, -40.0, "dB"), (0.1, 50.0, 1.0, "ms"),
        (5.0, 500.0, 50.0, "ms"), (0.0, 200.0, 10.0, "ms"),
        (0.0, 24.0, 0.0, "dB"), (0.0, 1.0, 0.0, ""),
        (0.0, 120.0, 60.0, "dB"), (0.0, 1.0, 0.0, ""),
        (1.0, 20.0, 2.0, ":1"), (0.0, 1.0, 0.0, ""),
        (20.0, 2000.0, 80.0, "Hz"), (0.0, 1.0, 0.0, ""),
        (1000.0, 20000.0, 12000.0, "Hz"), (0.0, 1.0, 0.0, ""),
        (0.0, 1.0, 1.0, ""),
    ];
    let legacy = [-32.0, 2.5, 120.0, 60.0];
    let mut effect = vibez_dsp::factory::create_effect_with_params(
        vibez_core::effect::EffectType::Gate, 48_000.0, &legacy);
    assert_eq!(effect.param_descriptors().len(), expected.len());
    for (index, &(min, max, default, unit)) in expected.iter().enumerate() {
        let descriptor = &effect.param_descriptors()[index];
        assert_eq!((descriptor.min, descriptor.max, descriptor.default, descriptor.unit),
            (min, max, default, unit), "stored contract changed at index {index}");
        assert_eq!(effect.get_param(index), legacy.get(index).copied().unwrap_or(default));
        for value in [min, max, default] {
            assert!(effect.set_param(index, value));
            assert_eq!(effect.get_param(index), value, "parameter {index} did not round trip");
        }
    }
}

#[test]
fn hysteresis_retains_the_open_band_including_its_closing_boundary_and_resets_closed() {
    for (hysteresis, states) in [(20.0, [true, true, true, false, false, true]),
                                 (0.0, [true, false, false, false, false, true])] {
        let mut gate = GateEffect::new(2.0);
        for (index, value) in [(0, -20.0), (1, 0.1), (2, 5.0), (3, 0.0), (4, hysteresis)] {
            assert!(gate.set_param(index, value));
        }
        // This valid low rate makes both envelopes settle in one frame, isolating threshold equality.
        for (sample, open) in [0.1_f32, 0.05, 0.01, 0.005, 0.05, 0.1].into_iter().zip(states) {
            let mut frame = [sample];
            gate.process(&mut frame, 1);
            let expected = if open { sample } else { 0.0 };
            assert!((frame[0] - expected).abs() <= 1e-7,
                "H={hysteresis}dB at input {sample}: actual {}, expected {expected}", frame[0]);
        }
        gate.reset();
        let mut below_open_threshold = [0.05];
        gate.process(&mut below_open_threshold, 1);
        assert!(below_open_threshold[0].abs() < 1e-7, "reset retained an open latch");
    }
}
