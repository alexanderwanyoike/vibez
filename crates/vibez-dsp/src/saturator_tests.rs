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

fn defaults() -> Vec<f32> {
    SATURATOR_PARAMS.iter().map(|d| d.default).collect()
}

fn params(edits: &[(usize, f32)]) -> Vec<f32> {
    let mut params = defaults();
    for &(index, value) in edits {
        params[index] = value;
    }
    params
}

fn saturator(edits: &[(usize, f32)]) -> SaturatorEffect {
    let mut saturator = SaturatorEffect::new(RATE);
    for &(index, value) in edits {
        saturator.set_param(index, value);
    }
    saturator
}

/// Renders in uneven blocks, as the engine's callbacks arrive.
fn render(effect: &mut SaturatorEffect, input: &[f32]) -> Vec<f32> {
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

fn rms_db(signal: &[f32]) -> f32 {
    let power = signal.iter().map(|s| s * s).sum::<f32>() / signal.len() as f32;
    10.0 * power.max(1e-24).log10()
}

/// The left channel once the oversampling and filters have settled.
fn settled(effect: &mut SaturatorEffect, input: &[f32]) -> Vec<f32> {
    left(&render(effect, input))[4_800..].to_vec()
}

#[test]
fn silence_stays_silent_with_bias_removed() {
    let mut biased = saturator(&[(param::BIAS, 1.0), (param::DRIVE_DB, 24.0)]);
    let out = settled(&mut biased, &vec![0.0; 2 * 48_000]);
    let tail = &out[out.len() - 4_800..];
    assert!(
        tail.iter().all(|s| s.abs() < 1e-3),
        "{:?}",
        tail.iter().fold(0.0f32, |m, s| m.max(s.abs()))
    );
}

#[test]
fn bias_leaves_an_offset_when_dc_removal_is_off() {
    let mut biased = saturator(&[(param::BIAS, 1.0), (param::DC_REMOVAL, 0.0)]);
    let out = settled(&mut biased, &vec![0.0; 2 * 9_600]);
    let mean = out.iter().sum::<f32>() / out.len() as f32;
    assert!(mean > 0.1, "{mean}");
}

#[test]
fn every_curve_stays_within_its_reach() {
    for shape in [shape::SOFT, shape::TUBE, shape::FOLD] {
        for amount in [0.0, 0.5, 1.0] {
            for step in -400..=400 {
                let x = step as f32 / 10.0;
                let y = curve(shape, amount, [-17.0, -1.0, 6.0], x);
                assert!(
                    y.abs() <= 1.0 + 1e-6,
                    "shape {shape} amount {amount} at {x}: {y}"
                );
            }
        }
    }
    let loudest = db_to_gain(24.0);
    for step in -400..=400 {
        let x = step as f32 / 10.0;
        let y = curve(shape::CUSTOM, 0.0, [0.0, 12.0, 24.0], x);
        assert!(y.abs() <= loudest * 1.0001, "{x}: {y}");
    }
}

#[test]
fn curves_pass_through_zero_and_keep_the_sign() {
    for shape in [shape::SOFT, shape::TUBE, shape::FOLD, shape::CUSTOM] {
        assert_eq!(curve(shape, 0.75, [-17.0, -1.0, 6.0], 0.0), 0.0);
        let y = curve(shape, 0.75, [-17.0, -1.0, 6.0], 0.1);
        assert!(y > 0.0, "shape {shape}: {y}");
        let y = curve(shape, 0.75, [-17.0, -1.0, 6.0], -0.1);
        assert!(y < 0.0, "shape {shape}: {y}");
    }
}

#[test]
fn custom_meets_each_of_its_points() {
    let points = [-20.0, 3.0, 9.0];
    for (input, output) in CUSTOM_INPUT_DB.into_iter().zip(points) {
        let y = curve(shape::CUSTOM, 0.0, points, db_to_gain(input));
        assert!((20.0 * y.log10() - output).abs() < 1e-3, "{input} dB: {y}");
    }
    // Level beyond the last point, a straight line below the first.
    let beyond = curve(shape::CUSTOM, 0.0, points, db_to_gain(30.0));
    assert!((20.0 * beyond.log10() - 9.0).abs() < 1e-3);
    let below = curve(shape::CUSTOM, 0.0, points, db_to_gain(-40.0));
    assert!((20.0 * below.log10() - (-42.0)).abs() < 1e-3);
}

#[test]
fn symmetric_curves_make_odd_harmonics_and_leaning_makes_even_ones() {
    let driven = [(param::DRIVE_DB, 18.0)];
    for shape in [shape::SOFT, shape::FOLD] {
        let harmonics = harmonics_db(&params(&[driven[0], (param::SHAPE, shape as f32)]), 3);
        assert!(harmonics[1] < -80.0, "shape {shape}: {harmonics:?}");
        assert!(harmonics[2] > -30.0, "shape {shape}: {harmonics:?}");
    }
    let tube = harmonics_db(&params(&[driven[0], (param::SHAPE, shape::TUBE as f32)]), 2);
    assert!(tube[1] > -40.0, "{tube:?}");
    let biased = harmonics_db(&params(&[driven[0], (param::BIAS, 0.5)]), 2);
    assert!(biased[1] > -30.0, "{biased:?}");
    let asymmetric = harmonics_db(&params(&[driven[0], (param::ASYMMETRY, 0.6)]), 2);
    assert!(asymmetric[1] > -30.0, "{asymmetric:?}");
}

#[test]
fn more_drive_means_more_harmonics() {
    let third = |drive: f32| harmonics_db(&params(&[(param::DRIVE_DB, drive)]), 3)[2];
    assert!(third(0.0) < third(12.0) && third(12.0) < third(24.0));
}

#[test]
fn auto_gain_holds_the_level_as_drive_rises() {
    for shape in [shape::SOFT, shape::TUBE, shape::FOLD] {
        for drive in [0.0, 12.0, 24.0, 36.0] {
            let mut effect = saturator(&[
                (param::SHAPE, shape as f32),
                (param::DRIVE_DB, drive),
                (param::AUTO_GAIN, 1.0),
                (param::OUTPUT_DB, 0.0),
            ]);
            let input = sine(220.0, AUTO_GAIN_LEVEL, 24_000);
            let out = settled(&mut effect, &input);
            let difference = rms_db(&out) - rms_db(&left(&input));
            assert!(
                difference.abs() < 1.5,
                "shape {shape} drive {drive}: {difference}"
            );
        }
    }
}

#[test]
fn the_steady_transfer_is_what_the_audio_gets() {
    let edits = [
        (param::QUALITY, quality::X1 as f32),
        (param::PRE_FILTER, 0.0),
        (param::POST_FILTER, 0.0),
        (param::DC_REMOVAL, 0.0),
        (param::DRIVE_DB, 12.0),
        (param::BIAS, 0.3),
        (param::ASYMMETRY, -0.4),
        (param::MIX, 0.7),
        (param::SHAPE, shape::TUBE as f32),
    ];
    for input in [-0.8f32, -0.1, 0.0, 0.05, 0.5, 1.0] {
        let mut effect = saturator(&edits);
        let out = left(&render(&mut effect, &vec![input; 2 * 256]));
        let expected = transfer(&params(&edits), input);
        assert!(
            (out[255] - expected).abs() < 2e-3,
            "{input}: heard {} expected {expected}",
            out[255]
        );
    }
}

#[test]
fn every_rate_lines_up_with_the_dry_signal() {
    // A quiet sine barely touches the curve, so a half mix only cancels
    // if the paths are out of step.
    for rate in [quality::X1, quality::X2, quality::X4] {
        let mut effect = saturator(&[
            (param::QUALITY, rate as f32),
            (param::DRIVE_DB, 0.0),
            (param::OUTPUT_DB, 0.0),
            (param::MIX, 0.5),
            (param::POST_FILTER, 0.0),
        ]);
        let input = sine(5_000.0, 0.01, 24_000);
        let out = settled(&mut effect, &input);
        let difference = level_db(&out, 5_000.0) - level_db(&left(&input), 5_000.0);
        assert!(difference.abs() < 0.5, "rate {rate}: {difference}");
    }
}

#[test]
fn the_dry_signal_arrives_after_the_reported_latency() {
    let mut effect = saturator(&[(param::MIX, 0.0)]);
    let mut impulse = vec![0.0; 2 * 256];
    impulse[0] = 1.0;
    impulse[1] = 1.0;
    let out = left(&render(&mut effect, &impulse));
    let peak = out.iter().enumerate().fold((0, 0.0f32), |best, (i, s)| {
        if s.abs() > best.1 {
            (i, s.abs())
        } else {
            best
        }
    });
    assert_eq!(peak.0, LATENCY_SAMPLES as usize);
    assert_eq!(effect.latency_samples(), LATENCY_SAMPLES);
}

#[test]
fn oversampling_keeps_the_aliases_down() {
    // 7 kHz's fifth harmonic, 35 kHz, folds back to 13 kHz at 1x.
    let alias = |rate: usize| {
        let mut effect = saturator(&[
            (param::QUALITY, rate as f32),
            (param::DRIVE_DB, 24.0),
            (param::POST_FILTER, 0.0),
        ]);
        level_db(&settled(&mut effect, &sine(7_000.0, 0.5, 24_000)), 13_000.0)
    };
    let (x1, x4) = (alias(quality::X1), alias(quality::X4));
    assert!(x1 - x4 > 20.0, "1x {x1} 4x {x4}");
}

#[test]
fn the_pre_filter_keeps_lows_out_of_the_curve() {
    let low = |filter: f32| {
        let mut effect = saturator(&[(param::PRE_FILTER, filter)]);
        level_db(&settled(&mut effect, &sine(30.0, 0.5, 48_000)), 30.0)
    };
    // 160 Hz against off.
    assert!(low(0.0) - low(4.0) > 20.0);
}

#[test]
fn the_post_filter_takes_the_top_off() {
    let high = |filter: f32| {
        let mut effect = saturator(&[(param::POST_FILTER, filter), (param::DRIVE_DB, 0.0)]);
        level_db(
            &settled(&mut effect, &sine(12_000.0, 0.1, 24_000)),
            12_000.0,
        )
    };
    // 4 kHz against off.
    assert!(high(0.0) - high(5.0) > 20.0);
}

#[test]
fn edits_glide_rather_than_step() {
    for (index, value) in [
        (param::DRIVE_DB, 30.0),
        (param::SHAPE, shape::FOLD as f32),
        (param::CURVE, 0.0),
        (param::OUTPUT_DB, 12.0),
        (param::QUALITY, quality::X1 as f32),
        (param::CUSTOM_HIGH, 24.0),
    ] {
        let mut effect = saturator(&[(param::SHAPE, shape::CUSTOM as f32)]);
        let input = sine(100.0, 0.3, 9_600);
        let mut out = input.clone();
        let half = out.len() / 2;
        effect.process(&mut out[..half], 2);
        effect.set_param(index, value);
        effect.process(&mut out[half..], 2);
        let out = left(&out);
        let jump = out
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0f32, f32::max);
        // A 100 Hz wave moves at most a few hundredths per sample even at
        // full level; a step would move far more.
        assert!(jump < 0.15, "param {index}: {jump}");
    }
}

#[test]
fn choices_snap_and_ranges_clamp() {
    let mut effect = SaturatorEffect::new(RATE);
    effect.set_param(param::SHAPE, 1.6);
    assert_eq!(effect.get_param(param::SHAPE), 2.0);
    effect.set_param(param::AUTO_GAIN, 0.7);
    assert_eq!(effect.get_param(param::AUTO_GAIN), 1.0);
    effect.set_param(param::QUALITY, 9.0);
    assert_eq!(effect.get_param(param::QUALITY), 2.0);
    effect.set_param(param::DRIVE_DB, -10.0);
    assert_eq!(effect.get_param(param::DRIVE_DB), 0.0);
    assert!(!effect.set_param(param::DRIVE_DB, f32::NAN));
    assert!(!effect.set_param(param::COUNT, 0.0));
}

#[test]
fn bad_input_never_reaches_the_output() {
    let mut effect = saturator(&[(param::DRIVE_DB, 36.0)]);
    let mut input = sine(440.0, 0.5, 2_000);
    input[100] = f32::NAN;
    input[301] = f32::INFINITY;
    input[700] = f32::NEG_INFINITY;
    let out = render(&mut effect, &input);
    assert!(out.iter().all(|s| s.is_finite()));
}

#[test]
fn reset_forgets_what_was_heard() {
    let mut effect = saturator(&[]);
    render(&mut effect, &sine(440.0, 0.8, 4_800));
    effect.reset();
    let out = render(&mut effect, &vec![0.0; 2 * 256]);
    assert!(out.iter().all(|s| *s == 0.0));
    assert_eq!(
        effect.get_param(param::DRIVE_DB),
        SATURATOR_PARAMS[param::DRIVE_DB].default
    );
}

#[test]
fn mono_buffers_work() {
    let mut effect = saturator(&[]);
    let mut buffer: Vec<f32> = (0..512).map(|i| (i as f32 * 0.05).sin() * 0.5).collect();
    effect.process(&mut buffer, 1);
    assert!(buffer.iter().all(|s| s.is_finite()));
    assert!(buffer[LATENCY_SAMPLES as usize + 10..]
        .iter()
        .any(|s| s.abs() > 0.1));
}

#[test]
fn soft_bends_gradually_rather_than_clipping() {
    let db = |x: f32| 20.0 * curve(shape::SOFT, 0.75, [0.0; 3], x).log10();
    // Already giving way at the knee, yet still rising 12 dB past it.
    assert!(db(1.0) < -1.5, "{}", db(1.0));
    assert!(db(4.0) - db(1.0) > 1.0, "{} {}", db(4.0), db(1.0));
}

fn largest_step(samples: &[f32]) -> f32 {
    samples
        .windows(2)
        .map(|w| (w[1] - w[0]).abs())
        .fold(0.0f32, f32::max)
}

#[test]
fn automated_curve_edits_fade_rather_than_step() {
    // A slow wave, so any step stands out against its own motion.
    let input = sine(50.0, 0.8, 9_600);
    let mut effect = saturator(&[(param::DRIVE_DB, 18.0)]);
    let mut out = input.clone();
    for (block, chunk) in out.chunks_mut(2 * 64).enumerate() {
        // A lane sweeping the shape's whole range, a step each block.
        let swept = (block % 2) as f32;
        effect.set_automated_param(param::CURVE, swept);
        effect.process(chunk, 2);
    }
    let jump = largest_step(&left(&out));
    assert!(jump < 0.05, "{jump}");
}

#[test]
fn an_edit_mid_fade_carries_on_from_what_is_heard() {
    let mut effect = saturator(&[(param::DRIVE_DB, 18.0)]);
    let mut out = sine(50.0, 0.8, 4_800);
    let (head, tail) = out.split_at_mut(2 * 2_400);
    effect.process(head, 2);
    effect.set_param(param::SHAPE, shape::FOLD as f32);
    // Well inside the first fade.
    let (mid, rest) = tail.split_at_mut(2 * 100);
    effect.process(mid, 2);
    effect.set_param(param::SHAPE, shape::TUBE as f32);
    effect.process(rest, 2);
    let jump = largest_step(&left(&out));
    assert!(jump < 0.05, "{jump}");
}

#[test]
fn a_filter_switched_back_on_starts_from_silence() {
    let mut effect = saturator(&[(param::PRE_FILTER, 4.0), (param::DC_REMOVAL, 0.0)]);
    render(&mut effect, &sine(30.0, 1.0, 4_800));
    effect.set_param(param::PRE_FILTER, 0.0);
    render(&mut effect, &vec![0.0; 2 * 4_800]);
    effect.set_param(param::PRE_FILTER, 4.0);
    let out = render(&mut effect, &vec![0.0; 2 * 512]);
    assert!(out.iter().all(|s| s.abs() < 1e-6), "{:?}", &out[..8]);
}

#[test]
fn reset_finishes_fades_without_changing_the_sound() {
    let mut effect = saturator(&[]);
    render(&mut effect, &sine(440.0, 0.5, 2_400));
    effect.set_param(param::DRIVE_DB, 24.0);
    effect.reset();
    let mut settled = saturator(&[(param::DRIVE_DB, 24.0)]);
    let input = sine(440.0, 0.5, 2_400);
    assert_eq!(render(&mut effect, &input), render(&mut settled, &input));
}
