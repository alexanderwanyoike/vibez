//! Public Drive behavior stays aligned, controllable and observable without callback allocation.

use vibez_core::effect::EffectType;
use vibez_dsp::factory::create_effect_with_params;

#[test]
fn legacy_and_dry_mix_have_the_same_declared_exact_64_frame_delay() {
    for controls in [[0.0, 0.5, 1.0, 1.0], [1.0, 1.0, 0.7, 0.0]] {
        let mut effect = create_effect_with_params(EffectType::Drive, 48_000.0, &controls);
        assert_eq!(effect.latency_samples(), 64);
        let mut impulse = [0.0; 260];
        impulse[0] = 0.5;
        impulse[1] = -0.25;
        effect.process(&mut impulse, 2);
        for (frame, pair) in impulse.as_chunks::<2>().0.iter().enumerate() {
            let expected = if frame == 64 { [0.5 * controls[2], -0.25 * controls[2]] } else { [0.0; 2] };
            assert_eq!(*pair, expected, "controls={controls:?}, frame={frame}");
        }
    }
}

#[test]
fn wider_buses_keep_their_real_stride_and_zero_channels_do_not_consume_audio() {
    for channels in [1, 2, 3, 5] {
        let mut effect = create_effect_with_params(EffectType::Drive, 48_000.0, &[0.0, 0.5, 1.0, 1.0]);
        let mut input: Vec<_> = (0..150).flat_map(|frame| (0..channels).map(move |channel| 0.01 * frame as f32 + channel as f32)).collect();
        input.push(0.123);
        let original = input.clone();
        effect.process(&mut input, channels);
        for frame in 0..original.len() / channels {
            for channel in 0..channels {
                let expected = if channel >= 2 { original[frame * channels + channel] } else if frame < 64 { 0.0 } else { original[(frame - 64) * channels + channel] };
                assert_eq!(input[frame * channels + channel], expected, "channels={channels}, frame={frame}, channel={channel}");
            }
        }
        if channels > 1 { assert_eq!(input.last(), original.last()); }
    }
    let mut effect = create_effect_with_params(EffectType::Drive, 48_000.0, &[0.0, 0.5, 1.0, 1.0]);
    let mut untouched = [0.2; 300];
    effect.process(&mut untouched, 0);
    assert_eq!(untouched, [0.2; 300]);
    let mut impulse = [0.0; 260];
    impulse[0] = 0.7;
    effect.process(&mut impulse, 2);
    assert_eq!(impulse[128], 0.7);
}

#[test]
fn legacy_project_values_keep_their_units_and_append_neutral_character_controls() {
    let mut effect = create_effect_with_params(EffectType::Drive, 48_000.0, &[0.73, 0.27, 1.4, 0.31]);
    let descriptors = effect.param_descriptors();
    assert_eq!(descriptors.len(), 10);
    for (index, (name, min, max, default, unit)) in [
        ("Amount", 0.0, 1.0, 0.35, ""), ("Tone", 0.0, 1.0, 0.5, ""),
        ("Output", 0.0, 2.0, 1.0, "x"), ("Mix", 0.0, 1.0, 1.0, ""),
        ("Character", 0.0, 3.0, 0.0, ""), ("Quality", 0.0, 2.0, 2.0, ""),
        ("Pre HP Enabled", 0.0, 1.0, 0.0, ""), ("Pre HP Frequency", 20.0, 1000.0, 80.0, "Hz"),
        ("Post LP Enabled", 0.0, 1.0, 0.0, ""), ("Post LP Frequency", 1000.0, 20000.0, 12000.0, "Hz"),
    ].into_iter().enumerate() {
        let actual = &descriptors[index];
        assert_eq!((actual.name, actual.min, actual.max, actual.default, actual.unit), (name, min, max, default, unit));
        if index >= 4 { assert_eq!(effect.get_param(index), default); }
    }
    for (index, expected) in [0.73, 0.27, 1.4, 0.31].into_iter().enumerate() {
        assert_eq!(effect.get_param(index), expected);
    }
    for index in 4..10 {
        assert!(effect.set_param(index, descriptors[index].max));
        assert_eq!(effect.get_param(index), descriptors[index].max);
    }
    assert!(!effect.set_param(10, 0.5));
}

#[test]
fn waveshaper_diagnostics_default_to_unavailable_instead_of_invented_silence() {
    use vibez_core::effect::WaveShaperMeter;
    let empty = WaveShaperMeter::default();
    assert_eq!(empty.count, 0);
    assert_eq!(empty.input, [0.0; 128]);
    assert_eq!(empty.output, [0.0; 128]);
    assert_eq!(empty.transfer, [0.0; 65]);
    assert!(create_effect_with_params(EffectType::Gain, 48_000.0, &[]).take_waveshaper_meter().is_none());
}

#[test]
fn each_modern_character_has_its_declared_curve_and_retains_amount_mix_output_blends() {
    for rate in [44_100.0, 48_000.0, 96_000.0] {
        for character in 1..=3 {
            for (input, amount, output, mix) in [(0.02_f32, 1.0_f32, 1.0_f32, 1.0_f32), (-0.08, 0.4, 1.4, 0.37)] {
                let gain = 1.0 + 29.0 * amount * amount;
                let u = (gain * input).clamp(-1.0, 1.0);
                let shaped = match character { 1 => (gain * input).tanh() / gain.tanh(), 2 => 1.5 * u - 0.5 * u.powi(3), _ => u };
                let expected = output * ((1.0 - mix) * input + mix * ((1.0 - amount) * input + amount * shaped));
                let mut effect = create_effect_with_params(EffectType::Drive, rate, &[amount, 0.5, output, mix, character as f32]);
                let mut dc = [input; 512];
                effect.process(&mut dc, 1);
                assert!(dc[480..].iter().all(|&sample| (sample - expected).abs() < 5e-5), "rate={rate}, character={character}, actual={}, expected={expected}",dc[511]);
                assert_eq!(effect.latency_samples(), 64);
            }
        }
    }
}

fn bin_amplitude(signal: &[f32], bin: usize) -> f64 {
    let (mut real, mut imaginary) = (0.0, 0.0);
    for (index, &sample) in signal.iter().enumerate() {
        let phase = std::f64::consts::TAU * bin as f64 * index as f64 / signal.len() as f64;
        real += sample as f64 * phase.cos();
        imaginary += sample as f64 * phase.sin();
    }
    2.0 * real.hypot(imaginary) / signal.len() as f64
}

#[test]
fn default_eight_times_quality_removes_the_folded_third_harmonic_of_hard_drive() {
    let period = 1024;
    let cycles = 235;
    let input: Vec<f32> = (0..period * 3).map(|frame| (0.5 * (std::f64::consts::TAU * cycles as f64 * frame as f64 / period as f64).sin()) as f32).collect();
    let naive: Vec<_> = input[period..period * 2].iter().map(|&sample| (sample * 30.0).clamp(-1.0, 1.0)).collect();
    let mut effect = create_effect_with_params(EffectType::Drive, 48_000.0, &[1.0, 0.5, 1.0, 1.0, 3.0]);
    let mut actual = input;
    effect.process(&mut actual, 1);
    let folded = period - 3 * cycles;
    let baseline = bin_amplitude(&naive, folded);
    let alias = bin_amplitude(&actual[period + 64..period * 2 + 64], folded);
    assert!(baseline > 0.1);
    assert!(alias <= baseline * 0.1, "folded harmonic amplitude {alias} vs naive {baseline}");
}

#[test]
fn drive_reports_actual_aligned_left_samples_and_a_memoryless_applied_transfer() {
    let mut effect = create_effect_with_params(EffectType::Drive, 48_000.0, &[0.0, 0.5, 1.0, 1.0]);
    let empty = effect.take_waveshaper_meter().expect("native Drive declares its diagnostic capability");
    assert_eq!(empty.count, 0);
    assert_eq!(empty.sample_rate, 48_000.0);
    assert_eq!(empty.latency_samples, 64);
    let mut signal: Vec<_> = (0..257).flat_map(|frame| [frame as f32 / 1000.0, -0.7]).collect();
    effect.process(&mut signal[..14], 2);
    effect.process(&mut signal[14..], 2);
    let meter = effect.take_waveshaper_meter().unwrap();
    assert_eq!(meter.count, 128);
    assert_eq!(meter.first_output_sample, 129);
    for point in 0..128 {
        let expected = (129 + point - 64) as f32 / 1000.0;
        assert_eq!(meter.input[point], expected);
        assert_eq!(meter.output[point], expected);
        assert_eq!(meter.output[point], signal[(129 + point) * 2]);
    }
    for point in 0..65 { assert_eq!(meter.transfer[point], -1.0 + point as f32 / 32.0); }
    assert!(effect.supports_level_meter());
    assert_eq!(effect.take_waveshaper_meter().unwrap().count, 0);
    effect.reset();
    let reset = effect.take_waveshaper_meter().unwrap();
    assert_eq!(reset.count, 0);
    assert_eq!(reset.first_output_sample, 0);
    let mut silence = [0.0; 16];
    effect.process(&mut silence, 2);
    let silent = effect.take_waveshaper_meter().unwrap();
    assert_eq!(silent.count, 8);
    assert_eq!(silent.input[..8], [0.0; 8]);
    assert_eq!(silent.output[..8], [0.0; 8]);
}

#[test]
fn manual_output_moves_in_512_host_frames_while_automation_is_sample_exact() {
    for partitions in [&[600][..], &[1, 7, 61, 128, 3][..]] {
        let render = |automated| {
            let mut effect = create_effect_with_params(EffectType::Drive, 48_000.0, &[0.0, 0.5, 1.0, 1.0]);
            effect.process(&mut [0.5; 128], 1);
            if automated { assert!(effect.set_automated_param(2, 2.0)); } else { assert!(effect.set_param(2, 2.0)); }
            assert_eq!(effect.get_param(2), 2.0);
            let mut output = vec![0.5; 600];
            let (mut frame, mut part) = (0, 0);
            while frame < output.len() {
                let end = (frame + partitions[part % partitions.len()]).min(output.len());
                effect.process(&mut output[frame..end], 1);
                frame = end;
                part += 1;
            }
            output
        };
        let manual = render(false);
        let automated = render(true);
        for frame in 0..600 {
            let expected = 0.5 * (1.0 + ((frame + 1).min(512)) as f32 / 512.0);
            assert_eq!(manual[frame], expected, "manual frame={frame}, partitions={partitions:?}");
            assert_eq!(automated[frame], 1.0, "automation frame={frame}");
        }
    }
}
