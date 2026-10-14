//! Independently calculated responses verify Drive's audible frequency controls.

use vibez_core::effect::EffectType;
use vibez_dsp::factory::create_effect_with_params;

fn amplitude(signal: &[f32], bin: usize) -> f64 {
    let (mut real, mut imaginary) = (0.0, 0.0);
    for (index, &sample) in signal.iter().enumerate() {
        let phase = std::f64::consts::TAU * bin as f64 * index as f64 / signal.len() as f64;
        real += sample as f64 * phase.cos();
        imaginary += sample as f64 * phase.sin();
    }
    2.0 * real.hypot(imaginary) / signal.len() as f64
}

#[test]
fn modern_tone_matches_the_one_kilohertz_complementary_tilt_at_each_host_rate() {
    let period = 2048;
    for rate in [44_100.0_f32, 48_000.0, 96_000.0] {
        for frequency in [100.0, 8_000.0] {
            let bin = (frequency * period as f64 / rate as f64).round() as usize;
            let input: Vec<f32> = (0..period * 3)
                .map(|sample| {
                    (1e-5
                        * (std::f64::consts::TAU * bin as f64 * sample as f64 / period as f64)
                            .sin()) as f32
                })
                .collect();
            let render = |tone| {
                let mut effect =
                    create_effect_with_params(EffectType::Drive, rate, &[1.0, tone, 1.0, 1.0, 1.0]);
                let mut result = input.clone();
                effect.process(&mut result, 1);
                amplitude(&result[period * 2..period * 3], bin)
            };
            let neutral = render(0.5);
            for tone in [0.0, 1.0] {
                let beta = (-std::f64::consts::TAU * 1000.0 / rate as f64).exp();
                let angle = std::f64::consts::TAU * bin as f64 / period as f64;
                let denominator = (1.0 - beta * angle.cos()).powi(2) + (beta * angle.sin()).powi(2);
                let low_real = (1.0 - beta) * (1.0 - beta * angle.cos()) / denominator;
                let low_imaginary = -(1.0 - beta) * beta * angle.sin() / denominator;
                let signed = 2.0 * tone - 1.0;
                let low_gain = 10.0_f64.powf(-6.0 * signed / 20.0);
                let high_gain = 10.0_f64.powf(6.0 * signed / 20.0);
                let expected = (high_gain + (low_gain - high_gain) * low_real)
                    .hypot((low_gain - high_gain) * low_imaginary);
                let actual = render(tone as f32) / neutral;
                assert!((actual - expected).abs() < 2e-4, "rate={rate}, tone={tone}, frequency={frequency}, actual={actual}, expected={expected}");
            }
        }
    }
}

fn butterworth_amplitude(rate: f64, cutoff: f64, bin: usize, period: usize, high: bool) -> f64 {
    let k = (std::f64::consts::PI * cutoff / rate).tan();
    let divisor = 1.0 + std::f64::consts::SQRT_2 * k + k * k;
    let b = if high {
        [1.0, -2.0, 1.0]
    } else {
        [k * k, 2.0 * k * k, k * k]
    }
    .map(|value| value / divisor);
    let a = [
        1.0,
        2.0 * (k * k - 1.0) / divisor,
        (1.0 - std::f64::consts::SQRT_2 * k + k * k) / divisor,
    ];
    let angle = std::f64::consts::TAU * bin as f64 / period as f64;
    let evaluate = |coefficients: [f64; 3]| {
        let real =
            coefficients[0] + coefficients[1] * angle.cos() + coefficients[2] * (angle * 2.0).cos();
        let imaginary = -coefficients[1] * angle.sin() - coefficients[2] * (angle * 2.0).sin();
        real.hypot(imaginary)
    };
    evaluate(b) / evaluate(a)
}

#[test]
fn pre_high_pass_filters_the_actual_shaper_input_at_its_stored_frequency() {
    let period = 2048;
    for rate in [44_100.0_f32, 48_000.0, 96_000.0] {
        let input: Vec<_> = (0..period * 3)
            .map(|sample| {
                let phase = std::f64::consts::TAU * sample as f64 / period as f64;
                (1e-6 * ((phase * 3.0).sin() + (phase * 501.0).sin())) as f32
            })
            .collect();
        let render = |enabled, cutoff| {
            let mut effect = create_effect_with_params(
                EffectType::Drive,
                rate,
                &[1.0, 0.5, 1.0, 1.0, 0.0, 2.0, enabled, cutoff],
            );
            let mut result = input.clone();
            effect.process(&mut result, 1);
            result
        };
        let dry = render(0.0, 80.0);
        for cutoff in [80.0, 700.0] {
            let filtered = render(1.0, cutoff);
            for bin in [3, 501] {
                let expected = butterworth_amplitude(rate as f64, cutoff as f64, bin, period, true);
                let actual = amplitude(&filtered[period * 2..period * 3], bin)
                    / amplitude(&dry[period * 2..period * 3], bin);
                assert!(
                    (actual - expected).abs() < 5e-5,
                    "rate={rate}, cutoff={cutoff}, bin={bin}: {actual} != {expected}"
                );
            }
        }
    }
}

#[test]
fn post_low_pass_filters_the_processed_wet_signal_and_leaves_dry_mix_aligned() {
    let period = 2048;
    for rate in [44_100.0_f32, 48_000.0, 96_000.0] {
        let input: Vec<_> = (0..period * 3)
            .map(|sample| {
                let phase = std::f64::consts::TAU * sample as f64 / period as f64;
                (1e-6 * ((phase * 3.0).sin() + (phase * 501.0).sin())) as f32
            })
            .collect();
        let render = |enabled, cutoff, mix| {
            let mut effect = create_effect_with_params(
                EffectType::Drive,
                rate,
                &[0.4, 0.5, 1.4, mix, 0.0, 2.0, 0.0, 80.0, enabled, cutoff],
            );
            let mut result = input.clone();
            effect.process(&mut result, 1);
            result
        };
        let unfiltered = render(0.0, 12000.0, 1.0);
        for cutoff in [1000.0, 12000.0] {
            let filtered = render(1.0, cutoff, 1.0);
            for bin in [3, 501] {
                let expected =
                    butterworth_amplitude(rate as f64, cutoff as f64, bin, period, false);
                let actual = amplitude(&filtered[period * 2..period * 3], bin)
                    / amplitude(&unfiltered[period * 2..period * 3], bin);
                assert!(
                    (actual - expected).abs() < 5e-5,
                    "rate={rate}, cutoff={cutoff}, bin={bin}: {actual} != {expected}"
                );
            }
            let dry = render(1.0, cutoff, 0.0);
            for sample in 64..input.len() {
                assert_eq!(dry[sample], input[sample - 64] * 1.4);
            }
        }
    }
}

#[test]
fn the_single_modern_tone_stage_retains_its_history_across_exact_character_changes() {
    for rate in [44_100.0_f32, 48_000.0, 96_000.0] {
        let mut effect =
            create_effect_with_params(EffectType::Drive, rate, &[1.0, 0.0, 1.0, 1.0, 1.0]);
        let mut warm = [0.02; 1024];
        effect.process(&mut warm, 1);
        let low_gain = 10.0_f64.powf(6.0 / 20.0);
        let high_gain = low_gain.recip();
        let mut low = warm[1023] as f64 / low_gain;
        let beta = (-std::f64::consts::TAU * 1000.0 / rate as f64).exp();
        assert!(effect.set_automated_param(4, 3.0));
        let mut output = [0.02; 192];
        effect.process(&mut output, 1);
        for (frame, actual) in output.into_iter().enumerate() {
            let shaped = if frame < 64 { (0.6_f64).tanh() } else { 0.6 };
            low = shaped * (1.0 - beta) + low * beta;
            let expected = low * low_gain + (shaped - low) * high_gain;
            assert!(
                (actual as f64 - expected).abs() < 5e-5,
                "rate={rate}, frame={frame}, actual={actual}, expected={expected}"
            );
        }
    }
}

#[test]
fn manual_filter_switches_crossfade_warm_signals_without_touching_the_dry_branch() {
    let mut high = create_effect_with_params(EffectType::Drive, 48_000.0, &[1.0, 0.5, 1.0, 1.0]);
    high.process(&mut [0.02; 4096], 1);
    assert!(high.set_param(6, 1.0));
    let mut output = [0.02; 640];
    high.process(&mut output, 1);
    for (frame, actual) in output.into_iter().enumerate() {
        let weight = (frame + 1).saturating_sub(64).min(512) as f32 / 512.0;
        let expected = (0.6 * (1.0 - weight)).tanh() / 30.0_f32.tanh();
        assert!(
            (actual - expected).abs() < 2e-6,
            "HP frame={frame}: {actual} != {expected}"
        );
    }
    let mut low = create_effect_with_params(
        EffectType::Drive,
        48_000.0,
        &[1.0, 0.5, 1.4, 0.37, 0.0, 2.0, 0.0, 80.0, 0.0, 1000.0],
    );
    let mut warm: Vec<_> = (0..4096)
        .map(|frame| if frame % 2 == 0 { 0.02 } else { -0.02 })
        .collect();
    low.process(&mut warm, 1);
    assert!(low.set_param(8, 1.0));
    let mut output: Vec<_> = (0..640)
        .map(|frame| if frame % 2 == 0 { 0.02 } else { -0.02 })
        .collect();
    low.process(&mut output, 1);
    for (frame, actual) in output.into_iter().enumerate() {
        let raw: f32 = if frame % 2 == 0 { 0.02 } else { -0.02 };
        let weight = (frame + 1).saturating_sub(64).min(512) as f32 / 512.0;
        let expected = 1.4 * (0.63 * raw + 0.37 * (1.0 - weight) * (raw * 30.0).tanh());
        assert!(
            (actual - expected).abs() < 2e-6,
            "LP frame={frame}: {actual} != {expected}"
        );
    }
}
