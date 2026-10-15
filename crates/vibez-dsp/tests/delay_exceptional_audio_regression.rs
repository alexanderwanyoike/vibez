//! Requires local exceptional-audio containment while preserving finite Delay echoes and channel isolation.

use serde_json::json;
use vibez_dsp::{delay::DelayEffect, effect::AudioEffect};

fn unowned_sample(index: usize) -> f32 {
    f32::from_bits([0x7fc1_2345, 0xffc2_3456, 0x8000_0000, 0x7f80_0000, 0xff80_0000][index % 5])
}

fn dry_input(case: &str, frame: usize, channel: usize, affected: usize, delay: usize) -> f32 {
    if channel != affected {
        return if frame == 0 { 0.25 } else { 0.0 };
    }
    if case == "nonfinite_input" {
        if frame == delay / 2 {
            return f32::from_bits(0xffc2_3456);
        }
        if frame == delay + 1 {
            return f32::INFINITY;
        }
        if frame == 2 * delay + 1 {
            return f32::NEG_INFINITY;
        }
    }
    let burst = match case {
        "positive_feedback_overflow" => f32::MAX,
        "negative_feedback_overflow" => -f32::MAX,
        "finite_linear_control" => 4.0,
        "nonfinite_input" => 1.0,
        _ => unreachable!(),
    };
    if frame == 0 {
        burst
    } else if frame == delay && case != "nonfinite_input" {
        if case == "finite_linear_control" { 2.0 } else { burst }
    } else if frame == 2 * delay + 3 {
        0.5
    } else {
        0.0
    }
}

fn impulse_wet(frame: usize, at: usize, amplitude: f32, delay: usize) -> f32 {
    if frame >= at + delay && (frame - at) % delay == 0 {
        amplitude * 0.5_f32.powi(((frame - at) / delay - 1) as i32)
    } else {
        0.0
    }
}

fn expected_output(case: &str, frame: usize, channel: usize, affected: usize, delay: usize, mix: f32) -> f32 {
    let dry = dry_input(case, frame, channel, affected, delay);
    let dry = if dry.is_finite() { dry } else { 0.0 };
    let wet = if channel != affected {
        impulse_wet(frame, 0, 0.25, delay)
    } else {
        let recovery = impulse_wet(frame, 2 * delay + 3, 0.5, delay);
        let burst = match case {
            "nonfinite_input" => impulse_wet(frame, 0, 1.0, delay),
            "finite_linear_control" => {
                impulse_wet(frame, 0, 4.0, delay) + impulse_wet(frame, delay, 2.0, delay)
            }
            // The two MAX inputs overflow the feedback write at D, replacing that write with +0.
            "positive_feedback_overflow" => if frame == delay { f32::MAX } else { 0.0 },
            "negative_feedback_overflow" => if frame == delay { -f32::MAX } else { 0.0 },
            _ => unreachable!(),
        };
        burst + recovery
    };
    let output = dry * (1.0 - mix) + wet * mix;
    if output.is_finite() { output } else { 0.0 }
}

#[test]
fn exceptional_owned_audio_is_contained_locally_without_reset_or_normal_gain_changes() {
    let cases = [
        "nonfinite_input", "positive_feedback_overflow",
        "negative_feedback_overflow", "finite_linear_control",
    ];
    let mut failures = Vec::new();
    let mut observations = Vec::new();
    for rate in [1000, 48_000] {
        let delay = rate / 100;
        let frames = 6 * delay + 5;
        let partitions: &[&[usize]] = &[&[frames], &[1, 7, 31, 64, 128, 512]];
        for case in cases {
            for (channels, affected) in [(1, 0), (2, 0), (2, 1), (4, 0), (4, 1)] {
                for mix in [0.0_f32, 0.375, 1.0] {
                    for parts in partitions {
                        let mut effect = DelayEffect::new(rate as f32);
                        assert!(effect.processing_configuration_valid());
                        for (index, value) in [10.0, 0.5, mix].into_iter().enumerate() {
                            assert!(effect.set_param(index, value));
                        }
                        assert_eq!(f64::from(effect.get_param(0)) * f64::from(rate as f32) / 1000.0, delay as f64);
                        let before_targets: [u32; 3] = std::array::from_fn(|index| effect.get_param(index).to_bits());
                        let mut owned_mismatches = 0;
                        let mut unowned_mismatches = 0;
                        let mut nonfinite_owned_outputs = 0;
                        let mut first_mismatches = Vec::new();
                        let mut witnesses = Vec::new();
                        let mut peak_normal_output = 0.0_f32;
                        let mut frame = 0;
                        let mut part = 0;
                        while frame < frames {
                            let end = (frame + parts[part % parts.len()]).min(frames);
                            let trailing = channels - 1;
                            let mut samples = Vec::with_capacity((end - frame) * channels + trailing);
                            let mut expected = Vec::with_capacity(samples.capacity());
                            for global_frame in frame..end {
                                for channel in 0..channels {
                                    if channel < 2 {
                                        samples.push(dry_input(case, global_frame, channel, affected, delay));
                                        expected.push(expected_output(case, global_frame, channel, affected, delay, mix).to_bits());
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
                            effect.process(&mut samples, channels);
                            let complete_samples = (end - frame) * channels;
                            for (index, (actual, expected)) in samples.iter().zip(&expected).enumerate() {
                                let owned = index < complete_samples && index % channels < 2;
                                if owned {
                                    nonfinite_owned_outputs += usize::from(!actual.is_finite());
                                    if case == "finite_linear_control" {
                                        peak_normal_output = peak_normal_output.max(actual.abs());
                                    }
                                }
                                if actual.to_bits() != *expected {
                                    if owned { owned_mismatches += 1; } else { unowned_mismatches += 1; }
                                    if first_mismatches.len() < 12 {
                                        first_mismatches.push(json!({
                                            "block_start_frame": frame, "sample_index": index,
                                            "owned": owned, "expected_bits": expected, "actual_bits": actual.to_bits(),
                                        }));
                                    }
                                }
                            }
                            for global_frame in frame..end {
                                if [delay, 2 * delay, 2 * delay + 3, 3 * delay + 3, 4 * delay + 3, 6 * delay + 3].contains(&global_frame) {
                                    let actual_bits: Vec<_> = (0..channels.min(2)).map(|channel| {
                                        samples[(global_frame - frame) * channels + channel].to_bits()
                                    }).collect();
                                    let expected_bits: Vec<_> = (0..channels.min(2)).map(|channel| {
                                        expected_output(case, global_frame, channel, affected, delay, mix).to_bits()
                                    }).collect();
                                    witnesses.push(json!({ "frame": global_frame, "expected_bits": expected_bits, "actual_bits": actual_bits }));
                                }
                            }
                            frame = end;
                            part += 1;
                        }
                        let after_targets: [u32; 3] = std::array::from_fn(|index| effect.get_param(index).to_bits());
                        if owned_mismatches != 0 || unowned_mismatches != 0 || before_targets != after_targets {
                            failures.push(format!(
                                "case={case}, rate={rate}, channels={channels}, affected={affected}, mix={mix}, parts={parts:?}: owned/unowned errors={owned_mismatches}/{unowned_mismatches}, nonfinite_outputs={nonfinite_owned_outputs}"
                            ));
                        }
                        observations.push(json!({
                            "input_case": case, "rate": rate, "delay_frames": delay,
                            "channels": channels, "affected_channel": affected, "mix": mix,
                            "feedback": 0.5, "partitions": parts,
                            "comparison": "Exact f32 algebra from independent impulse superposition and explicit single overflow-write cancellation",
                            "owned_mismatches": owned_mismatches, "unowned_mismatches": unowned_mismatches,
                            "nonfinite_owned_outputs": nonfinite_owned_outputs,
                            "before_target_bits": before_targets, "after_target_bits": after_targets,
                            "normal_peak_output": peak_normal_output, "witnesses": witnesses,
                            "first_mismatches": first_mismatches,
                        }));
                    }
                }
            }
        }
    }
    println!("DELAY_EXCEPTIONAL_AUDIO_RECORD {}", json!({
        "case": "local_exceptional_audio_containment_and_recovery",
        "policy": "Owned nonfinite dry input becomes +0 before processing; nonfinite f32 feedback writes and mixed outputs become +0 locally; ordinary finite algebra and unowned bits remain unchanged",
        "execution_head": std::env::var("VIBEZ_VERIFICATION_HEAD")
            .unwrap_or_else(|_| "UNLABELLED".into()),
        "observed": observations,
    }));
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
