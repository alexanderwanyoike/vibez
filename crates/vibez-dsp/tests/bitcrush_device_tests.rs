//! Bitcrush host-buffer and parameter boundaries preserve audio and state.

use vibez_core::effect::EffectType;
use vibez_dsp::factory::create_effect_with_params;

#[test]
fn zero_channels_do_not_advance_and_wider_buses_preserve_extra_and_partial_frames() {
    let build = || create_effect_with_params(EffectType::Bitcrush, 48_000.0, &[2.0, 3.0, 1.0]);
    let mut effect = build();
    let mut untouched = [0.2, -0.2, 0.8, -0.8];
    effect.process(&mut untouched, 0);
    assert_eq!(untouched, [0.2, -0.2, 0.8, -0.8]);
    let mut after = [0.6, 0.8, -0.2, -0.9];
    let mut fresh = after;
    effect.process(&mut after, 1);
    build().process(&mut fresh, 1);
    assert_eq!(after, fresh);

    for channels in [1, 2, 3, 8] {
        for frames in [0, 1, 2, 7] {
            let trailing = usize::from(channels > 1);
            let mut input: Vec<_> = (0..frames * channels + trailing)
                .map(|index| index as f32 / 7.0 - 0.8)
                .collect();
            let mut expected = input.clone();
            let mut hold = [0.0_f32; 2];
            for frame in 0..frames {
                for (channel, held) in hold.iter_mut().enumerate().take(channels.min(2)) {
                    let position = frame * channels + channel;
                    if frame % 3 == 0 {
                        *held = ((input[position] * 2.0).round() * 0.5).clamp(-1.0, 1.0);
                    }
                    expected[position] = *held;
                }
            }
            build().process(&mut input, channels);
            assert_eq!(input, expected, "channels={channels}, frames={frames}");
        }
    }
}

#[test]
fn nonfinite_parameter_edits_leave_targets_and_acquired_audio_unchanged() {
    let parameters = [4.5, 5.49, 0.37];
    let build = || create_effect_with_params(EffectType::Bitcrush, 48_000.0, &parameters);
    let mut effect = build();
    let mut control = build();
    let warm: Vec<_> = (0..34).map(|sample| sample as f32 / 47.0 - 0.31).collect();
    effect.process(&mut warm.clone(), 2);
    control.process(&mut warm.clone(), 2);
    for index in 0..parameters.len() {
        for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert!(
                !effect.set_param(index, value),
                "index={index}, value={value}"
            );
            for (index, expected) in parameters.into_iter().enumerate() {
                assert_eq!(effect.get_param(index), expected);
            }
        }
    }
    assert!(!effect.set_param(99, 0.5));
    let mut actual: Vec<_> = (0..90).map(|sample| 0.63 - sample as f32 / 73.0).collect();
    let mut expected = actual.clone();
    effect.process(&mut actual, 2);
    control.process(&mut expected, 2);
    assert_eq!(actual, expected);
}

#[test]
fn legacy_values_append_only_the_six_neutral_bitcrush_controls() {
    let mut effect =
        create_effect_with_params(EffectType::Bitcrush, 48_000.0, &[10.25, 3.51, 0.37]);
    let descriptors = effect.param_descriptors();
    assert_eq!(descriptors.len(), 9);
    let expected = [
        ("Bits", 2.0, 16.0, 10.0, "bit"),
        ("SR Reduce", 1.0, 32.0, 1.0, "x"),
        ("Mix", 0.0, 1.0, 1.0, ""),
        ("Drive", 0.0, 24.0, 0.0, "dB"),
        ("Rounding", 0.0, 2.0, 0.0, ""),
        ("Dither", 0.0, 1.0, 0.0, ""),
        ("Jitter", 0.0, 1.0, 0.0, ""),
        ("Post LP Enabled", 0.0, 1.0, 0.0, ""),
        ("Post LP Frequency", 20.0, 20000.0, 12000.0, "Hz"),
    ];
    for (index, (&expected, actual)) in expected.iter().zip(descriptors).enumerate() {
        assert_eq!(
            (
                actual.name,
                actual.min,
                actual.max,
                actual.default,
                actual.unit
            ),
            expected
        );
        if index >= 3 {
            assert_eq!(effect.get_param(index), actual.default);
            assert!(effect.set_param(index, actual.max));
            assert_eq!(effect.get_param(index), actual.max);
            assert!(!effect.set_param(index, f32::NAN));
            assert_eq!(effect.get_param(index), actual.max);
        }
    }
    assert_eq!(effect.get_param(0), 10.25);
    assert_eq!(effect.get_param(1), 3.51);
    assert_eq!(effect.get_param(2), 0.37);
    assert!(!effect.set_param(9, 1.0));
}
