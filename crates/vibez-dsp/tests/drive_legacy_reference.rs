//! The original four-control Drive sound survives host-rate and callback boundaries.

use vibez_core::effect::EffectType;
use vibez_dsp::{effect::AudioEffect, factory::create_effect_with_params};

fn original(input: &[f32], controls: [f32; 4]) -> Vec<f32> {
    let [amount, tone, output, mix] = controls;
    let gain = 1.0 + amount * amount * 29.0;
    let normalizer = gain.tanh();
    let mut previous = [0.0; 2];
    input
        .iter()
        .enumerate()
        .map(|(index, &dry)| {
            let channel = index % 2;
            let shaped = (dry * gain).tanh() / normalizer;
            let tilted = shaped + (2.0 * tone - 1.0) * (shaped - previous[channel]);
            previous[channel] = shaped;
            let wet = dry * (1.0 - amount) + tilted * amount;
            (dry * (1.0 - mix) + wet * mix) * output
        })
        .collect()
}

fn render(effect: &mut dyn AudioEffect, input: &[f32], partitions: &[usize]) -> Vec<f32> {
    let mut output = input.to_vec();
    let mut frame = 0;
    let mut part = 0;
    while frame < input.len() / 2 {
        let end = (frame + partitions[part % partitions.len()]).min(input.len() / 2);
        effect.process(&mut output[frame * 2..end * 2], 2);
        frame = end;
        part += 1;
    }
    output
}

#[test]
fn four_value_projects_preserve_original_tanh_tilt_blends_after_reported_delay() {
    for rate in [44_100.0, 48_000.0, 96_000.0] {
        let signal: Vec<_> = (0..4096)
            .flat_map(|frame| {
                let phase = std::f32::consts::TAU * 1379.0 * frame as f32 / rate;
                [0.41 * phase.sin() + 0.03, -0.29 * (phase * 0.73).cos()]
            })
            .collect();
        for controls in [
            [0.35, 0.5, 1.0, 1.0],
            [0.0, 0.0, 1.7, 1.0],
            [0.81, 0.0, 0.7, 1.0],
            [0.51, 0.25, 1.1, 0.37],
            [1.0, 1.0, 0.8, 0.91],
            [0.73, 0.91, 1.4, 0.0],
        ] {
            let expected = original(&signal, controls);
            for partitions in [&[4096][..], &[1, 7, 61, 128, 3][..]] {
                let mut effect = create_effect_with_params(EffectType::Drive, rate, &controls);
                let delay = effect.latency_samples() as usize;
                let mut padded = signal.clone();
                padded.extend(std::iter::repeat_n(0.0, delay * 2));
                let actual = render(effect.as_mut(), &padded, partitions);
                assert!(actual[..delay * 2].iter().all(|&sample| sample == 0.0));
                for (index, (&actual, &expected)) in actual[delay * 2..].iter().zip(&expected).enumerate() {
                    assert!((actual - expected).abs() < 1e-7,
                        "rate={rate}, controls={controls:?}, parts={partitions:?}, sample={index}: {actual} != {expected}");
                }
                effect.reset();
                let replay = render(effect.as_mut(), &padded, partitions);
                assert_eq!(replay, actual, "reset rate={rate}, controls={controls:?}");
            }
        }
    }
}
