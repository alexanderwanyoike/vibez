//! Characterizes the original public Delay contract with independent integer-time audio expectations.

use serde_json::{json, Value};
use vibez_core::effect::EffectType;
use vibez_dsp::{delay::DelayEffect, effect::AudioEffect};

fn record(case: &str, observed: Value) {
    println!(
        "DELAY_LEGACY_RECORD {}",
        json!({
            "case": case,
            "execution_head": std::env::var("VIBEZ_VERIFICATION_HEAD")
                .unwrap_or_else(|_| "UNLABELLED".into()),
            "original_delay_blob": "ec654ec086255e377d38b6095adceb28bf15696a",
            "observed": observed,
        })
    );
}

fn configured(rate: f32, parameters: [f32; 3]) -> DelayEffect {
    let mut effect = DelayEffect::new(rate);
    for (index, value) in parameters.into_iter().enumerate() {
        assert!(effect.set_param(index, value));
    }
    effect
}

fn render(effect: &mut dyn AudioEffect, input: &[f32], parts: &[usize]) -> Vec<f32> {
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

fn impulse_response(
    frames: usize,
    delay_frames: usize,
    input: [f32; 2],
    feedback: f32,
    mix: f32,
) -> Vec<f32> {
    let mut expected = vec![0.0; frames * 2];
    for (channel, amplitude) in input.into_iter().enumerate() {
        expected[channel] = amplitude * (1.0 - mix);
        let mut repeat = amplitude;
        for frame in (delay_frames..frames).step_by(delay_frames) {
            expected[2 * frame + channel] = repeat * mix;
            repeat *= feedback;
        }
    }
    expected
}

fn signal(frames: usize) -> Vec<f32> {
    (0..frames)
        .flat_map(|frame| {
            (0..2).map(move |channel| ((frame * 17 + channel * 11) % 37) as f32 / 32.0 - 0.5)
        })
        .collect()
}

fn fingerprint(samples: &[f32]) -> String {
    let mut value = 0xcbf2_9ce4_8422_2325_u64;
    for sample in samples {
        for byte in sample.to_le_bytes() {
            value = (value ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
        }
    }
    format!("{value:016x}")
}

#[test]
fn original_descriptors_getters_activation_and_zero_latency_remain_observable() {
    let expected: [(&str, f32, f32, f32, &str); 3] = [
        ("Time", 1.0, 2000.0, 500.0, "ms"),
        ("Feedback", 0.0, 0.95, 0.5, ""),
        ("Mix", 0.0, 1.0, 0.3, ""),
    ];
    let mut observations = Vec::new();
    for rate in [1000, 44_100, 48_000, 96_000, 192_000] {
        let mut effect = DelayEffect::new(rate as f32);
        assert_eq!(effect.effect_type(), EffectType::Delay);
        assert_eq!(effect.activation_sample_rate(), Some(rate));
        assert!(effect.processing_configuration_valid());
        assert_eq!(effect.latency_samples(), 0);
        assert_eq!(effect.param_descriptors().len(), 3);
        for (index, (name, minimum, maximum, default, unit)) in expected.into_iter().enumerate() {
            let descriptor = effect.param_descriptors()[index];
            assert_eq!(descriptor.name, name);
            assert_eq!(descriptor.min.to_bits(), minimum.to_bits());
            assert_eq!(descriptor.max.to_bits(), maximum.to_bits());
            assert_eq!(descriptor.default.to_bits(), default.to_bits());
            assert_eq!(descriptor.unit, unit);
            assert_eq!(effect.get_param(index).to_bits(), default.to_bits());
        }
        assert!(!effect.set_param(99, 1.0));
        for (index, target) in [125.0_f32, 0.625, 0.37].into_iter().enumerate() {
            assert!(effect.set_param(index, target));
            assert_eq!(effect.get_param(index).to_bits(), target.to_bits());
        }
        observations.push(json!({
            "activation_rate": effect.activation_sample_rate(),
            "latency_samples": effect.latency_samples(),
            "descriptor_count": effect.param_descriptors().len(),
            "target_bits": (0..3).map(|index| effect.get_param(index).to_bits()).collect::<Vec<_>>(),
        }));
    }
    record("descriptors_getters_activation_latency", json!(observations));
}

#[test]
fn integer_stereo_impulses_keep_arrivals_geometric_decay_and_linear_mix() {
    // These products are exact f32 seconds, excluding the separate fractional/endpoint defects.
    assert_eq!((125.0_f32 * 0.001).to_bits(), 0.125_f32.to_bits());
    assert_eq!((500.0_f32 * 0.001).to_bits(), 0.5_f32.to_bits());
    let cases = [
        (1000, 125.0, 125),
        (44_100, 500.0, 22_050),
        (48_000, 125.0, 6000),
        (96_000, 125.0, 12_000),
        (192_000, 125.0, 24_000),
    ];
    let mut observations = Vec::new();
    for (rate, time_ms, delay_frames) in cases {
        let frames = delay_frames * 6 + 3;
        let mut input = vec![0.0; frames * 2];
        input[..2].copy_from_slice(&[0.75, -0.25]);
        for feedback in [0.0, 0.5, 0.95] {
            for mix in [0.0, 0.37, 1.0] {
                let expected = impulse_response(frames, delay_frames, [0.75, -0.25], feedback, mix);
                let mut effect = configured(rate as f32, [time_ms, feedback, mix]);
                let actual = render(&mut effect, &input, &[1, 7, 64, 128, 512, 31]);
                assert_eq!(actual, expected, "rate={rate}, feedback={feedback}, mix={mix}");
                observations.push(json!({
                    "rate": rate,
                    "time_ms": time_ms,
                    "delay_frames": delay_frames,
                    "feedback": feedback,
                    "mix": mix,
                    "dry_bits": [actual[0].to_bits(), actual[1].to_bits()],
                    "repeat_bits": (1..=6).map(|repeat| {
                        let at = 2 * repeat * delay_frames;
                        [actual[at].to_bits(), actual[at + 1].to_bits()]
                    }).collect::<Vec<_>>(),
                    "output_fnv1a64": fingerprint(&actual),
                }));
            }
        }
    }
    record("integer_stereo_repeats", json!(observations));
}

#[test]
fn arbitrary_stereo_audio_uses_the_original_f32_linear_crossmix() {
    let input = signal(4500);
    let mut wet_effect = configured(1000.0, [125.0, 0.625, 1.0]);
    let wet = render(&mut wet_effect, &input, &[4500]);
    let mut observations = Vec::new();
    for mix in [0.0, 0.125, 0.37, 0.75, 1.0] {
        let expected: Vec<_> = input
            .iter()
            .zip(&wet)
            .map(|(dry, delayed)| dry * (1.0_f32 - mix) + delayed * mix)
            .collect();
        let mut effect = configured(1000.0, [125.0, 0.625, mix]);
        let actual = render(&mut effect, &input, &[31, 1, 128, 7]);
        assert_eq!(actual, expected, "mix={mix}");
        observations.push(json!({
            "mix": mix,
            "frames": input.len() / 2,
            "output_fnv1a64": fingerprint(&actual),
            "first_dry_bits": [actual[0].to_bits(), actual[1].to_bits()],
        }));
    }
    record("linear_f32_crossmix", json!(observations));
}

#[test]
fn reset_clears_audible_history_and_keeps_the_saved_targets() {
    let parameters = [125.0, 0.625, 0.37];
    let mut effect = configured(1000.0, parameters);
    let _ = render(&mut effect, &signal(4500), &[7, 128, 31]);
    effect.reset();
    for (index, value) in parameters.into_iter().enumerate() {
        assert_eq!(effect.get_param(index).to_bits(), value.to_bits());
    }
    let silent = render(&mut effect, &vec![0.0; 4500 * 2], &[1, 31, 512]);
    assert!(silent.iter().all(|sample| *sample == 0.0));
    effect.reset();
    let probe = signal(2500);
    let actual = render(&mut effect, &probe, &[1, 7, 31]);
    let mut fresh = configured(1000.0, parameters);
    let expected = render(&mut fresh, &probe, &[2500]);
    assert_eq!(actual, expected);
    assert_eq!(effect.activation_sample_rate(), Some(1000));
    assert_eq!(effect.latency_samples(), 0);
    record("reset", json!({
        "silent_frames": silent.len() / 2,
        "silent_nonzero_samples": silent.iter().filter(|sample| **sample != 0.0).count(),
        "replayed_output_fnv1a64": fingerprint(&actual),
        "target_bits": parameters.map(f32::to_bits),
    }));
}

#[test]
fn independent_instances_and_stereo_channels_do_not_share_delay_history() {
    let mut first = configured(1000.0, [125.0, 0.5, 1.0]);
    let mut second = configured(1000.0, [125.0, 0.5, 1.0]);
    first.process(&mut [1.0, 0.0], 2);
    second.process(&mut [0.0, -0.5], 2);
    first.reset();
    assert!(first.set_param(1, 0.75));
    assert_eq!(second.get_param(1), 0.5);
    let silence = vec![0.0; 500 * 2];
    let first_output = render(&mut first, &silence, &[7, 31, 128]);
    let second_output = render(&mut second, &silence, &[1, 64]);
    assert!(first_output.iter().all(|sample| *sample == 0.0));
    assert!(second_output.chunks_exact(2).all(|pair| pair[0] == 0.0));
    let expected = impulse_response(501, 125, [0.0, -0.5], 0.5, 1.0);
    assert_eq!(second_output.as_slice(), &expected[2..]);
    record("instance_and_channel_isolation", json!({
        "reset_instance_nonzero_samples": first_output.iter().filter(|sample| **sample != 0.0).count(),
        "unreset_right_repeat_bits": [124, 249, 374, 499]
            .map(|frame| second_output[2 * frame + 1].to_bits()),
        "unreset_output_fnv1a64": fingerprint(&second_output),
    }));
}

#[test]
fn integer_delay_samples_are_identical_across_partitions_and_ring_wraps() {
    let input = signal(5003);
    let parameters = [125.0, 0.625, 0.37];
    let mut whole_effect = configured(1000.0, parameters);
    let whole = render(&mut whole_effect, &input, &[5003]);
    let partitions: &[&[usize]] = &[&[1], &[7, 31, 3], &[64], &[128], &[512], &[4096, 17]];
    for parts in partitions {
        let mut effect = configured(1000.0, parameters);
        let actual = render(&mut effect, &input, parts);
        assert_eq!(actual, whole, "parts={parts:?}");
    }
    record("partitions_and_ring_wraps", json!({
        "rate": 1000,
        "time_ms": 125,
        "frames": input.len() / 2,
        "original_ring_frames": 2000,
        "partitions": partitions,
        "output_fnv1a64": fingerprint(&whole),
    }));
}
