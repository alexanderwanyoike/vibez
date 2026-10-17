//! Requires linked Stereo Sync arrivals from independent rational musical timing and retained context.

use crate::{delay::DelayEffect, effect::AudioEffect};
use serde_json::{json, Value};
use vibez_core::audio_context::DeviceAudioContext;

const SAMPLE_TOLERANCE: f64 = 2.0e-7;

#[derive(Clone, Copy)]
struct Distance {
    whole: usize,
    fraction: f64,
}

fn musical_distance(rate: u32, bpm: (u64, u64), division: usize, modifier: usize) -> Distance {
    let denominator = [64, 32, 16, 8, 4, 2, 1][division];
    let (modifier_numerator, modifier_denominator) = [(1, 1), (3, 2), (2, 3)][modifier];
    let numerator = u64::from(rate) * 240 * bpm.1 * modifier_numerator;
    let denominator = bpm.0 * denominator * modifier_denominator;
    Distance {
        whole: (numerator / denominator) as usize,
        fraction: (numerator % denominator) as f64 / denominator as f64,
    }
}

fn context(rate: u32, bpm: f64) -> DeviceAudioContext {
    DeviceAudioContext {
        musical_sample: 9_000_001,
        continuous_sample: 11_000_003,
        sample_rate: rate,
        bpm,
        playing: true,
    }
}

fn prepared(rate: u32, division: usize, modifier: usize) -> DelayEffect {
    let mut effect = DelayEffect::new(rate as f32);
    for (index, value) in [
        (0, 17.25),
        (1, 0.0),
        (2, 1.0),
        (3, 1.0),
        (4, division as f32),
        (5, modifier as f32),
        (7, 1.0),
    ] {
        assert!(effect.set_param(index, value));
    }
    effect
}

#[derive(Clone, Copy)]
enum BoundaryEdit {
    Context(f64),
    InactiveFree,
    InactiveSync(f64),
}

fn observe(
    effect: &mut DelayEffect,
    rate: u32,
    distance: Distance,
    impulse: usize,
    mixed: bool,
    boundary: Option<BoundaryEdit>,
) -> (Value, bool) {
    let frames = impulse + distance.whole + 3;
    let parts: &[usize] = if mixed {
        &[1, 7, 64, 127, 512, 4096]
    } else {
        &[frames]
    };
    let mut scratch = vec![0.0; 2 * parts.iter().copied().max().unwrap()];
    let boundary_frame = distance.whole / 2;
    let mut expected_targets: [u32; 17] =
        std::array::from_fn(|index| effect.get_param(index).to_bits());
    let mut maximum_error = 0.0_f64;
    let mut mismatched_samples = 0;
    let mut nonfinite_samples = 0;
    let mut actual_nonzero_samples = 0;
    let mut expected_witnesses = 0;
    let mut missing_witnesses = 0;
    let mut unexpected_samples = 0;
    let mut witnesses = Vec::new();
    let mut frame = 0;
    let mut part = 0;
    let mut boundary_done = boundary.is_none();
    while frame < frames {
        if !boundary_done && frame == boundary_frame {
            match boundary.unwrap() {
                BoundaryEdit::Context(bpm) => effect.set_audio_context(context(rate, bpm)),
                BoundaryEdit::InactiveFree => {
                    assert!(effect.set_automated_param(0, 1337.5));
                    expected_targets[0] = 1337.5_f32.to_bits();
                }
                BoundaryEdit::InactiveSync(bpm) => {
                    effect.set_audio_context(context(rate, bpm));
                    for (index, value) in [(4, 0.0_f32), (5, 2.0)] {
                        assert!(effect.set_automated_param(index, value));
                        expected_targets[index] = value.to_bits();
                    }
                }
            }
            boundary_done = true;
        }
        let mut end = (frame + parts[part % parts.len()]).min(frames);
        if !boundary_done {
            end = end.min(boundary_frame);
        }
        let block = &mut scratch[..2 * (end - frame)];
        block.fill(0.0);
        if (frame..end).contains(&impulse) {
            block[2 * (impulse - frame)..2 * (impulse - frame) + 2].copy_from_slice(&[1.0, 0.25]);
        }
        effect.process(block, 2);
        for (index, pair) in block.as_chunks::<2>().0.iter().enumerate() {
            let global_frame = frame + index;
            let weight = if global_frame == impulse + distance.whole {
                1.0 - distance.fraction
            } else if global_frame == impulse + distance.whole + 1 {
                distance.fraction
            } else {
                0.0
            };
            for (channel, actual) in pair.iter().enumerate() {
                let expected = weight * if channel == 0 { 1.0 } else { 0.25 };
                let error = if actual.is_finite() {
                    (f64::from(*actual) - expected).abs()
                } else {
                    f64::INFINITY
                };
                maximum_error = maximum_error.max(error);
                mismatched_samples += usize::from(error > SAMPLE_TOLERANCE);
                nonfinite_samples += usize::from(!actual.is_finite());
                actual_nonzero_samples += usize::from(*actual != 0.0);
                if expected > SAMPLE_TOLERANCE {
                    expected_witnesses += 1;
                    missing_witnesses += usize::from(f64::from(*actual) <= SAMPLE_TOLERANCE);
                    witnesses.push(json!({
                        "frame": global_frame, "channel": channel,
                        "expected_amplitude": expected, "actual_bits": actual.to_bits(),
                    }));
                } else {
                    unexpected_samples += usize::from(f64::from(actual.abs()) > SAMPLE_TOLERANCE);
                }
            }
        }
        frame = end;
        part += 1;
    }
    let target_mismatches: Vec<_> = expected_targets
        .into_iter()
        .enumerate()
        .filter_map(|(index, expected)| {
            let actual = effect.get_param(index).to_bits();
            (actual != expected).then(
                || json!({ "index": index, "expected_bits": expected, "actual_bits": actual }),
            )
        })
        .collect();
    let identity = effect.activation_sample_rate();
    let valid = effect.processing_configuration_valid();
    let matches = mismatched_samples == 0
        && nonfinite_samples == 0
        && missing_witnesses == 0
        && unexpected_samples == 0
        && target_mismatches.is_empty()
        && identity == Some(rate)
        && valid;
    (
        json!({
            "rate": rate, "mixed_partitions": mixed, "processed_frames": frames,
            "maximum_block_frames": scratch.len() / 2, "impulse_frame": impulse,
            "expected_whole_distance": distance.whole, "expected_fraction": distance.fraction,
            "boundary_frame": boundary.map(|_| boundary_frame),
            "maximum_error": maximum_error, "mismatched_samples": mismatched_samples,
            "nonfinite_samples": nonfinite_samples, "actual_nonzero_samples": actual_nonzero_samples,
            "expected_significant_witnesses": expected_witnesses, "missing_witnesses": missing_witnesses,
            "unexpected_significant_samples": unexpected_samples, "witnesses": witnesses,
            "target_mismatches": target_mismatches, "activation_identity": identity,
            "configuration_valid": valid, "matches": matches,
        }),
        matches,
    )
}

fn finish(case: &str, rows: Vec<Value>, failures: Vec<String>) {
    println!(
        "DELAY_SYNC_TIMING_RECORD {}",
        json!({
            "case": case, "sample_tolerance": SAMPLE_TOLERANCE,
            "execution_head": std::env::var("VIBEZ_VERIFICATION_HEAD").unwrap_or_else(|_| "UNLABELLED".into()),
            "oracle": "Exact rational beat distance; quotient/remainder two-tap impulse weights; no production timing helpers",
            "observed": rows,
        })
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn all_linked_sync_divisions_and_modifiers_sound_at_their_fractional_musical_arrivals() {
    let mut rows = Vec::new();
    let mut failures = Vec::new();
    for rate in [44_100, 96_000] {
        for bpm in [120, 999] {
            for division in 0..7 {
                for modifier in 0..3 {
                    let distance = musical_distance(rate, (bpm, 1), division, modifier);
                    for mixed in [false, true] {
                        let mut effect = prepared(rate, division, modifier);
                        effect.set_audio_context(context(rate, bpm as f64));
                        let (mut row, matches) =
                            observe(&mut effect, rate, distance, 0, mixed, None);
                        row["bpm"] = json!(bpm);
                        row["division_ordinal"] = json!(division);
                        row["modifier_ordinal"] = json!(modifier);
                        if !matches {
                            failures.push(format!("rate={rate}, bpm={bpm}, division={division}, modifier={modifier}, mixed={mixed}"));
                        }
                        rows.push(row);
                    }
                }
            }
        }
    }
    finish(
        "all_21_sync_timings_at_project_tempos_and_rates",
        rows,
        failures,
    );
}

#[test]
fn sync_context_preserves_tempo_intent_and_the_prepared_sample_clock() {
    let cases = [
        ("default120", None, None, (120, 1)),
        ("initial_invalid_uses120", Some(f64::NAN), None, (120, 1)),
        (
            "retain_after_nan",
            Some(240.0),
            Some(BoundaryEdit::Context(f64::NAN)),
            (240, 1),
        ),
        (
            "retain_after_positive_infinity",
            Some(240.0),
            Some(BoundaryEdit::Context(f64::INFINITY)),
            (240, 1),
        ),
        (
            "retain_after_negative_infinity",
            Some(240.0),
            Some(BoundaryEdit::Context(f64::NEG_INFINITY)),
            (240, 1),
        ),
        ("finite_zero_clamps20", Some(0.0), None, (20, 1)),
        ("finite_negative_clamps20", Some(-1.0), None, (20, 1)),
        ("finite_low_clamps20", Some(19.5), None, (20, 1)),
        ("finite_high_clamps999", Some(1000.0), None, (999, 1)),
        ("finite_maximum_clamps999", Some(f64::MAX), None, (999, 1)),
        ("fractional_bpm_preserved", Some(123.25), None, (493, 4)),
        (
            "unchanged_tempo_keeps_history",
            Some(240.0),
            Some(BoundaryEdit::Context(240.0)),
            (240, 1),
        ),
        (
            "changed_tempo_applies_at_boundary",
            Some(120.0),
            Some(BoundaryEdit::Context(240.0)),
            (240, 1),
        ),
        ("mismatched_context_clock", Some(240.0), None, (240, 1)),
        (
            "inactive_free_time_retained",
            Some(240.0),
            Some(BoundaryEdit::InactiveFree),
            (240, 1),
        ),
        (
            "inactive_sync_intent_in_free",
            Some(20.0),
            Some(BoundaryEdit::InactiveSync(999.0)),
            (20, 1),
        ),
    ];
    let mut rows = Vec::new();
    let mut failures = Vec::new();
    let rate = 48_000;
    for (label, initial, boundary, expected_bpm) in cases {
        for mixed in [false, true] {
            let mut effect = prepared(rate, 4, 0);
            if let Some(bpm) = initial {
                let context_rate = if label == "mismatched_context_clock" {
                    96_000
                } else {
                    rate
                };
                effect.set_audio_context(context(context_rate, bpm));
            }
            let distance = if label == "inactive_sync_intent_in_free" {
                assert!(effect.set_param(3, 0.0));
                assert!(effect.set_param(4, 6.0));
                assert!(effect.set_param(5, 1.0));
                Distance {
                    whole: 828,
                    fraction: 0.0,
                }
            } else {
                musical_distance(rate, expected_bpm, 4, 0)
            };
            let (mut row, matches) = observe(&mut effect, rate, distance, 0, mixed, boundary);
            row["context_case"] = json!(label);
            row["initial_bpm_bits"] = json!(initial.map(f64::to_bits));
            row["expected_bpm_rational"] = json!([expected_bpm.0, expected_bpm.1]);
            row["mismatched_engine_admission_tested"] = json!(false);
            if !matches {
                failures.push(format!("context={label}, mixed={mixed}"));
            }
            rows.push(row);
        }
    }
    finish(
        "tempo_context_and_inactive_timing_ownership",
        rows,
        failures,
    );
}

#[test]
fn sync_impulses_cross_the_current_guarded_eighteen_second_ring_boundary() {
    let rate = 1000;
    let ring_frames = 18 * rate as usize + 2;
    let impulse = ring_frames - 1;
    let mut rows = Vec::new();
    let mut failures = Vec::new();
    for (division, modifier, bpm) in [(6, 1, 20), (0, 1, 120)] {
        let distance = musical_distance(rate, (bpm, 1), division, modifier);
        for mixed in [false, true] {
            let mut effect = prepared(rate, division, modifier);
            effect.set_audio_context(context(rate, bpm as f64));
            let (mut row, matches) = observe(&mut effect, rate, distance, impulse, mixed, None);
            row["current_ring_frames"] = json!(ring_frames);
            row["division_ordinal"] = json!(division);
            row["modifier_ordinal"] = json!(modifier);
            row["bpm"] = json!(bpm);
            if !matches {
                failures.push(format!(
                    "current ring wrap division={division}, modifier={modifier}, mixed={mixed}"
                ));
            }
            rows.push(row);
        }
    }
    finish(
        "actual_18_second_ring_wrap_integer_and_fractional_arrivals",
        rows,
        failures,
    );
}
