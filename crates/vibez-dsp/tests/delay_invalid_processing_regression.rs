//! Requires invalid Delay instances to preserve dry sample ownership without history access or panics.

use serde_json::json;
use std::panic::{catch_unwind, AssertUnwindSafe};
use vibez_dsp::{delay::DelayEffect, effect::AudioEffect};

fn inputs() -> Vec<(&'static str, usize, Vec<f32>)> {
    let nan = f32::from_bits(0x7fc1_2345);
    let other_nan = f32::from_bits(0xffc2_3456);
    vec![
        (
            "mono",
            1,
            vec![
                0.25,
                -0.5,
                -0.0,
                nan,
                f32::INFINITY,
                f32::NEG_INFINITY,
                1.25,
            ],
        ),
        (
            "stereo_with_trailing",
            2,
            vec![
                0.25,
                -0.5,
                -0.0,
                nan,
                f32::INFINITY,
                f32::NEG_INFINITY,
                other_nan,
            ],
        ),
        (
            "three_channels",
            3,
            vec![
                0.25,
                -0.5,
                nan,
                other_nan,
                f32::INFINITY,
                f32::NEG_INFINITY,
                -0.0,
                nan,
            ],
        ),
        (
            "four_channels",
            4,
            vec![
                nan,
                -0.0,
                f32::INFINITY,
                other_nan,
                1.25,
                -1.5,
                -0.0,
                f32::NEG_INFINITY,
                nan,
            ],
        ),
        (
            "zero_channels",
            0,
            vec![nan, f32::INFINITY, -0.0, 0.25, -0.5],
        ),
        ("incomplete_only", 4, vec![nan, f32::INFINITY, -0.0]),
        ("empty", 2, vec![]),
    ]
}

fn expected_bits(input: &[f32], channels: usize) -> Vec<u32> {
    let complete_samples = input.len().checked_div(channels).unwrap_or(0) * channels;
    input
        .iter()
        .enumerate()
        .map(|(index, sample)| {
            let owned = channels != 0 && index < complete_samples && index % channels < 2;
            if owned && !sample.is_finite() {
                0.0_f32.to_bits()
            } else {
                sample.to_bits()
            }
        })
        .collect()
}

fn panic_text(result: std::thread::Result<()>) -> Option<String> {
    result.err().map(|payload| {
        payload
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| {
                payload
                    .downcast_ref::<&str>()
                    .map(|message| (*message).to_owned())
            })
            .unwrap_or_else(|| "non-string panic".into())
    })
}

#[test]
fn invalid_instances_process_owned_dry_samples_and_reset_without_panicking() {
    let rates: [(&str, f32, Option<u32>); 11] = [
        ("zero", 0.0, Some(0)),
        ("negative", -1000.0, None),
        ("fractional_below_minimum", 0.5, None),
        ("below_minimum", 999.0, Some(999)),
        ("fractional_above_minimum", 1000.5, None),
        ("fractional_standard_rate", 48_000.5, None),
        ("not_a_number", f32::NAN, None),
        ("positive_infinity", f32::INFINITY, None),
        ("negative_infinity", f32::NEG_INFINITY, None),
        ("unrepresentable_u32", 4_294_967_296.0, None),
        ("history_budget_exceeded", 466_034.0, Some(466_034)),
    ];
    let mut observations = Vec::new();
    let mut failures = Vec::new();
    for (rate_case, rate, expected_identity) in rates {
        for (layout, channels, input) in inputs() {
            let mut effect = DelayEffect::new(rate);
            let configuration_valid = effect.processing_configuration_valid();
            let activation_identity = effect.activation_sample_rate();
            let expected = expected_bits(&input, channels);
            let mut actual = input.clone();
            let process_panic = panic_text(catch_unwind(AssertUnwindSafe(|| {
                effect.process(&mut actual, channels);
            })));
            let actual_bits: Vec<_> = actual.iter().map(|sample| sample.to_bits()).collect();
            let reset_panic = panic_text(catch_unwind(AssertUnwindSafe(|| effect.reset())));
            let post_reset_valid = effect.processing_configuration_valid();
            let post_reset_identity = effect.activation_sample_rate();
            let mismatch_count = actual_bits
                .iter()
                .zip(&expected)
                .filter(|(actual, expected)| actual != expected)
                .count();
            if configuration_valid
                || activation_identity != expected_identity
                || process_panic.is_some()
                || reset_panic.is_some()
                || post_reset_valid
                || post_reset_identity != expected_identity
                || mismatch_count != 0
            {
                failures.push(format!(
                    "rate={rate_case}, layout={layout}: valid={configuration_valid}, identity={activation_identity:?}, process_panic={process_panic:?}, reset_panic={reset_panic:?}, post_reset_valid={post_reset_valid}, post_reset_identity={post_reset_identity:?}, mismatched_samples={mismatch_count}"
                ));
            }
            observations.push(json!({
                "rate_case": rate_case,
                "requested_rate_debug": format!("{rate:?}"),
                "requested_rate_bits": rate.to_bits(),
                "expected_activation_identity": expected_identity,
                "actual_activation_identity": activation_identity,
                "configuration_valid": configuration_valid,
                "layout": layout,
                "channels": channels,
                "input_bits": input.iter().map(|sample| sample.to_bits()).collect::<Vec<_>>(),
                "expected_output_bits": expected,
                "actual_output_bits": actual_bits,
                "mismatched_samples": mismatch_count,
                "process_panic": process_panic,
                "reset_panic": reset_panic,
                "post_reset_valid": post_reset_valid,
                "post_reset_identity": post_reset_identity,
            }));
        }
    }
    println!(
        "DELAY_INVALID_PROCESSING_RECORD {}",
        json!({
            "case": "invalid_direct_processing_ownership",
            "execution_head": std::env::var("VIBEZ_VERIFICATION_HEAD")
                .unwrap_or_else(|_| "UNLABELLED".into()),
            "comparison": "Exact sample bits: finite owned dry unchanged, nonfinite owned +0, unowned unchanged",
            "observed": observations,
        })
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
