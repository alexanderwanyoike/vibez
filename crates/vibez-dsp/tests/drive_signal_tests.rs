//! Real stereo signal families retain state across callback boundaries and reset.

use vibez_core::effect::EffectType;
use vibez_dsp::factory::create_effect_with_params;

#[test]
fn silence_impulses_tones_sweeps_noise_and_dc_are_partition_invariant_with_live_controls() {
    for rate in [44_100.0, 48_000.0, 96_000.0] {
        for family in 0..6 {
            let mut random = 7_u32;
            let signal: Vec<_> = (0..1024)
                .flat_map(|frame| {
                    random = random.wrapping_mul(1664525).wrapping_add(1013904223);
                    let sample = match family {
                        0 => 0.0,
                        1 => {
                            if [0, 319, 1023].contains(&frame) {
                                0.8
                            } else {
                                0.0
                            }
                        }
                        2 => 0.31 * (std::f32::consts::TAU * 997.0 * frame as f32 / rate).sin(),
                        3 => {
                            0.29 * (std::f32::consts::PI * 0.45 * frame as f32 * frame as f32
                                / 1024.0)
                                .sin()
                        }
                        4 => (random as f64 / u32::MAX as f64 * 0.6 - 0.3) as f32,
                        _ => {
                            if frame < 512 {
                                0.03
                            } else {
                                f32::MIN_POSITIVE
                            }
                        }
                    };
                    [sample, -sample * 0.37]
                })
                .collect();
            for character in 0..4 {
                let render = |parts: &[usize]| {
                    let initial = [
                        0.43,
                        0.27,
                        1.4,
                        0.71,
                        character as f32,
                        0.0,
                        0.0,
                        80.0,
                        0.0,
                        12000.0,
                    ];
                    let mut effect = create_effect_with_params(EffectType::Drive, rate, &initial);
                    let mut actual = signal.clone();
                    let (mut frame, mut part) = (0, 0);
                    while frame < 1024 {
                        if frame == 320 {
                            for (index, value) in [
                                0.81,
                                0.91,
                                0.7,
                                0.37,
                                ((character + 2) % 4) as f32,
                                2.0,
                                1.0,
                                700.0,
                                1.0,
                                2300.0,
                            ]
                            .into_iter()
                            .enumerate()
                            {
                                assert!(effect.set_param(index, value));
                            }
                        }
                        if frame == 512 {
                            assert!(effect.set_automated_param(9, 5300.0));
                        }
                        let boundary = if frame < 320 {
                            320
                        } else if frame < 512 {
                            512
                        } else {
                            1024
                        };
                        let end = (frame + parts[part % parts.len()]).min(boundary);
                        effect.process(&mut actual[frame * 2..end * 2], 2);
                        frame = end;
                        part += 1;
                    }
                    assert!(actual.iter().all(|value| value.is_finite()));
                    let targets: Vec<_> = (0..10).map(|index| effect.get_param(index)).collect();
                    effect.reset();
                    let mut replay = signal.clone();
                    effect.process(&mut replay, 2);
                    let mut fresh = create_effect_with_params(EffectType::Drive, rate, &targets);
                    let mut expected = signal.clone();
                    fresh.process(&mut expected, 2);
                    assert_eq!(
                        replay, expected,
                        "reset rate={rate}, family={family}, character={character}"
                    );
                    actual
                };
                let whole = render(&[1024]);
                let divided = render(&[1, 31, 127, 64, 7]);
                assert_eq!(
                    whole, divided,
                    "rate={rate}, family={family}, character={character}"
                );
                if family == 0 {
                    assert!(whole.iter().all(|&value| value == 0.0));
                }
            }
        }
    }
}
