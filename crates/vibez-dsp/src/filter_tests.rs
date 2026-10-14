//! Signal and compatibility regressions for the native Filter refresh.

use super::*;
use vibez_core::audio_context::DeviceAudioContext;

fn render(mode: f32, slope: f32, cutoff: f32, frequency: f32) -> f32 {
    let mut effect = FilterEffect::new(48000.0);
    effect.set_param(0, cutoff);
    effect.set_param(2, mode);
    effect.set_param(3, slope);
    let mut samples: Vec<_> = (0..48000)
        .map(|index| 0.1 * (std::f32::consts::TAU * frequency * index as f32 / 48000.0).sin())
        .collect();
    effect.process(&mut samples, 1);
    (samples[24000..]
        .iter()
        .map(|sample| sample * sample)
        .sum::<f32>()
        / 24000.0)
        .sqrt()
        / (0.1 / 2.0_f32.sqrt())
}

#[test]
fn legacy_parameters_and_neutral_appended_defaults_keep_their_meaning() {
    let effect = FilterEffect::new(48000.0);
    assert_eq!(effect.param_descriptors().len(), 13);
    for (index, expected) in [
        1000.0, 0.707, 0.0, 0.0, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0, 0.0, 10.0, 150.0,
    ]
    .into_iter()
    .enumerate()
    {
        assert_eq!(effect.get_param(index), expected);
    }
    assert!(effect.supports_level_meter());
}

#[test]
fn all_four_modes_have_their_expected_pass_and_stop_bands() {
    assert!(render(0.0, 0.0, 1000.0, 100.0) > 0.98);
    assert!(render(0.0, 0.0, 1000.0, 8000.0) < 0.02);
    assert!(render(1.0, 0.0, 1000.0, 100.0) < 0.02);
    assert!(render(1.0, 0.0, 1000.0, 8000.0) > 0.98);
    assert!((render(2.0, 0.0, 1000.0, 1000.0) - 1.0).abs() < 0.005);
    assert!(render(2.0, 0.0, 1000.0, 100.0) < 0.15);
    assert!(render(3.0, 0.0, 1000.0, 1000.0) < 0.001);
    assert!(render(3.0, 0.0, 1000.0, 100.0) > 0.98);
}

#[test]
fn the_second_stage_increases_stop_band_attenuation() {
    let single = render(0.0, 0.0, 1000.0, 4000.0);
    let double = render(0.0, 1.0, 1000.0, 4000.0);
    assert!(double < single * 0.1, "{single}, {double}");
}

#[test]
fn fully_dry_is_exact_and_preserves_multichannel_interleaving() {
    let mut effect = FilterEffect::new(48000.0);
    assert!(effect.set_param(5, 0.0));
    let input = [0.5, -0.25, 0.75, -0.5, -0.5, 0.25, -0.75, 0.5];
    let mut output = input;
    effect.process(&mut output, 4);
    assert_eq!(output, input);
}

#[test]
fn unsupported_channels_remain_untouched_while_stereo_is_filtered() {
    let mut effect = FilterEffect::new(48000.0);
    let mut output = [0.5, 0.25, 0.75, -0.5, 0.5, 0.25, 0.75, -0.5];
    effect.process(&mut output, 4);
    assert_eq!(
        [output[2], output[3], output[6], output[7]],
        [0.75, -0.5, 0.75, -0.5]
    );
    assert!((output[0] - output[1] * 2.0).abs() < 1e-6);
    assert!(output[0].abs() < 0.01);
}

#[test]
fn manual_edits_ramp_and_automation_applies_on_the_exact_frame() {
    let mut manual = FilterEffect::new(48000.0);
    let mut automated = FilterEffect::new(48000.0);
    let mut initial = [0.25; 4096];
    manual.process(&mut initial, 1);
    automated.process(&mut [0.25; 4096], 1);
    manual.set_param(5, 0.0);
    automated.set_automated_param(5, 0.0);
    let mut manual_output = [0.5; 1024];
    let mut automated_output = manual_output;
    manual.process(&mut manual_output, 1);
    automated.process(&mut automated_output, 1);
    assert!((manual_output[0] - 0.25).abs() < 0.01);
    assert_eq!(automated_output, [0.5; 1024]);
    assert_eq!(manual_output[1023], 0.5);
}

#[test]
fn every_parameter_rejects_nonfinite_values_without_mutating_its_target() {
    let mut effect = FilterEffect::new(48000.0);
    for index in 0..13 {
        let before = effect.get_param(index);
        for invalid in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert!(!effect.set_param(index, invalid), "parameter {index}");
            assert!(
                !effect.set_automated_param(index, invalid),
                "parameter {index}"
            );
            assert_eq!(effect.get_param(index), before);
        }
    }
}

#[test]
fn cutoff_is_safe_at_low_sample_rates_and_all_modes_are_finite() {
    for rate in [8000.0, 22050.0, 44100.0, 96000.0] {
        for mode in 0..4 {
            let mut effect = FilterEffect::new(rate);
            effect.set_param(0, 20000.0);
            effect.set_param(1, 10.0);
            effect.set_param(2, mode as f32);
            effect.set_param(3, 1.0);
            let mut impulse = [0.0; 8192];
            impulse[0] = 0.25;
            effect.process(&mut impulse, 1);
            assert!(impulse
                .iter()
                .all(|sample| sample.is_finite() && sample.abs() < 100.0));
            assert!(impulse[4096..].iter().all(|sample| sample.abs() < 0.01));
        }
    }
}

fn modulated_render(bpm: f64, block: usize, source: f32) -> Vec<f32> {
    let mut effect = FilterEffect::new(48000.0);
    for (parameter, value) in [
        (0, 800.0),
        (6, source),
        (7, 1.0),
        (8, 3.0),
        (9, 3.0),
        (10, 180.0),
    ] {
        assert!(effect.set_param(parameter, value));
    }
    let mut output: Vec<_> = (0..24000)
        .flat_map(|frame| {
            let sample = 0.1 * (std::f32::consts::TAU * 2000.0 * frame as f32 / 48000.0).sin();
            [sample, sample]
        })
        .collect();
    for (index, part) in output.chunks_mut(block * 2).enumerate() {
        effect.set_audio_context(DeviceAudioContext {
            musical_sample: (index * block) as u64,
            continuous_sample: (index * block) as u64,
            sample_rate: 48000,
            bpm,
            playing: true,
        });
        effect.process(part, 2);
    }
    output
}

#[test]
fn synced_lfo_follows_tempo_and_preserves_block_partition_invariance() {
    let slow = modulated_render(60.0, 64, 1.0);
    let fast = modulated_render(120.0, 64, 1.0);
    assert!(slow.iter().zip(&fast).any(|(a, b)| (a - b).abs() > 0.01));
    let partitioned = modulated_render(60.0, 127, 1.0);
    assert!(slow
        .iter()
        .zip(&partitioned)
        .all(|(a, b)| (a - b).abs() < 1e-5));
    assert!(slow
        .chunks_exact(2)
        .any(|frame| (frame[0] - frame[1]).abs() > 0.01));
}

#[test]
fn envelope_modulation_changes_the_actual_sound_and_silence_stays_silent() {
    let envelope = modulated_render(120.0, 64, 2.0);
    let disabled = modulated_render(120.0, 64, 0.0);
    assert!(envelope
        .iter()
        .zip(&disabled)
        .any(|(a, b)| (a - b).abs() > 0.005));
    let mut effect = FilterEffect::new(48000.0);
    effect.set_param(6, 2.0);
    effect.set_param(8, 6.0);
    let mut silence = [0.0; 8192];
    effect.process(&mut silence, 2);
    assert_eq!(silence, [0.0; 8192]);
}

#[test]
fn static_response_matches_rendered_audio_including_parallel_phase_cancellation() {
    for mode in 0..4 {
        for slope in 0..2 {
            for mix in [0.0, 0.5, 1.0] {
                for frequency in [187.5, 937.5, 3750.0] {
                    let mut effect = FilterEffect::new(48000.0);
                    for (index, value) in [(0, 1500.0), (2, mode as f32), (3, slope as f32), (5, mix)] {
                        effect.set_param(index, value);
                    }
                    let mut audio: Vec<f32> = (0..8192).map(|frame| {
                        (0.1 * (std::f64::consts::TAU * frequency as f64 * frame as f64 / 48000.0).sin()) as f32
                    }).collect();
                    effect.process(&mut audio, 1);
                    let measured = (audio[4096..].iter().map(|sample| (*sample as f64).powi(2)).sum::<f64>() / 4096.0).sqrt() / (0.1 / 2.0_f64.sqrt());
                    let displayed = response_magnitude(48000.0, 1500.0, 0.707, mode as f32, slope as f32, mix, frequency);
                    assert!((measured - displayed as f64).abs() < 1e-4, "mode {mode} slope {slope} mix {mix} f {frequency}: rendered {measured}, displayed {displayed}");
                }
            }
        }
    }
}

#[test]
fn constructor_and_response_handle_boundary_and_invalid_sample_rates() {
    for rate in [0.0, 1.0, 2.0, 40.0, f32::NAN, f32::INFINITY] {
        let mut effect = FilterEffect::new(rate);
        let mut impulse = [0.0; 1024];
        impulse[0] = 0.1;
        effect.process(&mut impulse, 1);
        assert!(impulse.into_iter().all(f32::is_finite));
        assert!(response_magnitude(rate, 20000.0, 10.0, 0.0, 1.0, 1.0, 1000.0).is_finite());
    }
}

#[test]
fn drive_zero_keeps_the_legacy_linear_path_and_positive_drive_creates_harmonics() {
    let mut clean = FilterEffect::new(48000.0);
    let mut driven = FilterEffect::new(48000.0);
    for effect in [&mut clean, &mut driven] { effect.set_param(0, 18000.0); }
    driven.set_param(4, 18.0);
    let mut input: Vec<_> = (0..8192).map(|frame| (0.5 * (std::f64::consts::TAU * 375.0 * frame as f64 / 48000.0).sin()) as f32).collect();
    let mut output = input.clone();
    clean.process(&mut input, 1);
    driven.process(&mut output, 1);
    let harmonic = |samples: &[f32]| samples[4096..].iter().enumerate().map(|(frame, sample)| *sample as f64 * (std::f64::consts::TAU * 1125.0 * frame as f64 / 48000.0).sin()).sum::<f64>().abs() * 2.0 / 4096.0;
    assert!(harmonic(&input) < 1e-5);
    assert!(harmonic(&output) > 0.05);
    assert!(output.into_iter().all(f32::is_finite));
}

#[test]
fn sweeps_and_rapid_manual_edits_remain_finite_across_rates_and_block_sizes() {
    for rate in [8000.0, 44100.0, 96000.0] {
        for block in [1, 17, 256] {
            let mut effect = FilterEffect::new(rate);
            effect.set_param(1, 5.0);
            effect.set_param(3, 1.0);
            let mut phase = 0.0_f64;
            for step in 0..128 {
                effect.set_param(0, if step % 2 == 0 { 20.0 } else { 20000.0 });
                let mut sweep = vec![0.0; block * 2];
                for frame in sweep.chunks_exact_mut(2) {
                    phase += (50.0 + step as f64 * 25.0) / rate as f64;
                    frame.fill((0.1 * (std::f64::consts::TAU * phase).sin()) as f32);
                }
                effect.process(&mut sweep, 2);
                assert!(sweep.iter().all(|sample| sample.is_finite() && sample.abs() < 100.0), "rate {rate}, block {block}, step {step}");
            }
        }
    }
}
