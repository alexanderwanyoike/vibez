//! Requires missing mono Right input to preserve the stereo feedback history lifecycle.

use crate::{delay::DelayEffect, effect::AudioEffect};
use serde_json::json;

fn process_parts(effect: &mut DelayEffect, buffer: &mut [f32], channels: usize, parts: &[usize]) {
    let mut frame = 0;
    let mut part = 0;
    let frames = buffer.len() / channels;
    while frame < frames {
        let end = (frame + parts[part % parts.len()]).min(frames);
        effect.process(&mut buffer[channels * frame..channels * end], channels);
        frame = end;
        part += 1;
    }
}

fn expected_left(frame: usize, delay: usize, mono_input: f32) -> f32 {
    // Empty impulse superposition is positive silence; floating Sum seeds negative zero.
    (1..=frame / delay)
        .map(|repeat| {
            let input_frame = frame - repeat * delay;
            let dry = if (1..2 * delay).contains(&input_frame) {
                f64::from(mono_input)
            } else {
                0.0
            };
            dry * 0.5_f64.powi((repeat - 1) as i32)
        })
        .fold(0.0_f64, |sum, value| sum + value) as f32
}

fn expected_right(frame: usize, delay: usize) -> f32 {
    if frame >= delay && frame.is_multiple_of(delay) {
        0.5_f32.powi((frame / delay - 1) as i32)
    } else {
        0.0
    }
}

fn sample_bits_hash(samples: &[f32]) -> String {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in samples
        .iter()
        .flat_map(|sample| sample.to_bits().to_le_bytes())
    {
        hash = (hash ^ u64::from(byte)).wrapping_mul(0x100_0000_01b3);
    }
    format!("{hash:016x}")
}

#[test]
fn mono_frames_advance_both_histories_with_zero_missing_right_input() {
    let mut observations = Vec::new();
    let mut failures = Vec::new();
    for rate in [1000, 48_000, 96_000] {
        let delay = rate / 100;
        let partitions: &[&[usize]] = &[&[8 * delay], &[1], &[7, 31, 64, 128, 512], &[4096, 17]];
        for mono_input in [0.0_f32, 0.125] {
            for parts in partitions {
                let mut effect = DelayEffect::new(rate as f32);
                assert!(effect.processing_configuration_valid());
                assert!(effect.set_param(0, 10.0));
                assert!(effect.set_param(1, 0.5));
                assert!(effect.set_param(2, 1.0));
                let stored_distance =
                    f64::from(effect.get_param(0)) * f64::from(rate as f32) / 1000.0;
                assert_eq!(stored_distance, delay as f64);
                let mut prime = [0.0, 1.0];
                process_parts(&mut effect, &mut prime, 2, parts);
                let mut mono = vec![mono_input; 2 * delay - 1];
                process_parts(&mut effect, &mut mono, 1, parts);
                let mut stereo = vec![0.0; 2 * (2 * delay + 2)];
                process_parts(&mut effect, &mut stereo, 2, parts);

                // Dyadic inputs/feedback and integral distances make every expected sample exact.
                let mut mismatches = prime.iter().filter(|sample| sample.to_bits() != 0).count();
                let mut max_left_error = 0.0_f64;
                let mut max_right_error = 0.0_f64;
                for (index, actual) in mono.iter().enumerate() {
                    let expected = expected_left(index + 1, delay, mono_input);
                    mismatches += usize::from(actual.to_bits() != expected.to_bits());
                    max_left_error =
                        max_left_error.max((f64::from(*actual) - f64::from(expected)).abs());
                }
                for (index, pair) in stereo.as_chunks::<2>().0.iter().enumerate() {
                    let frame = 2 * delay + index;
                    let left = expected_left(frame, delay, mono_input);
                    let right = expected_right(frame, delay);
                    mismatches += usize::from(pair[0].to_bits() != left.to_bits());
                    mismatches += usize::from(pair[1].to_bits() != right.to_bits());
                    max_left_error =
                        max_left_error.max((f64::from(pair[0]) - f64::from(left)).abs());
                    max_right_error =
                        max_right_error.max((f64::from(pair[1]) - f64::from(right)).abs());
                }
                let witnesses: Vec<_> = [2, 3, 4]
                    .into_iter()
                    .map(|repeat| {
                        let frame = repeat * delay;
                        let actual = stereo[2 * (frame - 2 * delay) + 1];
                        json!({
                            "frame": frame, "repeat": repeat,
                            "expected_right_bits": expected_right(frame, delay).to_bits(),
                            "actual_right_bits": actual.to_bits(),
                        })
                    })
                    .collect();
                let right_nonzero_count = stereo
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .filter(|pair| pair[1] != 0.0)
                    .count();
                if mismatches != 0 {
                    failures.push(format!(
                        "rate={rate}, mono_input={mono_input}, parts={parts:?}: {mismatches} exact sample mismatches, max L/R error={max_left_error}/{max_right_error}"
                    ));
                }
                observations.push(json!({
                    "rate": rate, "stored_time_bits": effect.get_param(0).to_bits(),
                    "delay_frames": delay, "feedback": 0.5, "mix": 1,
                    "partitions": parts, "mono_input": mono_input,
                    "mono_first_frame": 1, "mono_last_frame": 2 * delay - 1,
                    "stereo_resume_frame": 2 * delay,
                    "comparison": "Exact f32 sample bits from independent geometric impulse superposition",
                    "mismatched_samples": mismatches, "max_left_error": max_left_error,
                    "max_right_error": max_right_error, "right_positive_witnesses": witnesses,
                    "resumed_right_nonzero_count": right_nonzero_count,
                    "mono_output_fnv1a64": sample_bits_hash(&mono),
                    "stereo_output_fnv1a64": sample_bits_hash(&stereo),
                }));
            }
        }
    }
    println!(
        "DELAY_MONO_HISTORY_RECORD {}",
        json!({
            "case": "mono_missing_right_feedback_lifecycle",
            "execution_head": std::env::var("VIBEZ_VERIFICATION_HEAD")
                .unwrap_or_else(|_| "UNLABELLED".into()),
            "observed": observations,
        })
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
