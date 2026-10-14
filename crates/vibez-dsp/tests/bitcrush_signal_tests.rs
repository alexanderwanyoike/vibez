//! Bitcrush signal-stage placement preserves the raw dry branch and held wet audio.

use vibez_core::effect::EffectType;
use vibez_dsp::factory::create_effect_with_params;

#[test]
fn drive_changes_only_the_wet_signal_before_quantization_and_acquisition() {
    for rate in [44_100.0, 48_000.0, 96_000.0] {
        for channels in [1, 2] {
            for drive_db in [6.0, 12.0, 24.0] {
                for divider in [1, 3, 7] {
                    for mix in [0.0, 0.37, 1.0] {
                        let input: Vec<_> = (0..35)
                            .flat_map(|frame| {
                                (0..channels).map(move |channel| {
                                    let dry = match frame {
                                        0 => 1.75,
                                        1 => -2.5,
                                        2 => f32::MAX,
                                        _ => frame as f32 * 0.017 - 0.23,
                                    };
                                    if channel == 0 {
                                        dry
                                    } else {
                                        -dry * 0.37
                                    }
                                })
                            })
                            .collect();
                        let mut expected = input.clone();
                        let gain = 10.0_f64.powf(f64::from(drive_db) / 20.0);
                        let mut held = [0.0_f32; 2];
                        for frame in 0..35 {
                            for channel in 0..channels {
                                let index = frame * channels + channel;
                                if frame % divider == 0 {
                                    let grid_position = f64::from(input[index]) * gain * 32.0;
                                    held[channel] =
                                        (grid_position.round() / 32.0).clamp(-1.0, 1.0) as f32;
                                }
                                expected[index] = input[index] * (1.0 - mix) + held[channel] * mix;
                            }
                        }
                        let parameters = [6.0, divider as f32, mix, drive_db];
                        let mut effect =
                            create_effect_with_params(EffectType::Bitcrush, rate, &parameters);
                        let mut actual = input.clone();
                        let mut position = 0;
                        for frames in [1, 2, 8, 3, 21] {
                            let end = position + frames * channels;
                            effect.process(&mut actual[position..end], channels);
                            position = end;
                        }
                        assert_eq!(actual, expected,
                            "rate={rate}, channels={channels}, Drive={drive_db}, N={divider}, Mix={mix}");
                        if mix == 0.0 {
                            assert_eq!(actual, input);
                        }
                    }
                }
            }
        }
    }
}
