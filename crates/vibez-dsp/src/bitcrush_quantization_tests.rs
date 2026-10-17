//! Audible quantizer modes are checked against independent finite-grid code tables.

use crate::factory::create_effect_with_params;
use vibez_core::effect::EffectType;

#[test]
fn rounding_modes_quantize_only_acquired_frames_and_keep_inclusive_rails() {
    let input = [
        -2.0, -1.0, -0.76, -0.75, -0.51, -0.5, -0.25, 0.0, 0.25, 0.5, 0.51, 0.75, 0.76, 1.0, 2.0,
    ];
    let code_tables = [
        [
            -1.0, -1.0, -1.0, -1.0, -0.5, -0.5, -0.5, 0.0, 0.5, 0.5, 0.5, 1.0, 1.0, 1.0, 1.0,
        ],
        [
            -1.0, -1.0, -1.0, -1.0, -1.0, -0.5, -0.5, 0.0, 0.0, 0.5, 0.5, 0.5, 0.5, 1.0, 1.0,
        ],
        [
            -1.0, -1.0, -0.5, -0.5, -0.5, -0.5, 0.0, 0.0, 0.0, 0.5, 0.5, 0.5, 0.5, 1.0, 1.0,
        ],
    ];
    for (mode, codes) in code_tables.iter().enumerate() {
        for channels in [1, 2] {
            for divider in [1, 3, 5] {
                for offset in 0..input.len() {
                    let parameters = [2.0, divider as f32, 1.0, 0.0, mode as f32];
                    let mut effect =
                        create_effect_with_params(EffectType::Bitcrush, 48_000.0, &parameters);
                    let mut audio = Vec::new();
                    let mut expected = Vec::new();
                    for frame in 0..45 {
                        let acquired = frame - frame % divider;
                        for channel in 0..channels {
                            audio.push(input[(frame + offset + channel * 7) % input.len()]);
                            expected.push(codes[(acquired + offset + channel * 7) % input.len()]);
                        }
                    }
                    let mut position = 0;
                    for frames in [1, 7, 2, 11, 24] {
                        let end = position + frames * channels;
                        effect.process(&mut audio[position..end], channels);
                        position = end;
                    }
                    assert_eq!(
                        audio, expected,
                        "mode={mode}, channels={channels}, divider={divider}, offset={offset}"
                    );
                }
            }
        }
    }
}
