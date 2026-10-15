//! Requires fractional Delay repeats to match independent powers of a two-tap interpolation kernel.

use serde_json::json;
use vibez_dsp::{delay::DelayEffect, effect::AudioEffect};

const SAMPLE_TOLERANCE: f64 = 2.0e-7;
const BINOMIAL: [&[f64]; 4] = [
    &[1.0, 1.0],
    &[1.0, 2.0, 1.0],
    &[1.0, 3.0, 3.0, 1.0],
    &[1.0, 4.0, 6.0, 4.0, 1.0],
];

fn render(effect: &mut DelayEffect, input: &[f32], parts: &[usize]) -> Vec<f32> {
    let mut output = input.to_vec();
    let mut frame = 0;
    let mut part = 0;
    while frame < output.len() / 2 {
        let end = (frame + parts[part % parts.len()]).min(output.len() / 2);
        effect.process(&mut output[2 * frame..2 * end], 2);
        frame = end;
        part += 1;
    }
    output
}

fn expected_response(
    frames: usize,
    impulse_frame: usize,
    channel: usize,
    distance: f64,
    feedback: f32,
) -> Vec<f64> {
    let whole = distance.floor() as usize;
    let fraction = distance.fract();
    let mut output = vec![0.0; frames * 2];
    // Successive echoes convolve the two-tap kernel, so no simulated ring is an oracle dependency.
    for (power, coefficients) in BINOMIAL.iter().enumerate() {
        let repeat = power + 1;
        for (tap, coefficient) in coefficients.iter().copied().enumerate() {
            let weight = f64::from(feedback).powi(power as i32)
                * coefficient
                * (1.0 - fraction).powi((repeat - tap) as i32)
                * fraction.powi(tap as i32);
            let frame = impulse_frame + repeat * whole + tap;
            output[frame * 2 + channel] += weight;
        }
    }
    output
}

#[test]
fn fractional_distances_preserve_the_first_four_interpolated_feedback_repeats() {
    let cases = [
        (1000, 16.25_f32, 0.25),
        (1000, 16.5, 0.5),
        (1000, 16.75, 0.75),
        (48_000, 1.046_875, 0.25),
        (48_000, 1.031_25, 0.5),
        (48_000, 1.015_625, 0.75),
    ];
    let mut observations = Vec::new();
    let mut failures = Vec::new();
    for (rate, stored_time_ms, expected_fraction) in cases {
        let distance = f64::from(stored_time_ms) * rate as f64 / 1000.0;
        assert_eq!(distance.fract(), expected_fraction);
        let whole = distance.floor() as usize;
        for feedback in [0.0, 0.5] {
            for channel in 0..2 {
                for impulse_frame in [0, rate * 2 - 3] {
                    let frames = impulse_frame + 4 * whole + 5;
                    let mut input = vec![0.0; frames * 2];
                    input[impulse_frame * 2 + channel] = 1.0;
                    let expected =
                        expected_response(frames, impulse_frame, channel, distance, feedback);
                    let partitions: &[&[usize]] = &[&[frames], &[1], &[7, 31, 64, 128, 512]];
                    for parts in partitions {
                        let mut effect = DelayEffect::new(rate as f32);
                        assert!(effect.set_param(0, stored_time_ms));
                        assert!(effect.set_param(1, feedback));
                        assert!(effect.set_param(2, 1.0));
                        assert_eq!(effect.get_param(0).to_bits(), stored_time_ms.to_bits());
                        let actual = render(&mut effect, &input, parts);
                        let finite = actual.iter().all(|sample| sample.is_finite());
                        let maximum_error = actual
                            .iter()
                            .zip(&expected)
                            .map(|(actual, expected)| {
                                if actual.is_finite() {
                                    (f64::from(*actual) - expected).abs()
                                } else {
                                    f64::INFINITY
                                }
                            })
                            .fold(0.0_f64, f64::max);
                        let positive_witnesses: Vec<_> = expected
                            .chunks_exact(2)
                            .enumerate()
                            .filter(|(_, pair)| pair[channel] > 0.0)
                            .map(|(frame, pair)| {
                                json!({
                                    "frame": frame,
                                    "expected_amplitude": pair[channel],
                                    "actual_bits": actual[frame * 2 + channel].to_bits(),
                                })
                            })
                            .collect();
                        assert_eq!(
                            positive_witnesses.len(),
                            if feedback == 0.0 { 2 } else { 14 }
                        );
                        if !finite || maximum_error > SAMPLE_TOLERANCE {
                            failures.push(format!(
                                "rate={rate}, time_ms={stored_time_ms}, feedback={feedback}, channel={channel}, impulse_frame={impulse_frame}, parts={parts:?}: maximum_error={maximum_error} exceeds {SAMPLE_TOLERANCE}"
                            ));
                        }
                        observations.push(json!({
                            "rate": rate,
                            "stored_time_ms_bits": stored_time_ms.to_bits(),
                            "distance_frames": distance,
                            "fraction": expected_fraction,
                            "feedback": feedback,
                            "mix": 1,
                            "input_channel": channel,
                            "impulse_frame": impulse_frame,
                            "original_ring_frames": rate * 2,
                            "partitions": parts,
                            "frames": frames,
                            "all_finite": finite,
                            "maximum_absolute_sample_error": maximum_error.is_finite().then_some(maximum_error),
                            "positive_witnesses": positive_witnesses,
                        }));
                    }
                }
            }
        }
    }
    println!(
        "DELAY_FRACTIONAL_RECORD {}",
        json!({
            "case": "first_four_fractional_repeats",
            "execution_head": std::env::var("VIBEZ_VERIFICATION_HEAD")
                .unwrap_or_else(|_| "UNLABELLED".into()),
            "absolute_sample_tolerance": SAMPLE_TOLERANCE,
            "oracle": "g^(k-1) times binomial powers of (1-f)z^-m + f*z^-(m+1), derived from stored f32 Time in f64",
            "observed": observations,
        })
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
