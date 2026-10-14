//! Audible EQ behavior and compatibility contracts for the native device.

use super::*;

fn settings() -> Vec<f32> {
    EqEffect::new(48_000.0)
        .param_descriptors()
        .iter()
        .map(|p| p.default)
        .collect()
}

fn signal(frequency: f32, rate: f32, frames: usize) -> Vec<f32> {
    (0..frames)
        .map(|n| 0.2 * (std::f32::consts::TAU * frequency * n as f32 / rate).sin())
        .collect()
}

fn rms(samples: &[f32]) -> f32 {
    (samples.iter().map(|v| v * v).sum::<f32>() / samples.len() as f32).sqrt()
}

fn rendered_db(params: &[f32], frequency: f32, rate: f32) -> f32 {
    let mut effect = EqEffect::from_params(rate, params);
    let mut samples = signal(frequency, rate, rate as usize);
    let input = rms(&samples[samples.len() / 2..]);
    effect.process(&mut samples, 1);
    20.0 * (rms(&samples[samples.len() / 2..]) / input).log10()
}

#[test]
fn appended_defaults_are_neutral_and_legacy_values_keep_their_units() {
    let effect = EqEffect::new(48_000.0);
    assert_eq!(effect.param_descriptors().len(), 22);
    let legacy = [4.0, 120.0, 0.49, -3.0, 900.0, 1.2, 5.0, 3000.0, 2.0, -6.0, 9000.0, 0.51];
    let effect = EqEffect::from_params(48_000.0, &legacy);
    for (index, value) in legacy.into_iter().enumerate() {
        assert_eq!(effect.get_param(index), value);
    }
    for index in 12..16 {
        assert_eq!(effect.get_param(index), 1.0);
    }
    assert_eq!(effect.get_param(16), 0.0);
    assert_eq!(effect.get_param(17), 0.0);
    assert_eq!(effect.get_param(18), 20.0);
    assert_eq!(effect.get_param(19), 0.0);
    assert_eq!(effect.get_param(20), 20_000.0);
    assert_eq!(effect.get_param(21), 0.0);
    assert!(effect.supports_level_meter());
}

#[test]
fn each_band_can_be_disabled_without_changing_its_saved_settings() {
    for band in 0..4 {
        let mut p = settings();
        p[band * 3] = 12.0;
        let frequency = p[band * 3 + 1];
        assert!(rendered_db(&p, frequency, 48_000.0) > 5.0);
        assert!(EqEffect::from_params(48_000.0, &p).get_param(12 + band) > 0.5);
        let mut effect = EqEffect::from_params(48_000.0, &p);
        assert!(effect.set_param(12 + band, 0.0));
        assert_eq!(effect.get_param(band * 3), 12.0);
        assert!(effect.response_db(frequency).abs() < 0.01);
    }
}

#[test]
fn cuts_and_output_trim_match_measured_audio() {
    let mut p = settings();
    assert_eq!(p.len(), 22);
    p[17] = 1.0;
    p[18] = 150.0;
    p[19] = 1.0;
    p[20] = 5_000.0;
    p[21] = -4.0;
    let effect = EqEffect::from_params(48_000.0, &p);
    for frequency in [40.0, 150.0, 800.0, 5000.0, 15000.0] {
        let measured = rendered_db(&p, frequency, 48_000.0);
        assert!((effect.response_db(frequency) - measured).abs() < 0.03,
            "{frequency}: displayed {} measured {measured}", effect.response_db(frequency));
    }
    assert!(effect.response_db(40.0) < -25.0);
    assert!(effect.response_db(15000.0) < -25.0);
    assert!((effect.response_db(800.0) + 4.0).abs() < 0.1);
}

#[test]
fn audition_is_a_band_limited_signal_even_when_gain_is_flat_or_disabled() {
    let mut p = settings();
    assert_eq!(p.len(), 22);
    for band in 0..4 {
        p[16] = (band + 1) as f32;
        p[12 + band] = 0.0;
        let effect = EqEffect::from_params(48_000.0, &p);
        let center = p[3 * band + 1];
        let pass = if band == 0 { 20.0 } else if band == 3 { 18_000.0 } else { center };
        let stop = if band == 0 { 8000.0 } else if band == 3 { 100.0 } else { 30.0 };
        assert!(effect.response_db(pass) > -1.0);
        assert!(effect.response_db(stop) < -18.0);
        for frequency in [pass, stop] {
            assert!((effect.response_db(frequency) - rendered_db(&p, frequency, 48_000.0)).abs() < 0.08);
        }
        p[12 + band] = 1.0;
    }
    for (band, shape) in [(0, 2), (3, 11)] {
        p[shape] = 1.0;
        p[16] = (band + 1) as f32;
        let effect = EqEffect::from_params(48_000.0, &p);
        assert!(effect.response_db(p[3 * band + 1]).abs() < 0.01);
        let extreme = if band == 0 { 1.0 } else { 23_900.0 };
        assert!(effect.response_db(extreme) < -25.0);
    }
}

#[test]
fn display_matches_rendered_bells_and_shelves_at_multiple_sample_rates() {
    for rate in [22_050.0, 44_100.0, 48_000.0, 96_000.0] {
        for band in 0..4 {
            let mut p = settings();
            p[band * 3] = if band % 2 == 0 { 9.0 } else { -9.0 };
            for shape in [0.0, 1.0] {
                if band == 0 || band == 3 { p[band * 3 + 2] = shape; }
                let effect = EqEffect::from_params(rate, &p);
                for frequency in [60.0, 700.0, 3000.0, rate * 0.4] {
                    assert!((effect.response_db(frequency) - rendered_db(&p, frequency, rate)).abs() < 0.05,
                        "rate {rate}, band {band}, shape {shape}, frequency {frequency}");
                }
            }
        }
    }
}

#[test]
fn invalid_parameters_are_rejected_without_poisoning_state() {
    let mut effect = EqEffect::new(48_000.0);
    for index in 0..effect.param_descriptors().len() {
        let before = effect.get_param(index);
        for invalid in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert!(!effect.set_param(index, invalid));
            assert!(!effect.set_automated_param(index, invalid));
            assert_eq!(effect.get_param(index), before);
        }
    }
    assert!(!effect.set_param(100, 0.0));
    let mut samples = [0.1; 256];
    effect.process(&mut samples, 2);
    assert!(samples.iter().all(|v| v.is_finite()));
}

#[test]
fn multichannel_frames_preserve_extra_channels_and_zero_channels_is_a_noop() {
    let mut effect = EqEffect::new(48_000.0);
    effect.set_param(0, 12.0);
    let mut samples = vec![0.2; 4 * 1000];
    for frame in samples.chunks_exact_mut(4) { frame[2] = 0.37; frame[3] = -0.63; }
    effect.process(&mut samples, 4);
    for frame in samples.chunks_exact(4) { assert_eq!(frame[2..], [0.37, -0.63]); }
    let original = samples.clone();
    effect.process(&mut samples, 0);
    assert_eq!(samples, original);
}

#[test]
fn output_manual_edit_ramps_and_automation_takes_effect_on_the_first_sample() {
    let mut manual = EqEffect::new(48_000.0);
    let mut automated = EqEffect::new(48_000.0);
    manual.process(&mut [0.2; 128], 1);
    automated.process(&mut [0.2; 128], 1);
    assert!(manual.set_param(21, -12.0));
    assert!(automated.set_automated_param(21, -12.0));
    let mut a = [0.2; 1024];
    let mut b = a;
    manual.process(&mut a, 1);
    automated.process(&mut b, 1);
    assert!((a[0] - 0.2).abs() < 0.002);
    assert!((b[0] - 0.2 * 10.0_f32.powf(-12.0 / 20.0)).abs() < 1e-6);
    assert!((a[1023] - b[1023]).abs() < 1e-6);
    assert!(a.windows(2).all(|pair| (pair[1] - pair[0]).abs() < 0.002));
}

#[test]
fn enabling_and_auditioning_while_playing_do_not_step() {
    for (parameter, target) in [(12, 0.0), (16, 2.0), (17, 1.0), (19, 1.0), (2, 1.0), (11, 1.0)] {
        let mut p = settings();
        assert_eq!(p.len(), 22);
        p[0] = 12.0; p[9] = 12.0; p[18] = 1000.0; p[20] = 500.0;
        let mut effect = EqEffect::from_params(48_000.0, &p);
        let mut warmup = [0.1; 48_000];
        effect.process(&mut warmup, 1);
        let previous = warmup[warmup.len() - 1];
        effect.set_param(parameter, target);
        let mut samples = [0.1; 2048];
        effect.process(&mut samples, 1);
        assert!((samples[0] - previous).abs() < 0.005, "parameter {parameter}");
        assert!(samples.windows(2).all(|pair| (pair[1] - pair[0]).abs() < 0.005), "parameter {parameter}");
    }
}

#[test]
fn impulse_and_sweep_are_finite_at_low_rates_and_extreme_settings() {
    for rate in [2.0, 8000.0, 44100.0, 192000.0, f32::NAN] {
        let mut effect = EqEffect::new(rate);
        for index in [0, 3, 6, 9] { effect.set_param(index, 15.0); }
        for index in [5, 8] { effect.set_param(index, 4.0); }
        effect.set_param(17, 1.0); effect.set_param(18, 1000.0);
        effect.set_param(19, 1.0); effect.set_param(20, 20000.0);
        let mut samples = vec![0.0; 8192]; samples[0] = 0.5;
        effect.process(&mut samples, 2);
        assert!(samples.iter().all(|v| v.is_finite()));
        let mut samples: Vec<f32> = (0..8192).map(|n| ((n * n) as f32 * 0.0001).sin() * 0.1).collect();
        effect.process(&mut samples, 2);
        assert!(samples.iter().all(|v| v.is_finite()));
    }
}

#[test]
fn processing_is_block_partition_independent_including_manual_ramps() {
    let mut whole = EqEffect::new(48_000.0);
    let mut split = EqEffect::new(48_000.0);
    whole.process(&mut [0.0; 32], 2); split.process(&mut [0.0; 32], 2);
    for (index, value) in [(0, 12.0), (1, 350.0), (5, 3.0), (16, 3.0), (21, -3.0)] {
        whole.set_param(index, value); split.set_param(index, value);
    }
    let mut a = signal(700.0, 48000.0, 8192);
    let mut b = a.clone();
    whole.process(&mut a, 2);
    let mut offset = 0;
    for frames in [1, 7, 63, 512, 3, 1024, 249] {
        let end = offset + frames * 2;
        split.process(&mut b[offset..end], 2); offset = end;
    }
    split.process(&mut b[offset..], 2);
    assert_eq!(a, b);
}
