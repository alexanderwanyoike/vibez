//! Nonsubtractive TPDF moments apply to unsaturated nearest quantization at every frame.

use vibez_core::effect::EffectType;
use vibez_core::id::EffectId;
use vibez_dsp::factory::create_effect_for_instance;

const ACQUISITIONS: usize = 1 << 18;
const DELTA: f64 = 1.0 / 128.0;
const MOMENT_LIMIT: f64 = 0.005;
const CORRELATION_LIMIT: f64 = 0.015;

fn identity(raw: u64) -> EffectId {
    serde_json::from_str(&raw.to_string()).expect("persisted effect identity")
}

fn mean_and_second_moment(values: &[f64]) -> (f64, f64) {
    let count = values.len() as f64;
    let mean = values.iter().sum::<f64>() / count;
    let second = values.iter().map(|value| value * value).sum::<f64>() / count;
    (mean, second)
}

fn correlation(first: &[f64], second: &[f64]) -> f64 {
    assert_eq!(first.len(), second.len());
    let first_mean = first.iter().sum::<f64>() / first.len() as f64;
    let second_mean = second.iter().sum::<f64>() / second.len() as f64;
    let mut covariance = 0.0;
    let mut first_variance = 0.0;
    let mut second_variance = 0.0;
    for (&first, &second) in first.iter().zip(second) {
        let first = first - first_mean;
        let second = second - second_mean;
        covariance += first * second;
        first_variance += first * first;
        second_variance += second * second;
    }
    assert!(first_variance > 0.0 && second_variance > 0.0);
    covariance / (first_variance * second_variance).sqrt()
}

#[test]
fn nearest_tpdf_meets_frozen_eighteen_case_moment_and_correlation_budgets() {
    // These phases and limits were frozen against an independent finite-grid reference.
    let phases = [
        20.0, 20.125, 20.25, 20.375, 20.5, 20.625, 20.75, 20.875, -20.5,
    ];
    let mut cases = 0;
    for raw_id in [17, 9999] {
        for phase in phases {
            let input = (phase * DELTA) as f32;
            assert_eq!(f64::from(input) / DELTA, phase);
            assert!(f64::from(input).abs() + 2.0 * DELTA < 1.0);
            let mut effect = create_effect_for_instance(
                EffectType::Bitcrush,
                48_000.0,
                &[8.0, 1.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 12_000.0],
                identity(raw_id),
            );
            let mut audio = vec![input; ACQUISITIONS * 2];
            for block in audio.chunks_mut(512 * 2) {
                effect.process(block, 2);
            }
            let mut errors = [
                Vec::with_capacity(ACQUISITIONS),
                Vec::with_capacity(ACQUISITIONS),
            ];
            for frame in audio.chunks_exact(2) {
                for (channel, &output) in frame.iter().enumerate() {
                    assert!(output.is_finite() && output.abs() < 1.0);
                    let code = f64::from(output) / DELTA;
                    assert_eq!(
                        code.fract(),
                        0.0,
                        "quantizer grid: ID={raw_id}, phase={phase}"
                    );
                    errors[channel].push((f64::from(output) - f64::from(input)) / DELTA);
                }
            }
            let moments = errors
                .each_ref()
                .map(|values| mean_and_second_moment(values));
            let channel_correlation = correlation(&errors[0], &errors[1]);
            let lags = [1, 2, 7, 31]
                .map(|lag| correlation(&errors[0][..ACQUISITIONS - lag], &errors[0][lag..]));
            println!(
                "ID={raw_id}, phase_lsb={phase}, input={input}, acquisitions={ACQUISITIONS}, mean_second_lsb={moments:?}, channel_rho={channel_correlation}, left_lag_rho={lags:?}"
            );
            for (channel, &(mean, second)) in moments.iter().enumerate() {
                assert!(
                    mean.abs() <= MOMENT_LIMIT,
                    "ID={raw_id}, phase={phase}, channel={channel}, mean={mean}"
                );
                // This is total nonsubtractive error, whose second moment is Delta^2/4.
                assert!(
                    (second - 0.25).abs() <= MOMENT_LIMIT,
                    "ID={raw_id}, phase={phase}, channel={channel}, second={second}"
                );
            }
            assert!(
                channel_correlation.abs() <= CORRELATION_LIMIT,
                "ID={raw_id}, phase={phase}, channel_rho={channel_correlation}"
            );
            for (lag, rho) in [1, 2, 7, 31].into_iter().zip(lags) {
                assert!(
                    rho.abs() <= CORRELATION_LIMIT,
                    "ID={raw_id}, phase={phase}, lag={lag}, rho={rho}"
                );
            }
            cases += 1;
        }
    }
    assert_eq!(cases, 18);
}

#[test]
fn off_is_exactly_silent_and_rounding_ties_keep_their_distinct_biases() {
    let normalized_input = [-1.5, -0.5, -0.25, 0.0, 0.25, 0.5, 1.5];
    let expected_codes = [
        [-2.0, -1.0, 0.0, 0.0, 0.0, 1.0, 2.0],
        [-2.0, -1.0, -1.0, 0.0, 0.0, 0.0, 1.0],
        [-1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0],
    ];
    for (rounding, codes) in expected_codes.into_iter().enumerate() {
        let mut effect = create_effect_for_instance(
            EffectType::Bitcrush,
            48_000.0,
            &[8.0, 1.0, 1.0, 0.0, rounding as f32, 0.0, 0.0, 0.0, 12_000.0],
            identity(17),
        );
        let mut silence = vec![0.0; 4096];
        effect.process(&mut silence, 2);
        assert!(silence.iter().all(|&sample| sample == 0.0));
        let mut actual: Vec<_> = normalized_input
            .into_iter()
            .flat_map(|code| [(code * DELTA) as f32; 2])
            .collect();
        effect.process(&mut actual, 2);
        let expected: Vec<_> = codes
            .into_iter()
            .flat_map(|code| [(code * DELTA) as f32; 2])
            .collect();
        assert_eq!(actual, expected, "Dither Off rounding={rounding}");
    }
}

#[test]
fn saturated_wet_rails_are_bounded_for_all_rounding_and_dither_modes() {
    // Saturation violates the unsaturated moment theorem and has its own exact contract.
    for raw_id in [17, 9999] {
        for rounding in [0.0, 1.0, 2.0] {
            for dither in [0.0, 1.0] {
                let mut effect = create_effect_for_instance(
                    EffectType::Bitcrush,
                    48_000.0,
                    &[8.0, 1.0, 1.0, 0.0, rounding, dither, 0.0, 0.0, 12_000.0],
                    identity(raw_id),
                );
                let mut actual: Vec<_> = (0..1024).flat_map(|_| [1.25, -1.25]).collect();
                effect.process(&mut actual, 2);
                assert!(
                    actual.chunks_exact(2).all(|frame| frame == [1.0, -1.0]),
                    "ID={raw_id}, rounding={rounding}, dither={dither}"
                );
            }
        }
    }
}
