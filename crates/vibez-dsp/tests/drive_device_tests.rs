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
