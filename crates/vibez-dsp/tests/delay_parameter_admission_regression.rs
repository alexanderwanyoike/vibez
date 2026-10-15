//! Requires legacy Delay parameter admission to preserve targets and audible history on rejection.

use serde_json::json;
use vibez_dsp::{delay::DelayEffect, effect::AudioEffect};

fn edit(effect: &mut DelayEffect, automated: bool, index: usize, value: f32) -> bool {
    if automated {
        effect.set_automated_param(index, value)
    } else {
        effect.set_param(index, value)
    }
}

fn target_bits(effect: &DelayEffect) -> [u32; 3] {
    std::array::from_fn(|index| effect.get_param(index).to_bits())
}

fn prepare(rate: f32) -> DelayEffect {
    let mut effect = DelayEffect::new(rate);
    assert!(effect.processing_configuration_valid());
    for (index, value) in [10.0, 0.5, 0.5].into_iter().enumerate() {
        assert!(effect.set_param(index, value));
    }
    effect
}

fn expected_sample(frame: usize, channel: usize, delay: usize) -> f32 {
    let amplitude = if channel == 0 { 1.0 } else { 0.25 };
    if frame == 0 {
        amplitude * 0.5
    } else if frame % delay == 0 {
        amplitude * 0.5_f32.powi((frame / delay) as i32)
    } else {
        0.0
    }
}

#[test]
fn rejected_manual_and_automated_edits_preserve_legacy_targets_and_impulse_history() {
    let mut edits = Vec::new();
    for index in 0..3 {
        for (label, value) in [("nan", f32::NAN), ("positive_infinity", f32::INFINITY), ("negative_infinity", f32::NEG_INFINITY)] {
            edits.push((index, label, value));
        }
    }
    // The appended layout admits 3..16; rejection now starts at 17.
    for index in [17, usize::MAX] {
        for (label, value) in [("unknown_finite", 0.25), ("unknown_nan", f32::NAN), ("unknown_positive_infinity", f32::INFINITY), ("unknown_negative_infinity", f32::NEG_INFINITY)] {
            edits.push((index, label, value));
        }
    }
    let mut failures = Vec::new();
    let mut observations = Vec::new();
    for automated in [false, true] {
        for rate in [1000, 48_000] {
            let delay = rate / 100;
            let frames = 4 * delay + 2;
            let prefix_frames = delay / 2 + 1;
            let partitions: &[&[usize]] = &[&[frames], &[1, 7, 31, 64, 128, 512]];
            for &(index, label, value) in &edits {
                for parts in partitions {
                    let mut effect = prepare(rate as f32);
                    assert_eq!(f64::from(effect.get_param(0)) * f64::from(rate as f32) / 1000.0, delay as f64);
                    let mut prefix = vec![0.0; 2 * prefix_frames];
                    prefix[0] = 1.0;
                    prefix[1] = 0.25;
                    effect.process(&mut prefix, 2);
                    let before = target_bits(&effect);
                    let accepted = edit(&mut effect, automated, index, value);
                    let after_edit = target_bits(&effect);
                    let mut output = vec![0.0; 2 * (frames - prefix_frames)];
                    let mut frame = 0;
                    let mut part = 0;
                    while frame < output.len() / 2 {
                        let end = (frame + parts[part % parts.len()]).min(output.len() / 2);
                        effect.process(&mut output[2 * frame..2 * end], 2);
                        frame = end;
                        part += 1;
                    }
                    let mismatches = prefix.iter().chain(output.iter()).enumerate().filter(|(sample, actual)| {
                        actual.to_bits() != expected_sample(sample / 2, sample % 2, delay).to_bits()
                    }).count();
                    let nonfinite_outputs = prefix.iter().chain(output.iter()).filter(|sample| !sample.is_finite()).count();
                    let after_audio = target_bits(&effect);
                    let witnesses: Vec<_> = (1..=4).map(|repeat| {
                        let frame = repeat * delay;
                        let offset = 2 * (frame - prefix_frames);
                        json!({
                            "frame": frame,
                            "expected_bits": [expected_sample(frame, 0, delay).to_bits(), expected_sample(frame, 1, delay).to_bits()],
                            "actual_bits": [output[offset].to_bits(), output[offset + 1].to_bits()],
                        })
                    }).collect();
                    if accepted || after_edit != before || after_audio != before || mismatches != 0 {
                        failures.push(format!(
                            "automated={automated}, rate={rate}, index={index}, value={label}, parts={parts:?}: accepted={accepted}, target_bits_changed={}, sample_mismatches={mismatches}",
                            after_edit != before || after_audio != before
                        ));
                    }
                    observations.push(json!({
                        "automated": automated, "rate": rate, "index": index,
                        "value_case": label, "requested_value_bits": value.to_bits(),
                        "partitions": parts, "edit_after_frames": prefix_frames,
                        "accepted": accepted, "before_target_bits": before,
                        "after_edit_target_bits": after_edit, "after_audio_target_bits": after_audio,
                        "comparison": "Exact original target bits and independent dyadic geometric echo samples",
                        "sample_mismatches": mismatches, "nonfinite_outputs": nonfinite_outputs,
                        "positive_echo_witnesses": witnesses,
                    }));
                }
            }
        }
    }
    println!("DELAY_PARAMETER_REJECTION_RECORD {}", json!({
        "case": "legacy_rejected_edits_preserve_targets_and_history",
        "execution_head": std::env::var("VIBEZ_VERIFICATION_HEAD")
            .unwrap_or_else(|_| "UNLABELLED".into()),
        "observed": observations,
    }));
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn finite_manual_and_automated_legacy_values_keep_the_original_clamp_bounds() {
    let cases: [(usize, f32, f32); 18] = [
        (0, -f32::MAX, 1.0), (0, 0.0, 1.0), (0, 1.0, 1.0),
        (0, 16.25, 16.25), (0, 2000.0, 2000.0), (0, f32::MAX, 2000.0),
        (1, -f32::MAX, 0.0), (1, 0.0, 0.0), (1, 0.375, 0.375),
        (1, 0.5, 0.5), (1, 0.95, 0.95), (1, f32::MAX, 0.95),
        (2, -f32::MAX, 0.0), (2, 0.0, 0.0), (2, 0.375, 0.375),
        (2, 0.5, 0.5), (2, 1.0, 1.0), (2, f32::MAX, 1.0),
    ];
    let mut observations = Vec::new();
    let mut failures = Vec::new();
    for automated in [false, true] {
        let mut effect = prepare(1000.0);
        for (index, value, expected_value) in cases {
            for (baseline_index, baseline) in [10.0, 0.5, 0.5].into_iter().enumerate() {
                assert!(effect.set_param(baseline_index, baseline));
            }
            let mut expected = [10.0_f32.to_bits(), 0.5_f32.to_bits(), 0.5_f32.to_bits()];
            expected[index] = expected_value.to_bits();
            let accepted = edit(&mut effect, automated, index, value);
            let actual = target_bits(&effect);
            if !accepted || actual != expected {
                failures.push(format!("automated={automated}, index={index}, finite_value={value}: clamp mismatch"));
            }
            observations.push(json!({
                "automated": automated, "index": index, "requested_bits": value.to_bits(),
                "expected_target_bits": expected, "actual_target_bits": actual, "accepted": accepted,
            }));
        }
    }
    println!("DELAY_PARAMETER_CLAMP_RECORD {}", json!({
        "case": "original_finite_legacy_clamp_bounds",
        "execution_head": std::env::var("VIBEZ_VERIFICATION_HEAD")
            .unwrap_or_else(|_| "UNLABELLED".into()),
        "observed": observations,
    }));
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
