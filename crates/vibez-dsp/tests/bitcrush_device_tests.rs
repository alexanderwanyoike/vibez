//! Bitcrush respects the host's actual interleaved channel layout.

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
