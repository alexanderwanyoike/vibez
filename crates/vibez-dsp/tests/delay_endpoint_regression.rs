//! Requires the full stored two-second delay instead of a one-sample-short arrival.

use serde_json::json;
use vibez_dsp::{delay::DelayEffect, effect::AudioEffect};

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

#[test]
fn full_two_second_delay_arrives_at_the_endpoint_and_never_one_sample_early() {
    // The endpoint's f32 seconds are exact, isolating capacity from fractional-time rounding.
    assert_eq!((2000.0_f32 * 0.001).to_bits(), 2.0_f32.to_bits());
    let mut observations = Vec::new();
    let mut failures = Vec::new();
    for rate in [1000, 48_000] {
        let delay_frames = rate * 2;
        let frames = delay_frames + 2;
        let partitions: &[&[usize]] = &[&[frames], &[1], &[7, 31, 64, 128, 512], &[4096, 17]];
        for channel in 0..2 {
            let mut input = vec![0.0; frames * 2];
            input[channel] = 1.0;
            for parts in partitions {
                let mut effect = DelayEffect::new(rate as f32);
                assert!(effect.set_param(0, 2000.0));
                assert!(effect.set_param(1, 0.0));
                assert!(effect.set_param(2, 1.0));
                let actual = render(&mut effect, &input, parts);
                let early = 2 * (delay_frames - 1);
                let endpoint = 2 * delay_frames;
                let early_bits = [actual[early].to_bits(), actual[early + 1].to_bits()];
                let endpoint_bits = [actual[endpoint].to_bits(), actual[endpoint + 1].to_bits()];
                let nonzero_frames: Vec<_> = actual
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .enumerate()
                    .filter(|(_, pair)| pair.iter().any(|sample| *sample != 0.0))
                    .map(|(frame, pair)| {
                        json!({
                            "frame": frame,
                            "left_bits": pair[0].to_bits(),
                            "right_bits": pair[1].to_bits(),
                        })
                    })
                    .collect();
                let exact_impulse = actual.iter().enumerate().all(|(index, sample)| {
                    *sample
                        == if index == endpoint + channel {
                            1.0
                        } else {
                            0.0
                        }
                });
                if !exact_impulse {
                    failures.push(format!(
                        "rate={rate}, channel={channel}, parts={parts:?}: endpoint={} (expected 1), early={} (expected 0)",
                        actual[endpoint + channel], actual[early + channel]
                    ));
                }
                observations.push(json!({
                    "rate": rate,
                    "input_channel": channel,
                    "time_ms": 2000,
                    "feedback": 0,
                    "mix": 1,
                    "partitions": parts,
                    "expected_positive_witness_frame": delay_frames,
                    "expected_positive_witness_bits": 1.0_f32.to_bits(),
                    "early_frame": delay_frames - 1,
                    "early_stereo_bits": early_bits,
                    "endpoint_stereo_bits": endpoint_bits,
                    "nonzero_frames": nonzero_frames,
                    "exact_endpoint_impulse": exact_impulse,
                }));
            }
        }
    }
    println!(
        "DELAY_ENDPOINT_RECORD {}",
        json!({
            "case": "full_two_second_endpoint",
            "execution_head": std::env::var("VIBEZ_VERIFICATION_HEAD")
                .unwrap_or_else(|_| "UNLABELLED".into()),
            "observed": observations,
        })
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
