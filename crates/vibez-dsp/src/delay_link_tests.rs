//! Requires independent Right timing and retained intent through exact automated Link/Sync edits.

use crate::{delay::DelayEffect, effect::AudioEffect};
use serde_json::{json, Value};
use vibez_core::audio_context::DeviceAudioContext;

const SAMPLE_TOLERANCE: f64 = 2.0e-7;

fn prepared(rate: u32, targets: [f32; 17], bpm: u32) -> DelayEffect {
    let mut effect = DelayEffect::new(rate as f32);
    for (index, value) in targets.into_iter().enumerate() {
        assert!(effect.set_param(index, value));
    }
    effect.set_audio_context(DeviceAudioContext {
        musical_sample: 900_001,
        continuous_sample: 1_100_003,
        sample_rate: rate,
        bpm: f64::from(bpm),
        playing: true,
    });
    effect
}

fn target_mismatches(effect: &DelayEffect, expected: &[f32; 17]) -> Vec<Value> {
    expected
        .iter()
        .enumerate()
        .filter_map(|(index, value)| {
            let actual = effect.get_param(index).to_bits();
            (actual != value.to_bits()).then(|| {
                json!({
                    "index": index, "expected_bits": value.to_bits(), "actual_bits": actual,
                })
            })
        })
        .collect()
}

fn musical_distance(rate: u32, bpm: u32, division: usize, modifier: usize) -> f64 {
    let (numerator, denominator) = [(1_u64, 1_u64), (3, 2), (2, 3)][modifier];
    let numerator = u64::from(rate) * 240 * numerator;
    let denominator = u64::from(bpm) * [64, 32, 16, 8, 4, 2, 1][division] * denominator;
    (numerator / denominator) as f64 + (numerator % denominator) as f64 / denominator as f64
}

fn choose(n: usize, k: usize) -> f64 {
    let mut result = 1_u64;
    for index in 0..k.min(n - k) {
        result = result * (n - index) as u64 / (index + 1) as u64;
    }
    result as f64
}

fn kernel_weight(repeat: usize, older_terms: usize, fraction: f64) -> f64 {
    choose(repeat, older_terms)
        * (1.0 - fraction).powi((repeat - older_terms) as i32)
        * fraction.powi(older_terms as i32)
}

fn stationary_expected(
    frames: usize,
    distances: [f64; 2],
    feedback: f32,
) -> (Vec<f64>, Vec<Value>) {
    let mut expected = vec![0.0; 2 * frames];
    let mut peaks = Vec::new();
    for (channel, distance) in distances.into_iter().enumerate() {
        let whole = distance.floor() as usize;
        let fraction = distance.fract();
        let amplitude = [1.0, 0.25][channel];
        let repeats = (frames - 1) / whole;
        // The fixture bound keeps exact integer binomial coefficients small, including the faster line's tail.
        assert!(repeats <= 32);
        for repeat in 1..=repeats {
            let gain = amplitude * f64::from(feedback).powi((repeat - 1) as i32);
            let mut peak = (0, 0.0_f64);
            for older_terms in 0..=repeat {
                let frame = repeat * whole + older_terms;
                let contribution = gain * kernel_weight(repeat, older_terms, fraction);
                if frame < frames {
                    expected[2 * frame + channel] += contribution;
                    if contribution > peak.1 {
                        peak = (frame, contribution);
                    }
                }
            }
            if repeat <= 4 && gain > 0.0 {
                assert!(peak.1 > SAMPLE_TOLERANCE);
                peaks.push((channel, repeat, peak));
            }
        }
    }
    let witnesses = peaks.into_iter().map(|(channel, repeat, (frame, contribution))| json!({
        "channel": channel, "repeat": repeat, "frame": frame,
        "repeat_contribution": contribution, "expected_total": expected[2 * frame + channel],
    })).collect();
    (expected, witnesses)
}

fn observe(actual: &[f32], expected: &[f64]) -> (Value, bool) {
    assert_eq!(actual.len(), expected.len());
    let mut maximum_error = [0.0_f64; 2];
    let mut mismatches = [0; 2];
    let mut nonfinite = [0; 2];
    let mut expected_witnesses = [0; 2];
    let mut missing_witnesses = [0; 2];
    for (index, (&actual, &expected)) in actual.iter().zip(expected).enumerate() {
        let channel = index % 2;
        let error = if actual.is_finite() {
            (f64::from(actual) - expected).abs()
        } else {
            f64::INFINITY
        };
        maximum_error[channel] = maximum_error[channel].max(error);
        mismatches[channel] += usize::from(error > SAMPLE_TOLERANCE);
        nonfinite[channel] += usize::from(!actual.is_finite());
        if expected > SAMPLE_TOLERANCE {
            expected_witnesses[channel] += 1;
            missing_witnesses[channel] += usize::from(f64::from(actual) <= SAMPLE_TOLERANCE);
        }
    }
    (
        json!({
            "maximum_error": maximum_error, "mismatched_samples": mismatches,
            "nonfinite_samples": nonfinite, "expected_significant_witnesses": expected_witnesses,
            "missing_witnesses": missing_witnesses,
        }),
        mismatches == [0, 0] && nonfinite == [0, 0] && missing_witnesses == [0, 0],
    )
}

fn process_range(
    effect: &mut DelayEffect,
    audio: &mut [f32],
    start: usize,
    end: usize,
    mixed: bool,
) {
    let parts: &[usize] = if mixed {
        &[1, 7, 31, 64, 128, 512]
    } else {
        &[end - start]
    };
    let mut frame = start;
    let mut part = 0;
    while frame < end {
        let next = (frame + parts[part % parts.len()]).min(end);
        effect.process(&mut audio[2 * frame..2 * next], 2);
        frame = next;
        part += 1;
    }
}

fn finish(case: &str, rows: Vec<Value>, failures: Vec<String>) {
    println!(
        "DELAY_LINK_RECORD {}",
        json!({
            "case": case, "sample_tolerance": SAMPLE_TOLERANCE,
            "execution_head": std::env::var("VIBEZ_VERIFICATION_HEAD").unwrap_or_else(|_| "UNLABELLED".into()),
            "observed": rows,
        })
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn unlinked_free_and_sync_lines_have_independent_fractional_geometric_echoes() {
    let mut fixtures = Vec::new();
    for (rate, left, right) in [
        (1000, 10.0_f32, 17.0_f32),
        (1000, 16.25, 23.5),
        (48_000, 10.0, 17.0),
        (48_000, 1.046875, 1.03125),
        (1000, 1000.0, 2000.0),
    ] {
        fixtures.push((
            rate,
            false,
            120,
            left,
            right,
            4,
            0,
            4,
            0,
            [
                f64::from(left) * f64::from(rate) / 1000.0,
                f64::from(right) * f64::from(rate) / 1000.0,
            ],
        ));
    }
    for rate in [1000, 48_000] {
        for (bpm, left_division, left_modifier, right_division, right_modifier) in
            [(120, 3, 0, 3, 1), (20, 3, 0, 3, 1), (999, 0, 2, 1, 0)]
        {
            fixtures.push((
                rate,
                true,
                bpm,
                17.25,
                23.5,
                left_division,
                left_modifier,
                right_division,
                right_modifier,
                [
                    musical_distance(rate, bpm, left_division, left_modifier),
                    musical_distance(rate, bpm, right_division, right_modifier),
                ],
            ));
        }
    }
    fixtures.push((1000, true, 20, 17.25, 23.5, 4, 0, 6, 1, [3000.0, 18_000.0]));
    let mut rows = Vec::new();
    let mut failures = Vec::new();
    for (
        rate,
        sync,
        bpm,
        left,
        right,
        left_division,
        left_modifier,
        right_division,
        right_modifier,
        distances,
    ) in fixtures
    {
        for feedback in [0.0_f32, 0.5] {
            let targets = [
                left,
                feedback,
                1.0,
                u8::from(sync) as f32,
                left_division as f32,
                left_modifier as f32,
                0.0,
                0.0,
                right,
                right_division as f32,
                right_modifier as f32,
                0.0,
                120.0,
                0.0,
                8000.0,
                0.0,
                0.0,
            ];
            let frames = 4 * distances.into_iter().fold(0.0_f64, f64::max).ceil() as usize + 5;
            let (expected, witness_template) = stationary_expected(frames, distances, feedback);
            for mixed in [false, true] {
                let mut effect = prepared(rate, targets, bpm);
                let mut audio = vec![0.0; 2 * frames];
                audio[..2].copy_from_slice(&[1.0, 0.25]);
                process_range(&mut effect, &mut audio, 0, frames, mixed);
                let (errors, audio_matches) = observe(&audio, &expected);
                let target_mismatches = target_mismatches(&effect, &targets);
                let mut witnesses = witness_template.clone();
                for witness in &mut witnesses {
                    let index = 2 * witness["frame"].as_u64().unwrap() as usize
                        + witness["channel"].as_u64().unwrap() as usize;
                    witness["actual_bits"] = json!(audio[index].to_bits());
                }
                let matches = audio_matches && target_mismatches.is_empty();
                rows.push(json!({
                    "rate": rate, "sync": sync, "bpm": bpm, "feedback": feedback,
                    "stored_target_bits": targets.map(f32::to_bits), "expected_distances": distances,
                    "mixed_partitions": mixed, "frames": frames, "errors": errors,
                    "repeat_witnesses": witnesses, "target_mismatches": target_mismatches, "matches": matches,
                    "oracle": "Independent binomial convolution powers of two-tap interpolation, all repeats within the rendered interval",
                }));
                if !matches {
                    failures.push(format!("rate={rate}, sync={sync}, bpm={bpm}, distances={distances:?}, feedback={feedback}, mixed={mixed}"));
                }
            }
        }
    }
    finish("independent_unlinked_geometric_timing", rows, failures);
}

#[test]
fn exact_link_and_sync_automation_recalls_retained_right_intent_without_erasing_history() {
    let mut rows = Vec::new();
    let mut failures = Vec::new();
    for rate in [1000, 48_000] {
        for initial_sync in [false, true] {
            let epoch_ms = if initial_sync {
                [
                    [31.25, 31.25],
                    [31.25, 31.25],
                    [31.25, 46.875],
                    [31.25, 31.25],
                    [31.25, 31.25],
                    [31.25, 62.5],
                    [10.25, 23.75],
                    [31.25, 62.5],
                ]
            } else {
                [
                    [10.25, 10.25],
                    [10.25, 10.25],
                    [10.25, 23.75],
                    [10.25, 10.25],
                    [10.25, 10.25],
                    [10.25, 17.5],
                    [31.25, 46.875],
                    [10.25, 17.5],
                ]
            };
            let epoch_frames = 3 * (62.5 * f64::from(rate) / 1000.0).ceil() as usize + 16;
            let frames = 8 * epoch_frames;
            let mut impulses = Vec::new();
            for epoch in 0..8 {
                impulses.push((epoch * epoch_frames + 8, [0.5, 0.25]));
                if epoch > 0 {
                    impulses.push((epoch * epoch_frames - 2, [1.0, 0.5]));
                }
            }
            let mut expected = vec![0.0; 2 * frames];
            for frame in 0..frames {
                for channel in 0..2 {
                    let distance =
                        epoch_ms[frame / epoch_frames][channel] * f64::from(rate) / 1000.0;
                    // With zero feedback, each past dry impulse contributes its triangular interpolation kernel at the current tap.
                    expected[2 * frame + channel] = impulses
                        .iter()
                        .filter(|(at, _)| *at <= frame)
                        .fold(0.0, |sum, (at, amplitudes)| {
                            sum + amplitudes[channel]
                                * (1.0 - ((frame - at) as f64 - distance).abs()).max(0.0)
                        });
                }
            }
            for mixed in [false, true] {
                let mut targets = [
                    10.25,
                    0.0,
                    1.0,
                    u8::from(initial_sync) as f32,
                    0.0,
                    0.0,
                    0.0,
                    1.0,
                    17.5,
                    1.0,
                    0.0,
                    0.0,
                    120.0,
                    0.0,
                    8000.0,
                    0.0,
                    0.0,
                ];
                let mut effect = prepared(rate, targets, 120);
                let mut audio = vec![0.0; 2 * frames];
                for (at, amplitudes) in &impulses {
                    audio[2 * at..2 * at + 2]
                        .copy_from_slice(&amplitudes.map(|value| value as f32));
                }
                let mut epochs = Vec::new();
                let mut all_targets_match = true;
                for (epoch, expected_ms) in epoch_ms.iter().enumerate() {
                    let edits: Vec<(usize, f32)> = match epoch {
                        0 => vec![],
                        1 if initial_sync => vec![(9, 0.0), (10, 1.0)],
                        1 => vec![(8, 23.75)],
                        2 | 5 => vec![(7, 0.0)],
                        3 => vec![(7, 1.0)],
                        4 if initial_sync => vec![(9, 1.0), (10, 0.0), (8, 23.75)],
                        4 => vec![(8, 17.5), (9, 0.0), (10, 1.0)],
                        6 => vec![(3, u8::from(!initial_sync) as f32)],
                        7 => vec![(3, u8::from(initial_sync) as f32)],
                        _ => unreachable!(),
                    };
                    for &(index, value) in &edits {
                        assert!(effect.set_automated_param(index, value));
                        targets[index] = value;
                    }
                    let start = epoch * epoch_frames;
                    let end = start + epoch_frames;
                    process_range(&mut effect, &mut audio, start, end, mixed);
                    let target_mismatches = target_mismatches(&effect, &targets);
                    all_targets_match &= target_mismatches.is_empty();
                    let mut witnesses = Vec::new();
                    for channel in 0..2 {
                        let peak_frame = (start..end)
                            .max_by(|&a, &b| {
                                expected[2 * a + channel].total_cmp(&expected[2 * b + channel])
                            })
                            .unwrap();
                        assert!(expected[2 * peak_frame + channel] > SAMPLE_TOLERANCE);
                        witnesses.push(json!({ "channel": channel, "frame": peak_frame,
                            "expected": expected[2 * peak_frame + channel], "actual_bits": audio[2 * peak_frame + channel].to_bits() }));
                    }
                    epochs.push(json!({ "epoch": epoch, "start_frame": start, "edits": edits,
                        "expected_ms": expected_ms, "expected_target_bits": targets.map(f32::to_bits),
                        "target_mismatches": target_mismatches, "witnesses": witnesses }));
                }
                let (errors, audio_matches) = observe(&audio, &expected);
                let matches = audio_matches && all_targets_match;
                rows.push(json!({ "rate": rate, "initial_sync": initial_sync, "mixed_partitions": mixed,
                    "frames_per_epoch": epoch_frames, "frames": frames, "epochs": epochs,
                    "errors": errors, "matches": matches,
                    "oracle": "Explicit piecewise Free/musical distances; independent triangular kernels of past dry impulses; zero feedback; automation before the next frame",
                }));
                if !matches {
                    failures.push(format!(
                        "rate={rate}, initial_sync={initial_sync}, mixed={mixed}"
                    ));
                }
            }
        }
    }
    finish(
        "exact_automation_retains_inactive_right_and_both_time_representations",
        rows,
        failures,
    );
}
