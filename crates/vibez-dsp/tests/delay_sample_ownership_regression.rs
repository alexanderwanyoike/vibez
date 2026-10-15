//! Requires valid Delay processing to respect complete caller frames and preserve unowned sample bits.

use serde_json::json;
use vibez_dsp::{delay::DelayEffect, effect::AudioEffect};

fn unowned_sample(index: usize) -> f32 {
    f32::from_bits(
        [
            0x7fc1_2345,
            0xffc2_3456,
            0x8000_0000,
            0x7f80_0000,
            0xff80_0000,
            0x3f40_0000,
        ][index % 6],
    )
}

fn expected_owned(frame: usize, channel: usize, impulse_channel: usize, delay: usize) -> f32 {
    if channel != impulse_channel {
        0.0
    } else if frame == 0 {
        0.5
    } else if frame.is_multiple_of(delay) {
        0.5_f32.powi((frame / delay) as i32)
    } else {
        0.0
    }
}

#[derive(Default)]
struct Comparison {
    calls: usize,
    owned_samples: usize,
    unowned_samples: usize,
    owned_mismatches: usize,
    unowned_mismatches: usize,
    nonfinite_owned_outputs: usize,
    first_mismatches: Vec<serde_json::Value>,
}

impl Comparison {
    fn process(
        &mut self,
        effect: &mut DelayEffect,
        samples: &mut [f32],
        channels: usize,
        expected: &[u32],
    ) {
        effect.process(samples, channels);
        self.calls += 1;
        let complete_samples = samples.len().checked_div(channels).unwrap_or(0) * channels;
        for (index, (actual, expected)) in samples.iter().zip(expected).enumerate() {
            let owned = channels != 0 && index < complete_samples && index % channels < 2;
            if owned {
                self.owned_samples += 1;
                self.nonfinite_owned_outputs += usize::from(!actual.is_finite());
            } else {
                self.unowned_samples += 1;
            }
            if actual.to_bits() != *expected {
                if owned {
                    self.owned_mismatches += 1;
                } else {
                    self.unowned_mismatches += 1;
                }
                if self.first_mismatches.len() < 12 {
                    self.first_mismatches.push(json!({
                        "call": self.calls, "channels": channels, "sample_index": index,
                        "owned": owned, "expected_bits": expected, "actual_bits": actual.to_bits(),
                    }));
                }
            }
        }
    }
}

#[test]
fn valid_instances_process_only_owned_channels_of_complete_caller_frames() {
    let mut observations = Vec::new();
    let mut failures = Vec::new();
    let layouts = [
        ("stereo_trailing", 2, 1, None),
        ("three_stride", 3, 0, None),
        ("four_stride", 4, 0, None),
        ("three_stride_trailing", 3, 2, None),
        ("four_stride_trailing", 4, 3, None),
        ("zero_channels_interrupt", 2, 0, Some((0, 7))),
        ("incomplete_stereo_interrupt", 2, 0, Some((2, 1))),
        ("incomplete_three_interrupt", 2, 0, Some((3, 2))),
        ("incomplete_four_interrupt", 2, 0, Some((4, 3))),
    ];
    for rate in [1000, 48_000] {
        let delay = rate / 100;
        let frames = 4 * delay + 2;
        let partitions: &[&[usize]] = &[&[frames], &[1], &[7, 31, 64, 128, 512], &[4096, 17]];
        for (layout, channels, trailing, interrupt) in layouts {
            for impulse_channel in 0..2 {
                for parts in partitions {
                    let mut effect = DelayEffect::new(rate as f32);
                    assert!(effect.processing_configuration_valid());
                    assert!(effect.set_param(0, 10.0));
                    assert!(effect.set_param(1, 0.5));
                    assert!(effect.set_param(2, 0.5));
                    assert_eq!(
                        f64::from(effect.get_param(0)) * f64::from(rate as f32) / 1000.0,
                        delay as f64
                    );
                    let mut comparison = Comparison::default();
                    let mut witnesses = Vec::new();
                    let mut frame = 0;
                    let mut part = 0;
                    while frame < frames {
                        if let Some((probe_channels, probe_length)) = interrupt {
                            let mut probe: Vec<_> = (0..probe_length).map(unowned_sample).collect();
                            let expected: Vec<_> =
                                probe.iter().map(|sample| sample.to_bits()).collect();
                            comparison.process(&mut effect, &mut probe, probe_channels, &expected);
                        }
                        let end = (frame + parts[part % parts.len()]).min(frames);
                        let mut samples = Vec::with_capacity((end - frame) * channels + trailing);
                        let mut expected = Vec::with_capacity(samples.capacity());
                        for global_frame in frame..end {
                            for channel in 0..channels {
                                if channel < 2 {
                                    samples.push(
                                        if global_frame == 0 && channel == impulse_channel {
                                            1.0
                                        } else {
                                            0.0
                                        },
                                    );
                                    expected.push(
                                        expected_owned(
                                            global_frame,
                                            channel,
                                            impulse_channel,
                                            delay,
                                        )
                                        .to_bits(),
                                    );
                                } else {
                                    let sample = unowned_sample(global_frame + channel);
                                    samples.push(sample);
                                    expected.push(sample.to_bits());
                                }
                            }
                        }
                        for index in 0..trailing {
                            let sample = unowned_sample(part + index);
                            samples.push(sample);
                            expected.push(sample.to_bits());
                        }
                        comparison.process(&mut effect, &mut samples, channels, &expected);
                        for global_frame in frame..end {
                            if global_frame != 0 && global_frame.is_multiple_of(delay) {
                                witnesses.push(json!({
                                    "frame": global_frame,
                                    "expected_bits": expected_owned(global_frame, impulse_channel, impulse_channel, delay).to_bits(),
                                    "actual_bits": samples[(global_frame - frame) * channels + impulse_channel].to_bits(),
                                }));
                            }
                        }
                        frame = end;
                        part += 1;
                    }
                    if comparison.owned_mismatches != 0 || comparison.unowned_mismatches != 0 {
                        failures.push(format!(
                            "rate={rate}, layout={layout}, impulse={impulse_channel}, parts={parts:?}: owned/unowned mismatches={}/{}",
                            comparison.owned_mismatches, comparison.unowned_mismatches
                        ));
                    }
                    observations.push(json!({
                        "rate": rate, "layout": layout, "channels": channels,
                        "trailing_samples_per_call": trailing, "interrupt": interrupt,
                        "impulse_channel": impulse_channel, "partitions": parts,
                        "delay_frames": delay, "feedback": 0.5, "mix": 0.5,
                        "comparison": "Exact owned geometric impulse bits; unowned input bits unchanged",
                        "calls": comparison.calls, "owned_samples": comparison.owned_samples,
                        "unowned_samples": comparison.unowned_samples,
                        "owned_mismatches": comparison.owned_mismatches,
                        "unowned_mismatches": comparison.unowned_mismatches,
                        "nonfinite_owned_outputs": comparison.nonfinite_owned_outputs,
                        "first_mismatches": comparison.first_mismatches,
                        "positive_echo_witnesses": witnesses,
                    }));
                }
            }
        }
    }
    println!(
        "DELAY_SAMPLE_OWNERSHIP_RECORD {}",
        json!({
            "case": "complete_frame_and_stride_ownership",
            "execution_head": std::env::var("VIBEZ_VERIFICATION_HEAD")
                .unwrap_or_else(|_| "UNLABELLED".into()),
            "observed": observations,
        })
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
