//! Standalone nonsubtractive TPDF follows independently frozen source-frame streams.

use vibez_core::effect::EffectType;
use vibez_dsp::factory::create_effect_with_params;

#[test]
fn standalone_tpdf_uses_frozen_frame_streams_on_signal_and_silence() {
    // A fixed independent code table keeps this oracle separate from production RNG math.
    let codes: [[i8; 2]; 32] = [
        [0, 0],
        [0, -1],
        [0, 0],
        [0, 0],
        [1, 0],
        [1, 0],
        [0, 0],
        [0, 0],
        [0, 0],
        [0, 0],
        [0, -1],
        [0, 0],
        [0, 0],
        [0, 0],
        [1, 0],
        [0, 1],
        [0, 0],
        [0, 0],
        [0, 0],
        [0, -1],
        [1, 0],
        [1, 1],
        [1, 0],
        [0, 0],
        [0, 0],
        [0, 1],
        [1, 0],
        [0, 0],
        [0, 0],
        [0, 0],
        [0, 0],
        [1, 0],
    ];
    let expected: Vec<_> = codes
        .into_iter()
        .flatten()
        .map(|code| f32::from(code) * 0.125)
        .collect();
    for parts in [&[32][..], &[1, 7, 3, 5, 16][..]] {
        let mut effect = create_effect_with_params(
            EffectType::Bitcrush,
            48_000.0,
            &[4.0, 1.0, 1.0, 0.0, 0.0, 1.0],
        );
        for repeat in 0..2 {
            let mut audio: Vec<_> = (0..32).flat_map(|_| [0.03125, 0.0]).collect();
            let mut position = 0;
            for &frames in parts {
                let end = position + frames * 2;
                effect.process(&mut audio[position..end], 2);
                position = end;
            }
            assert_eq!(audio, expected, "parts={parts:?}, reset_repeat={repeat}");
            effect.reset();
        }
    }
}
