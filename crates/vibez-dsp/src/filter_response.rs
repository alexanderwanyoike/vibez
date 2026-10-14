//! Biquad coefficients and complex response shared by Filter audio and its display.

use std::f64::consts::TAU;

#[derive(Clone, Copy, Debug)]
pub(super) struct Coefficients {
    pub b: [f64; 3],
    pub a: [f64; 2],
}

pub(super) fn coefficients(
    sample_rate: f32,
    cutoff: f32,
    resonance: f32,
    mode: f32,
) -> Coefficients {
    let mode = mode.round().clamp(0.0, 3.0) as usize;
    coefficients_for_weights(
        sample_rate,
        cutoff,
        resonance,
        std::array::from_fn(|index| if index == mode { 1.0 } else { 0.0 }),
    )
}

pub(super) fn coefficients_for_weights(
    sample_rate: f32,
    cutoff: f32,
    resonance: f32,
    weights: [f32; 4],
) -> Coefficients {
    let omega = TAU * cutoff.clamp((sample_rate * 0.001).min(1.0), sample_rate * 0.49) as f64
        / sample_rate as f64;
    let (sin, cos) = omega.sin_cos();
    let alpha = sin / (2.0 * resonance as f64);
    // These four topologies share poles, so numerator weights can morph them
    // without maintaining extra signal paths or passing through another mode.
    let numerators = [
        [(1.0 - cos) * 0.5, 1.0 - cos, (1.0 - cos) * 0.5],
        [(1.0 + cos) * 0.5, -(1.0 + cos), (1.0 + cos) * 0.5],
        [alpha, 0.0, -alpha],
        [1.0, -2.0 * cos, 1.0],
    ];
    let a0 = 1.0 + alpha;
    Coefficients {
        b: std::array::from_fn(|coefficient| {
            numerators
                .iter()
                .zip(weights)
                .map(|(b, weight)| b[coefficient] * weight as f64)
                .sum::<f64>()
                / a0
        }),
        a: [-2.0 * cos / a0, (1.0 - alpha) / a0],
    }
}

/// Static linear response, including dry/wet interference. Nonlinear drive and
/// time-varying modulation cannot be represented by one frequency response.
pub fn response_magnitude(
    sample_rate: f32,
    cutoff: f32,
    resonance: f32,
    mode: f32,
    slope: f32,
    mix: f32,
    frequency: f32,
) -> f32 {
    if ![sample_rate, cutoff, resonance, mode, slope, mix, frequency]
        .into_iter()
        .all(f32::is_finite)
        || sample_rate < 2.0
    {
        return 0.0;
    }
    let c = coefficients(sample_rate, cutoff, resonance.clamp(0.1, 10.0), mode);
    let omega = TAU * frequency.clamp(0.0, sample_rate * 0.5) as f64 / sample_rate as f64;
    let nr = c.b[0] + c.b[1] * omega.cos() + c.b[2] * (2.0 * omega).cos();
    let ni = -c.b[1] * omega.sin() - c.b[2] * (2.0 * omega).sin();
    let dr = 1.0 + c.a[0] * omega.cos() + c.a[1] * (2.0 * omega).cos();
    let di = -c.a[0] * omega.sin() - c.a[1] * (2.0 * omega).sin();
    let denominator = dr * dr + di * di;
    let re = (nr * dr + ni * di) / denominator;
    let im = (ni * dr - nr * di) / denominator;
    let slope = slope.clamp(0.0, 1.0) as f64;
    let wet_re = re + slope * (re * re - im * im - re);
    let wet_im = im + slope * (2.0 * re * im - im);
    let mix = mix.clamp(0.0, 1.0) as f64;
    (((1.0 - mix + mix * wet_re).powi(2) + (mix * wet_im).powi(2)).sqrt()) as f32
}
