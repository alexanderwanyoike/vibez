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

#[test]
fn gate_diagnostics_report_actual_decisions_hold_order_and_fresh_windows() {
    use vibez_core::{effect::GatePhase, routing::{ExternalInputBlock, ExternalInputId}};
    let mut effect = GateEffect::new(1000.0);
    for (index, value) in [(0, -20.0), (1, 0.1), (2, 5.0), (3, 3.0)] {
        assert!(effect.set_param(index, value));
    }
    let initial = effect.take_dynamics_meter().expect("Gate has real dynamics diagnostics").gate.unwrap();
    assert_eq!(initial.phase, GatePhase::Closed);
    assert_eq!(initial.envelope_db, -120.0);
    assert_eq!(initial.phases_seen, 0);
    let mut frame = [0.1_f32];
    effect.process_with_inputs(&mut frame, 1, &[ExternalInputBlock {
        id: ExternalInputId(0), channels: 1, samples: &[0.5], connected: true,
    }]);
    let snapshot = effect.take_dynamics_meter().unwrap();
    let opened = snapshot.gate.unwrap();
    assert_eq!(opened.phase, GatePhase::Open);
    assert_eq!(opened.phases_seen, 4);
    assert_eq!(opened.hold_remaining_ms, 3.0);
    assert_eq!(opened.open_threshold_db, -20.0);
    assert_eq!(opened.close_threshold_db, -20.0);
    assert_eq!(opened.peak_envelope_db, opened.envelope_db);
    assert_eq!(snapshot.detector_peak, [0.5, 0.5]);
    let mut previous = opened.envelope_db;
    for hold_ms in [3.0, 2.0, 1.0, 0.0] {
        let mut frame = [0.1_f32];
        effect.process_with_inputs(&mut frame, 1, &[ExternalInputBlock {
            id: ExternalInputId(0), channels: 1, samples: &[0.0], connected: true,
        }]);
        let snapshot = effect.take_dynamics_meter().unwrap();
        let gate = snapshot.gate.unwrap();
        let expected_phase = if hold_ms > 0.0 { GatePhase::Hold } else { GatePhase::Closed };
        assert_eq!(gate.phase, expected_phase);
        assert_eq!(gate.hold_remaining_ms, hold_ms);
        assert_eq!(gate.phases_seen, if hold_ms > 0.0 { 2 } else { 1 });
        assert!(gate.envelope_db < previous);
        assert_eq!(gate.peak_envelope_db, gate.envelope_db,
            "the previous window's high envelope leaked into this falling window");
        let measured_reduction = -20.0 * (frame[0] / 0.1).max(1e-10).log10();
        assert!((snapshot.current_reduction_db[0] - measured_reduction).abs() < 2e-5);
        previous = gate.envelope_db;
    }
    let drained = effect.take_dynamics_meter().unwrap();
    assert_eq!(drained.peak_reduction_db, [0.0; 2]);
    assert_eq!(drained.detector_peak, [0.0; 2]);
    assert_eq!(drained.gate.unwrap().phases_seen, 0);
    assert_eq!(drained.gate.unwrap().peak_envelope_db, previous);
    effect.reset();
    let reset = effect.take_dynamics_meter().unwrap().gate.unwrap();
    assert_eq!(reset.phase, GatePhase::Closed);
    assert_eq!(reset.envelope_db, -120.0);
    assert_eq!(reset.hold_remaining_ms, 0.0);
    assert_eq!(reset.phases_seen, 0);
    let mut silent_main = [0.0; 6];
    effect.process_with_inputs(&mut silent_main, 1, &[ExternalInputBlock {
        id: ExternalInputId(0), channels: 1, samples: &[0.0, 0.5, 0.0, 0.0, 0.0, 0.0], connected: true,
    }]);
    assert_eq!(silent_main, [0.0; 6]);
    let burst = effect.take_dynamics_meter().unwrap().gate.unwrap();
    assert_eq!(burst.phase, GatePhase::Closed);
    assert_eq!(burst.phases_seen, 1 | 2 | 4,
        "a short actual opening must survive the telemetry window despite silent main audio");
    assert!(burst.peak_envelope_db > burst.envelope_db);
}

#[test]
fn external_detector_layouts_distinguish_connected_silence_from_safe_fallback() {
    use vibez_core::routing::{ExternalInputBlock, ExternalInputId};
    for (id, channels, samples, connected, expected) in [
        (0, 0, &[][..], true, [0.25, 0.25]),
        (7, 1, &[0.0, 0.0][..], true, [0.25, 0.25]),
        (0, 1, &[0.0, 0.0][..], false, [0.25, 0.25]),
        (0, 1, &[0.0, 0.0][..], true, [0.0, 0.0]),
        (0, 2, &[][..], true, [0.0, 0.0]),
        (0, 1, &[0.5][..], true, [0.25, 0.0]),
        (0, 2, &[0.5][..], true, [0.25, 0.0]),
        (0, 4, &[0.0, 0.0, 0.9, 0.9, 0.0, 0.0, 0.9, 0.9][..], true, [0.0, 0.0]),
        (0, usize::MAX, &[0.5, 0.5][..], true, [0.25, 0.0]),
    ] {
        let mut effect = GateEffect::new(2.0);
        for (index, value) in [(0, -20.0), (1, 0.1), (2, 5.0), (3, 0.0)] {
            assert!(effect.set_param(index, value));
        }
        let mut main = [0.25, 0.25];
        effect.process_with_inputs(&mut main, 1, &[ExternalInputBlock {
            id: ExternalInputId(id), channels, samples, connected,
        }]);
        for (actual, expected) in main.into_iter().zip(expected) {
            assert!((actual - expected).abs() < 1e-7,
                "external id{id}/channels{channels}/connected{connected}: {actual} vs {expected}");
        }
        let mut no_channels = [0.3, -0.7];
        effect.process_with_inputs(&mut no_channels, 0, &[ExternalInputBlock {
            id: ExternalInputId(0), channels: 0, samples: &[], connected: true,
        }]);
        assert_eq!(no_channels, [0.3, -0.7]);
    }
}

#[test]
fn parallel_mix_preserves_the_dry_signal_without_falsifying_wet_attenuation() {
    use vibez_core::routing::{ExternalInputBlock, ExternalInputId};
    let frames = 4800;
    let source: Vec<f32> = (0..frames).flat_map(|_| [0.01, -0.02]).collect();
    let detector: Vec<f32> = (0..frames).map(|frame| if frame < 96 { 0.5 } else { 0.0 }).collect();
    let render = |mix| {
        let mut gate = GateEffect::new(48_000.0);
        for (index, value) in [(0, -20.0), (1, 0.1), (2, 5.0), (3, 0.0), (14, mix)] {
            assert!(gate.set_param(index, value));
        }
        let mut audio = source.clone();
        gate.process_with_inputs(&mut audio, 2, &[ExternalInputBlock {
            id: ExternalInputId(0), channels: 1, samples: &detector, connected: true,
        }]);
        (audio, gate.take_dynamics_meter().unwrap())
    };
    let (wet, reference_meter) = render(1.0);
    for mix in [0.0, 0.25, 0.75] {
        let (audio, meter) = render(mix);
        for ((sample, dry), wet) in audio.iter().zip(&source).zip(&wet) {
            let expected = dry * (1.0 - mix) + wet * mix;
            assert!((*sample - expected).abs() < 2e-8,
                "Mix{mix} output {sample} differs from actual parallel sum {expected}");
        }
        assert_eq!(meter, reference_meter, "Mix changed genuine wet dynamics diagnostics");
    }
}

#[test]
fn manual_mix_takes_exactly_512_frames_and_automation_snaps_only_the_edited_ramp() {
    let make_gate = || {
        let mut gate = GateEffect::new(48_000.0);
        gate.set_param(0, -20.0);
        gate.set_param(3, 0.0);
        gate.process(&mut [0.01; 64], 1);
        gate
    };
    let mut gate = make_gate();
    assert!(gate.set_param(14, 0.0));
    assert_eq!(gate.get_param(14), 0.0);
    let mut signal = [0.01; 513];
    gate.process(&mut signal, 1);
    for (frame, output) in signal.into_iter().enumerate() {
        let expected = 0.01 * ((frame + 1).min(512) as f32 / 512.0);
        assert!((output - expected).abs() < 1e-8,
            "manual frame{frame}: output{output}, expected512-frame mix{expected}");
    }
    let mut gate = make_gate();
    assert!(gate.set_param(0, -10.0));
    assert!(gate.set_param(14, 0.0));
    gate.process(&mut [0.01; 100], 1);
    assert!(gate.set_automated_param(14, 0.0));
    let mut next = [0.01];
    gate.process(&mut next, 1);
    assert_eq!(next[0], 0.01, "same-target automation did not snap the running Mix ramp");
    let measured = gate.take_dynamics_meter().unwrap().gate.unwrap();
    assert!((measured.open_threshold_db - (-20.0 + 101.0 * 10.0 / 512.0)).abs() < 1e-6,
        "snapping Mix cancelled the unrelated Threshold ramp: {}", measured.open_threshold_db);
    assert!(gate.set_automated_param(0, -5.0));
    gate.process(&mut [0.01], 1);
    assert_eq!(gate.take_dynamics_meter().unwrap().gate.unwrap().open_threshold_db, -5.0);
}

#[test]
fn manual_choice_changes_crossfade_warm_paths_without_switching_halfway() {
    use vibez_core::routing::{ExternalInputBlock, ExternalInputId};
    for index in [5, 7, 13] {
        let mut effect = GateEffect::new(2.0);
        for (parameter, value) in [(0, -20.0), (1, 0.1), (2, 5.0), (3, 0.0), (6, 12.0), (8, 1.0)] {
            assert!(effect.set_param(parameter, value));
        }
        let source = if index == 13 { 0.02 } else { 0.001 };
        let detector = if index == 13 { -0.04 } else { 0.001 };
        let mut warm = [source; 8];
        effect.process_with_inputs(&mut warm, 1, &[ExternalInputBlock {
            id: ExternalInputId(0), channels: 1, samples: &[detector; 8], connected: true,
        }]);
        assert!(effect.set_param(index, 1.0));
        let mut audio = [source; 513];
        effect.process_with_inputs(&mut audio, 1, &[ExternalInputBlock {
            id: ExternalInputId(0), channels: 1, samples: &[detector; 513], connected: true,
        }]);
        let destination = match index {
            5 => source * 10.0_f32.powf(-12.0 / 20.0),
            7 => source,
            13 => detector,
            _ => unreachable!(),
        };
        for (frame, output) in audio.into_iter().enumerate() {
            let expected = destination * ((frame + 1).min(512) as f32 / 512.0);
            assert!((output - expected).abs() < 1e-7,
                "choice index{index} frame{frame} changed abruptly: {output} vs {expected}");
        }
        assert!(effect.set_automated_param(index, 0.0));
        let mut frame = [source];
        effect.process_with_inputs(&mut frame, 1, &[ExternalInputBlock {
            id: ExternalInputId(0), channels: 1, samples: &[detector], connected: true,
        }]);
        assert!(frame[0].abs() < 1e-7, "choice automation was smoothed at index{index}");
    }
}

#[test]
fn invalid_rates_parameters_and_nonfinite_audio_cannot_poison_gate_state() {
    use vibez_core::routing::{ExternalInputBlock, ExternalInputId};
    for rate in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -1.0, 0.0, 1.0, 2.0, 10.0, 48_000.0] {
        let mut effect = GateEffect::new(rate);
        let expected_rate = if rate.is_finite() && rate >= 2.0 { rate as u32 } else { 48_000 };
        assert_eq!(effect.activation_sample_rate(), Some(expected_rate));
        for index in 0..15 {
            let original = effect.get_param(index);
            for invalid in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
                assert!(!effect.set_param(index, invalid));
                assert!(!effect.set_automated_param(index, invalid));
                assert_eq!(effect.get_param(index), original);
            }
        }
        assert!(!effect.set_param(usize::MAX, 1.0));
        assert_eq!(effect.get_param(usize::MAX), 0.0);
        for (index, value) in [(9, 1.0), (11, 1.0), (13, 1.0)] {
            assert!(effect.set_param(index, value));
        }
        let mut audio = [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, f32::MAX, 0.2, -0.1];
        effect.process(&mut audio, 2);
        assert!(audio.iter().all(|sample| sample.is_finite()), "nonfinite internal audio at{rate}Hz");
        let mut clean_main = [0.1; 6];
        effect.process_with_inputs(&mut clean_main, 2, &[ExternalInputBlock {
            id: ExternalInputId(0), channels: 2, samples: &[f32::NAN, f32::INFINITY, f32::NEG_INFINITY, f32::MAX], connected: true,
        }]);
        assert!(clean_main.iter().all(|sample| sample.is_finite()), "nonfinite external audio at{rate}Hz");
        let meter = effect.take_dynamics_meter().unwrap();
        assert!(meter.current_reduction_db.into_iter().chain(meter.peak_reduction_db)
            .chain(meter.detector_peak).all(f32::is_finite));
        let gate = meter.gate.unwrap();
        assert!([gate.envelope_db, gate.peak_envelope_db, gate.open_threshold_db,
            gate.close_threshold_db, gate.hold_remaining_ms].into_iter().all(f32::is_finite));
    }
}
