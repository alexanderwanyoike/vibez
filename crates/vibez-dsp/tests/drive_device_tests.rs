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
