//! Control changes follow source audio through the declared FIR latency.

use vibez_core::effect::EffectType;
use vibez_dsp::factory::create_effect_with_params;

#[test]
fn modern_tone_and_post_filter_automation_wait_for_their_source_audio() {
    let mut tone = create_effect_with_params(EffectType::Drive, 48_000.0, &[1.0, 0.5, 1.0, 1.0, 1.0]);
    let mut warm = [0.02; 4096];
    tone.process(&mut warm, 1);
    let neutral = warm[4095];
    assert!(tone.set_automated_param(1, 1.0));
    let mut output = [0.02; 128];
    tone.process(&mut output, 1);
    for (frame, actual) in output.into_iter().enumerate() {
        let expected = neutral * if frame < 64 { 1.0 } else { 10.0_f32.powf(-6.0 / 20.0) };
        assert!((actual - expected).abs() < 2e-6, "Tone frame={frame}: {actual} != {expected}");
    }
    let mut filter = create_effect_with_params(EffectType::Drive, 48_000.0, &[1.0, 0.5, 1.0, 1.0, 0.0, 2.0, 0.0, 80.0, 0.0, 1000.0]);
    let mut warm: Vec<_> = (0..4096).map(|n| if n % 2 == 0 { 0.02 } else { -0.02 }).collect();
    filter.process(&mut warm, 1);
    assert!(filter.set_automated_param(8, 1.0));
    let mut output: Vec<_> = (0..128).map(|n| if n % 2 == 0 { 0.02 } else { -0.02 }).collect();
    filter.process(&mut output, 1);
    for (frame, actual) in output.into_iter().enumerate() {
        let input = if frame % 2 == 0 { 0.6_f32 } else { -0.6 };
        let expected = if frame < 64 { input.tanh() } else { 0.0 };
        assert!((actual - expected).abs() < 2e-6, "Post LP frame={frame}: {actual} != {expected}");
    }
}

fn reference_kernel(factor: usize) -> Vec<f64> {
    let i0 = |x: f64| {
        let (mut sum, mut term) = (1.0, 1.0);
        for order in 1..100 { term *= (x / (2 * order) as f64).powi(2); sum += term; }
        sum
    };
    let mut kernel: Vec<_> = (0..=64 * factor).map(|index| {
        let position = index as f64 - (32 * factor) as f64;
        let angle = std::f64::consts::PI * 0.9 * position / factor as f64;
        let sinc = if position == 0.0 { 1.0 } else { angle.sin() / angle };
        let window = i0(8.6 * (1.0 - (position / (32 * factor) as f64).powi(2)).max(0.0).sqrt()) / i0(8.6);
        sinc * window * 0.9 / factor as f64
    }).collect();
    let dc: f64 = kernel.iter().sum();
    for tap in &mut kernel { *tap /= dc; }
    kernel
}

#[test]
fn modern_amount_automation_matches_independent_center_aligned_fir_convolution() {
    let frames = 768;
    let input: Vec<_> = (0..frames).map(|n| (0.12 * (n as f64 * 0.071).sin() + 0.02) as f32).collect();
    for (quality, factor) in [2, 4, 8].into_iter().enumerate() {
        let kernel = reference_kernel(factor);
        let mut shaped = vec![0.0; frames * factor];
        for (high_frame, value) in shaped.iter_mut().enumerate() {
            let interpolated: f64 = kernel.iter().enumerate().filter_map(|(tap, coefficient)| {
                let source = high_frame.checked_sub(tap)?;
                (source % factor == 0).then(|| input[source / factor] as f64 * coefficient * factor as f64)
            }).sum();
            let input_frame = (high_frame / factor).saturating_sub(32);
            let amount = if input_frame < 256 { 0.2_f32 } else { 0.8 };
            let gain = (1.0 + amount * amount * 29.0) as f64;
            *value = (interpolated * gain).tanh() / gain.tanh();
        }
        let mut effect = create_effect_with_params(EffectType::Drive, 48_000.0, &[0.2, 0.5, 1.0, 1.0, 1.0, quality as f32]);
        let mut actual = input.clone();
        effect.process(&mut actual[..256], 1);
        assert!(effect.set_automated_param(0, 0.8));
        effect.process(&mut actual[256..], 1);
        for (frame, actual) in actual.into_iter().enumerate() {
            let wet: f64 = kernel.iter().enumerate().filter_map(|(tap, coefficient)| (frame * factor).checked_sub(tap).map(|source| shaped[source] * coefficient)).sum();
            let source = frame.checked_sub(64);
            let dry = source.map_or(0.0, |source| input[source] as f64);
            let amount = if source.unwrap_or(0) < 256 { 0.2_f32 } else { 0.8 } as f64;
            let expected = dry * (1.0 - amount) + wet * amount;
            assert!((actual as f64 - expected).abs() < 3e-6, "factor={factor}, frame={frame}: {actual} != {expected}");
        }
    }
}

#[test]
fn manual_character_changes_crossfade_only_the_requested_warm_characters() {
    let signal: Vec<_> = (0..1280).map(|frame| (0.13 * (frame as f32 * 0.17).sin()) + 0.01).collect();
    for (from, to) in [(0, 3), (3, 1), (1, 2), (2, 0)] {
        let build = |character| create_effect_with_params(EffectType::Drive, 48_000.0, &[0.7, 0.5, 1.0, 1.0, character as f32]);
        let mut moving = build(from);
        let mut first = build(from);
        let mut last = build(to);
        for effect in [&mut moving, &mut first, &mut last] { effect.process(&mut signal[..640].to_vec(), 1); }
        assert!(moving.set_param(4, to as f32));
        let mut actual = signal[640..].to_vec();
        let mut before = actual.clone();
        let mut after = actual.clone();
        first.process(&mut before, 1);
        last.process(&mut after, 1);
        moving.process(&mut actual[..128], 1);
        let meter = moving.take_waveshaper_meter().unwrap();
        let x = 1.0_f32 / 32.0;
        let gain = 1.0_f32 + 0.7 * 0.7 * 29.0;
        let shape = |character| {
            let u = (gain * x).clamp(-1.0, 1.0);
            let saturated = match character { 2 => 1.5 * u - 0.5 * u * u * u, 3 => u, _ => (gain * x).tanh() / gain.tanh() };
            0.3 * x + 0.7 * saturated
        };
        let weight = 64.0 / 512.0;
        assert!((meter.transfer[33] - ((1.0 - weight) * shape(from) + weight * shape(to))).abs() < 2e-6, "curve from={from}, to={to}");
        assert!(moving.set_param(4, to as f32));
        moving.process(&mut actual[128..], 1);
        for (frame, actual) in actual.into_iter().enumerate() {
            let weight = (frame + 1).saturating_sub(64).min(512) as f32 / 512.0;
            let expected = before[frame] * (1.0 - weight) + after[frame] * weight;
            assert!((actual - expected).abs() < 3e-6, "from={from}, to={to}, frame={frame}: {actual} != {expected}");
        }
    }
}

#[test]
fn quality_switches_mix_only_aligned_warm_paths_and_automation_does_not_ramp() {
    let input: Vec<_> = (0..1280).map(|frame| (0.2 * (frame as f32 * 2.47).sin()) + 0.01).collect();
    for (from, to) in [(0, 2), (2, 1), (1, 0)] {
        for automated in [false, true] {
            let build = |quality| create_effect_with_params(EffectType::Drive, 48_000.0, &[1.0, 0.5, 1.0, 1.0, 3.0, quality as f32]);
            let mut moving = build(from);
            let mut first = build(from);
            let mut last = build(to);
            for effect in [&mut moving, &mut first, &mut last] { effect.process(&mut input[..640].to_vec(), 1); }
            if automated { assert!(moving.set_automated_param(5, to as f32)); } else { assert!(moving.set_param(5, to as f32)); }
            let mut actual = input[640..].to_vec();
            let mut before = actual.clone();
            let mut after = actual.clone();
            first.process(&mut before, 1);
            last.process(&mut after, 1);
            for chunk in actual.chunks_mut(31) { moving.process(chunk, 1); }
            for (frame, actual) in actual.into_iter().enumerate() {
                let weight = if automated { f32::from(frame >= 64) } else { (frame + 1).saturating_sub(64).min(512) as f32 / 512.0 };
                let expected = before[frame] * (1.0 - weight) + after[frame] * weight;
                assert!((actual - expected).abs() < 3e-6, "quality {from}->{to}, automated={automated}, frame={frame}: {actual} != {expected}");
            }
        }
    }
}

#[test]
fn exact_automation_on_other_controls_preserves_an_unfinished_manual_ramp() {
    let mut effect = create_effect_with_params(EffectType::Drive, 48_000.0, &[0.0, 0.5, 1.0, 1.0]);
    effect.process(&mut [0.5; 128], 1);
    assert!(effect.set_param(2, 2.0));
    let mut output = [0.5; 640];
    effect.process(&mut output[..128], 1);
    assert!(effect.set_param(2, 2.0));
    for (index, value) in [(4, 3.0), (5, 0.0), (1, 1.0), (3, 1.0)] { assert!(effect.set_automated_param(index, value)); }
    effect.process(&mut output[128..], 1);
    for (frame, actual) in output.into_iter().enumerate() {
        let expected = 0.5 * (1.0 + (frame + 1).saturating_sub(64).min(512) as f32 / 512.0);
        assert_eq!(actual, expected, "frame={frame}");
    }
}
