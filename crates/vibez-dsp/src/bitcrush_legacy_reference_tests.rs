//! An independent quantizer and acquisition clock preserve legacy Bitcrush projects.

use crate::{effect::AudioEffect, factory::create_effect_with_params};
use vibez_core::effect::EffectType;

fn original(input: &[f32], channels: usize, parameters: [f32; 3]) -> Vec<f32> {
    let [bits, reduction, mix] = parameters;
    let step = 2.0_f32.powi(1 - bits.clamp(2.0, 16.0).trunc() as i32);
    let divider = reduction.clamp(1.0, 32.0).round() as usize;
    let mut held = [0.0; 2];
    let mut phase = 0;
    let mut result = input.to_vec();
    for pair in result.chunks_exact_mut(channels) {
        for (channel, sample) in pair.iter_mut().enumerate() {
            if phase == 0 {
                held[channel] = ((*sample / step).round() * step).clamp(-1.0, 1.0);
            }
            *sample = *sample * (1.0 - mix) + held[channel] * mix;
        }
        phase = (phase + 1) % divider;
    }
    result
}

fn render(
    effect: &mut dyn AudioEffect,
    input: &[f32],
    channels: usize,
    parts: &[usize],
) -> Vec<f32> {
    let mut output = input.to_vec();
    let mut position = 0;
    let mut part = 0;
    while position < output.len() {
        let end = (position + parts[part % parts.len()] * channels).min(output.len());
        effect.process(&mut output[position..end], channels);
        position = end;
        part += 1;
    }
    output
}

#[test]
fn three_value_projects_keep_fractional_quantization_hold_mix_and_reset_samples() {
    let bits: Vec<_> = (2..=16)
        .map(|value| value as f32)
        .chain([2.99, 7.99, 10.25, 10.99, 15.99])
        .collect();
    let reductions: Vec<_> = (1..=32)
        .map(|value| value as f32)
        .chain([1.49, 1.5, 1.51, 17.49, 17.5, 31.49, 31.5])
        .collect();
    for rate in [44_100.0, 48_000.0, 96_000.0] {
        for channels in [1, 2] {
            for &bits in &bits {
                let step = 2.0_f32.powi(1 - bits.trunc() as i32);
                let input: Vec<_> = (0..97)
                    .flat_map(|frame| {
                        (0..channels).map(move |channel| {
                            let value = match frame % 11 {
                                0 => step * 0.5,
                                1 => -step * 0.5,
                                2 => step * 1.5,
                                3 => -step * 1.5,
                                4 => 0.0,
                                5 => 1.0,
                                6 => -1.0,
                                7 => 1.75,
                                8 => -1.5,
                                _ => frame as f32 / 49.0 - 0.8,
                            };
                            if channel == 0 {
                                value
                            } else {
                                -value * 0.37
                            }
                        })
                    })
                    .collect();
                for &reduction in &reductions {
                    for mix in [0.0, 0.37, 1.0] {
                        let parameters = [bits, reduction, mix];
                        let expected = original(&input, channels, parameters);
                        for parts in [&[97][..], &[1, 7, 31, 5, 64][..]] {
                            let mut effect =
                                create_effect_with_params(EffectType::Bitcrush, rate, &parameters);
                            assert_eq!(effect.latency_samples(), 0);
                            for (index, value) in parameters.into_iter().enumerate() {
                                assert_eq!(effect.get_param(index), value);
                            }
                            let actual = render(&mut *effect, &input, channels, parts);
                            assert_eq!(
                                actual, expected,
                                "rate={rate}, channels={channels}, values={parameters:?}"
                            );
                            effect.reset();
                            assert_eq!(render(&mut *effect, &input, channels, parts), expected);
                        }
                    }
                }
            }
        }
    }
}
