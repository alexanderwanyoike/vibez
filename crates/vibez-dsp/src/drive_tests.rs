use super::*;

const RATE: f32 = 48_000.0;

fn sine(frequency: f32, amplitude: f32, frames: usize) -> Vec<f32> {
    (0..frames)
        .flat_map(|frame| {
            let value = amplitude * (std::f32::consts::TAU * frequency * frame as f32 / RATE).sin();
            [value, value]
        })
        .collect()
}

fn drive(edits: &[(usize, f32)]) -> DriveEffect {
    let mut drive = DriveEffect::new(RATE);
    for &(index, value) in edits {
        drive.set_param(index, value);
    }
    drive
}

/// Renders in uneven blocks, as the engine's callbacks arrive.
fn render(effect: &mut DriveEffect, input: &[f32]) -> Vec<f32> {
    let mut output = input.to_vec();
    let mut start = 0;
    for (block, size) in [64usize, 7, 128, 1, 300].iter().cycle().enumerate() {
        if start >= output.len() / 2 {
            break;
        }
        let end = (start + size + block % 3).min(output.len() / 2);
        effect.process(&mut output[start * 2..end * 2], 2);
        start = end;
    }
    output
}

fn left(samples: &[f32]) -> Vec<f32> {
    samples.iter().step_by(2).copied().collect()
}

/// Level of `frequency` in `signal`, by correlation with a sine and cosine.
fn level_db(signal: &[f32], frequency: f32) -> f32 {
    let (mut re, mut im) = (0.0f64, 0.0f64);
    for (n, sample) in signal.iter().enumerate() {
        let phase = std::f64::consts::TAU * frequency as f64 * n as f64 / RATE as f64;
        re += *sample as f64 * phase.cos();
        im += *sample as f64 * phase.sin();
    }
    let amplitude = 2.0 * (re * re + im * im).sqrt() / signal.len() as f64;
    20.0 * (amplitude.max(1e-12)).log10() as f32
}

/// The original four-control Drive, before characters and oversampling.
fn original(input: &[f32], [amount, tone, output, mix]: [f32; 4]) -> Vec<f32> {
    let gain = 1.0 + amount * amount * 29.0;
    let normalizer = gain.tanh();
    let mut previous = [0.0; 2];
    input
        .iter()
        .enumerate()
        .map(|(index, &dry)| {
            let channel = index % 2;
            let shaped = (dry * gain).tanh() / normalizer;
            let tilted = shaped + (2.0 * tone - 1.0) * (shaped - previous[channel]);
            previous[channel] = shaped;
            let wet = dry * (1.0 - amount) + tilted * amount;
            (dry * (1.0 - mix) + wet * mix) * output
        })
        .collect()
}

#[test]
fn four_value_projects_sound_as_they_did_after_the_reported_delay() {
    let controls = [0.7, 0.8, 0.9, 0.6];
    let input = sine(220.0, 0.8, 4096);
    let mut effect = drive(&[]);
    for (index, value) in controls.into_iter().enumerate() {
        effect.set_param(index, value);
    }
    let output = render(&mut effect, &input);
    let expected = original(&input, controls);
    let delay = LATENCY * 2;
    for (index, (got, want)) in output[delay..].iter().zip(&expected).enumerate() {
        assert!((got - want).abs() < 1e-6, "sample {index}: {got} vs {want}");
    }
    assert!(output[..delay].iter().all(|sample| *sample == 0.0));
}

#[test]
fn every_character_and_rate_lines_up_with_the_reported_delay() {
    let input = sine(300.0, 0.05, 6000);
    for character in 0..character::COUNT {
        for quality in 0..quality::COUNT {
            let mut effect = drive(&[
                (param::CHARACTER, character as f32),
                (param::QUALITY, quality as f32),
                (param::AMOUNT, 0.2),
            ]);
            let output = left(&render(&mut effect, &input));
            let input = left(&input);
            let correlation = |lag: usize| -> f32 {
                output[lag..]
                    .iter()
                    .zip(&input)
                    .skip(1000)
                    .map(|(a, b)| a * b)
                    .sum()
            };
            let best = (0..40).max_by(|a, b| correlation(*a).total_cmp(&correlation(*b)));
            assert_eq!(
                best,
                Some(LATENCY),
                "character {character} quality {quality}"
            );
            assert_eq!(effect.latency_samples(), LATENCY_SAMPLES);
        }
    }
}

#[test]
fn oversampling_keeps_hard_clipping_from_aliasing() {
    // 7 kHz clipped hard puts odd harmonics at 35, 49 and 63 kHz, which
    // fold back to 13, 1 and 15 kHz at the host rate.
    let input = sine(7_000.0, 0.5, 16_384);
    let alias = |quality: usize| {
        let mut effect = drive(&[
            (param::CHARACTER, character::HARD as f32),
            (param::QUALITY, quality as f32),
            (param::AMOUNT, 1.0),
        ]);
        let output = left(&render(&mut effect, &input));
        let tail = &output[4096..];
        [13_000.0, 1_000.0, 15_000.0]
            .map(|frequency| level_db(tail, frequency))
            .into_iter()
            .fold(f32::NEG_INFINITY, f32::max)
    };
    let (x1, x2, x4) = (alias(quality::X1), alias(quality::X2), alias(quality::X4));
    assert!(x1 > -40.0, "the test signal does alias at 1x: {x1} dB");
    assert!(x2 < x1 - 15.0, "2x {x2} dB against 1x {x1} dB");
    assert!(x4 < x1 - 30.0, "4x {x4} dB against 1x {x1} dB");
}

#[test]
fn switching_character_or_rate_mid_signal_never_clicks() {
    let input = sine(200.0, 0.5, 8192);
    let steady_step = |output: &[f32]| {
        output
            .windows(2)
            .map(|pair| (pair[1] - pair[0]).abs())
            .fold(0.0, f32::max)
    };
    let mut reference = drive(&[(param::CHARACTER, character::SOFT as f32)]);
    let limit = steady_step(&left(&render(&mut reference, &input))[2000..]) * 1.5;
    for (index, from, to) in [
        (param::CHARACTER, character::CLASSIC, character::HARD),
        (param::CHARACTER, character::SOFT, character::WARM),
        (param::QUALITY, quality::X1, quality::X4),
        (param::QUALITY, quality::X4, quality::X2),
    ] {
        let mut effect = drive(&[
            (param::CHARACTER, character::SOFT as f32),
            (index, from as f32),
        ]);
        let mut output = input.clone();
        effect.process(&mut output[..4000 * 2], 2);
        effect.set_param(index, to as f32);
        effect.process(&mut output[4000 * 2..], 2);
        let step = steady_step(&left(&output)[2000..]);
        assert!(
            step <= limit,
            "{index} {from}->{to}: step {step} limit {limit}"
        );
    }
}

#[test]
fn the_filters_trim_lows_before_and_highs_after_the_shaper() {
    let level = |edits: &[(usize, f32)], frequency: f32| {
        let mut effect = drive(edits);
        let output = left(&render(&mut effect, &sine(frequency, 0.3, 8192)));
        level_db(&output[4096..], frequency)
    };
    // At full drive the output is all shaped signal; below it, the dry
    // share keeps its lows, which is what a pre filter is for.
    let soft = (param::CHARACTER, character::SOFT as f32);
    let full = (param::AMOUNT, 1.0);
    let lows = level(&[soft, full], 40.0)
        - level(
            &[
                soft,
                full,
                (param::PRE_HP_ENABLED, 1.0),
                (param::PRE_HP_FREQUENCY, 400.0),
            ],
            40.0,
        );
    assert!(lows > 20.0, "a 400 Hz pre filter takes {lows} dB off 40 Hz");
    let highs = level(&[soft], 9_000.0)
        - level(
            &[
                soft,
                (param::POST_LP_ENABLED, 1.0),
                (param::POST_LP_FREQUENCY, 1_500.0),
            ],
            9_000.0,
        );
    assert!(
        highs > 20.0,
        "a 1.5 kHz post filter takes {highs} dB off 9 kHz"
    );
}

#[test]
fn mix_at_zero_is_the_dry_signal_delayed() {
    let input = sine(440.0, 0.4, 2048);
    let mut effect = drive(&[(param::AMOUNT, 1.0), (param::MIX, 0.0)]);
    let output = render(&mut effect, &input);
    for (got, want) in output[LATENCY * 2..].iter().zip(&input) {
        assert!((got - want).abs() < 1e-6);
    }
}

#[test]
fn bad_values_and_audio_cannot_poison_the_drive() {
    let mut effect = drive(&[(param::CHARACTER, character::SOFT as f32)]);
    assert!(!effect.set_param(param::AMOUNT, f32::NAN));
    assert!(!effect.set_param(param::COUNT, 0.5));
    effect.set_param(param::QUALITY, 9.0);
    assert_eq!(effect.get_param(param::QUALITY), quality::X4 as f32);
    let mut buffer = vec![f32::NAN, f32::INFINITY, 1e30, -1e30, 0.5, -0.5];
    buffer.extend(sine(100.0, 0.5, 256));
    effect.process(&mut buffer, 2);
    assert!(buffer.iter().all(|sample| sample.is_finite()));
}

#[test]
fn the_transfer_curve_follows_the_character() {
    let params = |character: usize| {
        let mut params: Vec<f32> = DRIVE_PARAMS.iter().map(|d| d.default).collect();
        params[param::AMOUNT] = 1.0;
        params[param::CHARACTER] = character as f32;
        params
    };
    assert_eq!(transfer(&params(character::HARD), 0.5), 1.0);
    assert!((transfer(&params(character::WARM), 0.5) - 1.0).abs() < 1e-6);
    assert!(transfer(&params(character::SOFT), 0.01) < 0.35);
    assert_eq!(transfer(&params(character::SOFT), 0.0), 0.0);
}
