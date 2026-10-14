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
            let input: Vec<f32> = (0..period * 3).map(|sample| (1e-5 * (std::f64::consts::TAU * bin as f64 * sample as f64 / period as f64).sin()) as f32).collect();
            let render = |tone| {
                let mut effect = create_effect_with_params(EffectType::Drive, rate, &[1.0, tone, 1.0, 1.0, 1.0]);
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
                let expected = (high_gain + (low_gain - high_gain) * low_real).hypot((low_gain - high_gain) * low_imaginary);
                let actual = render(tone as f32) / neutral;
                assert!((actual - expected).abs() < 2e-4, "rate={rate}, tone={tone}, frequency={frequency}, actual={actual}, expected={expected}");
            }
        }
    }
}
