//! Legacy projects depend on two distinct envelopes and the original hold-frame ordering.

use vibez_dsp::{effect::AudioEffect, gate::GateEffect};

fn signal(rate: f32, channels: usize) -> Vec<f32> {
    let frames = (rate * 0.28) as usize;
    let mut noise = 0x4519_0a31_u32;
    let mut samples = Vec::with_capacity(frames * channels);
    for frame in 0..frames {
        noise = noise.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let ms = frame as f32 * 1000.0 / rate;
        let left = if ms < 8.0 {
            0.0
        } else if ms < 24.0 {
            0.5
        } else if ms < 26.0 {
            (noise as f64 / u32::MAX as f64 * 1.6 - 0.8) as f32
        } else if ms < 65.0 {
            0.0007
        } else if ms < 95.0 {
            0.3 * (std::f32::consts::TAU * 431.0 * frame as f32 / rate).sin()
        } else if ms < 100.0 {
            if frame % 2 == 0 {
                0.0118
            } else {
                0.0125
            }
        } else {
            0.001
        };
        samples.push(left);
        if channels == 2 {
            samples.push(if (65.0..95.0).contains(&ms) {
                -0.42
            } else {
                -left * 0.37
            });
        }
    }
    samples
}

fn legacy_reference(input: &[f32], channels: usize, rate: f32, params: [f32; 4]) -> Vec<f32> {
    let [threshold, attack, release, hold] = params;
    let attack = (-1.0 / ((attack * 0.001).max(0.000_01) * rate)).exp();
    let release = (-1.0 / ((release * 0.001).max(0.000_01) * rate)).exp();
    let hold_frames = (hold * 0.001 * rate) as u32;
    let mut decision_db = -120.0;
    let mut linear_gain = 0.0;
    let mut hold_left = 0;
    let mut expected = Vec::with_capacity(input.len());
    for frame in input.chunks_exact(channels) {
        let peak = frame.iter().copied().map(f32::abs).fold(0.0_f32, f32::max);
        let level_db = 20.0 * peak.max(1e-10).log10();
        let detector_coefficient = if level_db > decision_db {
            attack
        } else {
            release
        };
        decision_db = level_db + detector_coefficient * (decision_db - level_db);
        let opens = decision_db >= threshold;
        let requested_gain = if opens || hold_left > 0 { 1.0 } else { 0.0 };
        let gain_coefficient = if requested_gain > linear_gain {
            attack
        } else {
            release
        };
        linear_gain = requested_gain + gain_coefficient * (linear_gain - requested_gain);
        expected.extend(frame.iter().map(|sample| sample * linear_gain));
        // Decrementing before the gain decision shortens old projects by one frame.
        hold_left = if opens {
            hold_frames
        } else {
            hold_left.saturating_sub(1)
        };
    }
    expected
}

fn render(
    effect: &mut GateEffect,
    source: &[f32],
    channels: usize,
    partitions: &[usize],
) -> Vec<f32> {
    let mut rendered = source.to_vec();
    let mut offset = 0;
    let mut block = 0;
    while offset < rendered.len() {
        let end = (offset + partitions[block % partitions.len()] * channels).min(rendered.len());
        effect.process(&mut rendered[offset..end], channels);
        offset = end;
        block += 1;
    }
    rendered
}

fn assert_samples(actual: &[f32], expected: &[f32], context: &str) {
    assert_eq!(actual.len(), expected.len());
    for (index, (&actual, &expected)) in actual.iter().zip(expected).enumerate() {
        let tolerance = 2e-8 + expected.abs() * 2e-7;
        assert!((actual - expected).abs() <= tolerance,
            "{context}: sample {index}: actual {actual}, expected {expected}, tolerance {tolerance}");
    }
}

#[test]
fn legacy_double_envelope_and_hold_remain_sample_accurate_across_rates_blocks_and_reset() {
    for rate in [44_100.0, 48_000.0, 96_000.0] {
        for channels in [1, 2] {
            let source = signal(rate, channels);
            for params in [
                [-40.0, 1.0, 50.0, 10.0],
                [-24.0, 0.1, 5.0, 0.0],
                [-32.0, 2.5, 15.0, 17.37],
            ] {
                let expected = legacy_reference(&source, channels, rate, params);
                for partitions in [&[source.len() / channels][..], &[1, 7, 32, 251, 1024][..]] {
                    let mut effect = GateEffect::new(rate);
                    for (index, value) in params.into_iter().enumerate() {
                        assert!(effect.set_param(index, value));
                    }
                    let context = format!("{rate}Hz/{channels}ch/{params:?}/{partitions:?}");
                    assert_samples(
                        &render(&mut effect, &source, channels, partitions),
                        &expected,
                        &context,
                    );
                    effect.reset();
                    assert_samples(
                        &render(&mut effect, &source, channels, partitions),
                        &expected,
                        &context,
                    );
                }
            }
        }
    }
}
