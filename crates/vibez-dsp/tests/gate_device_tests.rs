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

#[test]
fn expander_ratio_sets_the_below_threshold_slope_with_optional_range_and_unity_at_one() {
    for (ratio, range, expected_gain) in [(1.0_f32, None, 1.0), (2.0, None, 0.1),
                                         (4.0, None, 0.001), (4.0, Some(12.0), 10.0_f32.powf(-12.0 / 20.0))] {
        let mut effect = GateEffect::new(48_000.0);
        for (index, value) in [(0, -20.0), (1, 0.1), (2, 5.0), (3, 0.0), (7, 1.0), (8, ratio)] {
            assert!(effect.set_param(index, value));
        }
        if let Some(depth) = range {
            assert!(effect.set_param(5, 1.0));
            assert!(effect.set_param(6, depth));
        }
        let mut signal = vec![0.01; 48_000];
        effect.process(&mut signal, 1);
        let samples = if ratio == 1.0 { &signal[..] } else { &signal[signal.len() - 128..] };
        for sample in samples {
            let expected = 0.01 * expected_gain;
            assert!((*sample - expected).abs() <= 2e-8 + expected * 2e-6,
                "ratio {ratio}, Range {range:?}: actual {sample}, expected {expected}");
        }
    }
}

fn butterworth_response(rate: f64, frequency: f64, cutoff: f64, high: bool) -> (f64, f64) {
    let warped = (std::f64::consts::PI * frequency / rate).tan()
        / (std::f64::consts::PI * cutoff / rate).tan();
    let magnitude = if high { warped * warped } else { 1.0 } / (1.0 + warped.powi(4)).sqrt();
    let phase = -std::f64::consts::SQRT_2.mul_add(warped, 0.0).atan2(1.0 - warped * warped)
        + if high { std::f64::consts::PI } else { 0.0 };
    (magnitude, phase)
}

#[test]
fn detector_listen_auditions_the_genuine_external_butterworth_band_at_all_rates() {
    use vibez_core::routing::{ExternalInputBlock, ExternalInputId};
    for rate in [44_100.0_f32, 48_000.0, 96_000.0] {
        for (hp, lp) in [(false, false), (true, false), (false, true), (true, true)] {
            for frequency in [40.0_f64, 1000.0, 8000.0] {
                let mut effect = GateEffect::new(rate);
                for (index, value) in [(9, hp as u8 as f32), (10, 120.0),
                    (11, lp as u8 as f32), (12, 4000.0), (13, 1.0)] {
                    assert!(effect.set_param(index, value));
                }
                let frames = rate as usize;
                let external: Vec<f32> = (0..frames).flat_map(|frame| {
                    let phase = std::f64::consts::TAU * frequency * frame as f64 / rate as f64;
                    [(0.25 * phase.sin()) as f32, (0.125 * phase.cos()) as f32]
                }).collect();
                let mut output: Vec<f32> = (0..frames).flat_map(|_| [0.02, -0.01]).collect();
                effect.process_with_inputs(&mut output, 2, &[ExternalInputBlock {
                    id: ExternalInputId(0), channels: 2, samples: &external, connected: true,
                }]);
                let (mut magnitude, mut response_phase) = (1.0, 0.0);
                for (enabled, cutoff, high) in [(hp, 120.0, true), (lp, 4000.0, false)] {
                    if enabled {
                        let response = butterworth_response(rate as f64, frequency, cutoff, high);
                        magnitude *= response.0;
                        response_phase += response.1;
                    }
                }
                for channel in 0..2 {
                    let amplitude = if channel == 0 { 0.25 } else { 0.125 };
                    let mut measured = [0.0; 2];
                    for frame in frames * 3 / 4..frames {
                        let phase = std::f64::consts::TAU * frequency * frame as f64 / rate as f64
                            + if channel == 0 { 0.0 } else { std::f64::consts::FRAC_PI_2 };
                        measured[0] += output[frame * 2 + channel] as f64 * phase.sin();
                        measured[1] += output[frame * 2 + channel] as f64 * phase.cos();
                    }
                    let scale = 2.0 / ((frames / 4) as f64 * amplitude);
                    for (actual, expected) in measured.into_iter().map(|v| v * scale)
                        .zip([magnitude * response_phase.cos(), magnitude * response_phase.sin()]) {
                        assert!((actual - expected).abs() < 3e-6,
                            "{rate}Hz/{frequency}Hz HP={hp} LP={lp} channel{channel}: actual{actual}, expected{expected}");
                    }
                }
            }
        }
    }
}
