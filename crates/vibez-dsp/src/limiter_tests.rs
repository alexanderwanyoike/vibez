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

fn limiter(edits: &[(usize, f32)]) -> LimiterEffect {
    let mut limiter = LimiterEffect::new(RATE);
    for &(index, value) in edits {
        limiter.set_param(index, value);
    }
    limiter
}

/// Renders in uneven blocks, as the engine's callbacks arrive.
fn render(effect: &mut LimiterEffect, input: &[f32]) -> Vec<f32> {
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

fn right(samples: &[f32]) -> Vec<f32> {
    samples.iter().skip(1).step_by(2).copied().collect()
}

fn peak(samples: &[f32]) -> f32 {
    samples
        .iter()
        .fold(0.0f32, |peak, sample| peak.max(sample.abs()))
}

fn rms_db(samples: &[f32]) -> f32 {
    let power = samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32;
    10.0 * power.max(1e-24).log10()
}

/// The highest level the waveform reaches between samples, by 16x
/// windowed-sinc reconstruction: an independent check on the limiter's own
/// interpolator.
fn true_peak(samples: &[f32]) -> f32 {
    let half = 32isize;
    let mut highest = peak(samples);
    for n in half as usize..samples.len() - half as usize {
        for step in 1..16 {
            let t = step as f64 / 16.0;
            let mut sum = 0.0f64;
            for k in -half + 1..=half {
                let x = k as f64 - t;
                let sinc = (std::f64::consts::PI * x).sin() / (std::f64::consts::PI * x);
                let window = 0.5 + 0.5 * (std::f64::consts::PI * x / half as f64).cos();
                sum += f64::from(samples[(n as isize + k) as usize]) * sinc * window;
            }
            highest = highest.max(sum.abs() as f32);
        }
    }
    highest
}

/// Loud transients over a steady noise bed.
fn drums(frames: usize, level: f32) -> Vec<f32> {
    let mut seed = 0x1234_5678u32;
    let mut random = move || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        seed as f32 / u32::MAX as f32 * 2.0 - 1.0
    };
    (0..frames)
        .flat_map(|frame| {
            let hit = (-((frame % 12_000) as f32) / 600.0).exp();
            let left = level * (0.3 * random() + hit * random().signum());
            let right = level * (0.3 * random() + 0.8 * hit * random().signum());
            [left, right]
        })
        .collect()
}

#[test]
fn nothing_passes_the_ceiling_whatever_the_settings() {
    let ceiling = db_to_gain(-1.0);
    for style in [style::TRANSPARENT, style::PUNCHY, style::LOUD] {
        for (quality, true_peak) in [(1.0, 1.0), (0.0, 0.0), (1.0, 0.0)] {
            for lookahead in [0.5, 2.0, 5.0] {
                let mut effect = limiter(&[
                    (param::STYLE, style as f32),
                    (param::QUALITY, quality),
                    (param::TRUE_PEAK, true_peak),
                    (param::LOOKAHEAD_MS, lookahead),
                    (param::INPUT_DB, 18.0),
                ]);
                let out = render(&mut effect, &drums(48_000, 0.9));
                let highest = peak(&out);
                assert!(
                    highest <= ceiling + 1e-6,
                    "style {style} quality {quality} tp {true_peak} lookahead {lookahead}: {highest}"
                );
            }
        }
    }
}

#[test]
fn true_peak_keeps_the_peaks_between_samples_under_the_ceiling_too() {
    // A quarter-rate sine sampled half way between its crests: every
    // sample sits 3 dB under the peak the waveform reaches between them.
    let frames = 24_000;
    let input: Vec<f32> = (0..frames)
        .flat_map(|frame| {
            let value =
                (std::f32::consts::FRAC_PI_2 * frame as f32 + std::f32::consts::FRAC_PI_4).sin();
            [value, value]
        })
        .collect();
    let ceiling = db_to_gain(-1.0);
    let settle = |out: &[f32]| left(out)[4_800..].to_vec();
    let mut on = limiter(&[(param::TRUE_PEAK, 1.0)]);
    let heard = true_peak(&settle(&render(&mut on, &input)));
    assert!(
        heard <= ceiling * db_to_gain(0.3),
        "with true peak: {}",
        gain_to_db(heard)
    );
    let mut off = limiter(&[(param::TRUE_PEAK, 0.0), (param::QUALITY, 0.0)]);
    let heard = true_peak(&settle(&render(&mut off, &input)));
    assert!(
        heard > ceiling * db_to_gain(1.0),
        "without: {}",
        gain_to_db(heard)
    );
}

#[test]
fn a_quiet_signal_comes_through_untouched_after_the_latency() {
    let mut effect = limiter(&[]);
    let input = sine(997.0, 0.1, 9_600);
    let out = render(&mut effect, &input);
    let latency = effect.latency_samples() as usize;
    for (n, (heard, sent)) in left(&out)[latency..]
        .iter()
        .zip(left(&input))
        .enumerate()
        .skip(64)
    {
        assert!((heard - sent).abs() < 1e-3, "{n}: {heard} {sent}");
    }
    assert!(effect.take_dynamics_meter().unwrap().peak_reduction_db[0] < 1e-3);
}

#[test]
fn the_latency_never_changes_with_the_settings() {
    let reported = LimiterEffect::new(RATE).latency_samples();
    for lookahead in [0.5, 1.0, 5.0] {
        for style in [style::TRANSPARENT, style::PUNCHY, style::LOUD] {
            let effect = limiter(&[
                (param::LOOKAHEAD_MS, lookahead),
                (param::STYLE, style as f32),
            ]);
            assert_eq!(effect.latency_samples(), reported);
        }
    }
    let mut effect = limiter(&[]);
    let mut impulse = vec![0.0; 2 * 1_024];
    impulse[0] = 0.5;
    impulse[1] = 0.5;
    let out = left(&render(&mut effect, &impulse));
    let loudest = out.iter().enumerate().fold((0, 0.0f32), |best, (n, s)| {
        if s.abs() > best.1 {
            (n, s.abs())
        } else {
            best
        }
    });
    assert_eq!(loudest.0, reported as usize);
}

#[test]
fn the_gain_is_already_down_when_a_peak_arrives() {
    // Quiet, then a loud step: lookahead lowers the gain over the quiet
    // samples just before the step reaches the output.
    for lookahead in [0.5, 2.0, 5.0] {
        let mut effect = limiter(&[(param::LOOKAHEAD_MS, lookahead), (param::TRUE_PEAK, 0.0)]);
        let step = 4_800;
        let input: Vec<f32> = (0..9_600)
            .flat_map(|n| {
                let value = if n < step { 0.1 } else { 1.0 };
                [value, value]
            })
            .collect();
        let out = left(&render(&mut effect, &input));
        let latency = effect.latency_samples() as usize;
        let window = (lookahead * 0.001 * RATE) as usize;
        let before = out[step + latency - window / 2];
        assert!(before < 0.1 * 0.95, "lookahead {lookahead}: {before}");
        let earlier = out[step + latency - window - 40];
        assert!(
            (earlier - 0.1).abs() < 1e-3,
            "lookahead {lookahead}: {earlier}"
        );
    }
}

/// How long after a loud burst ends until the reduction is under 1 dB, in
/// milliseconds.
fn recovery_ms(edits: &[(usize, f32)], burst_frames: usize) -> f32 {
    // Driven 12 dB into the ceiling, then a tail well under it.
    let mut effect = limiter(edits);
    effect.set_param(param::INPUT_DB, 12.0);
    let mut input = sine(100.0, 1.0, burst_frames);
    input.extend(sine(100.0, 0.05, 144_000));
    let mut reduction = Vec::new();
    for chunk in input.chunks(2 * 48) {
        let mut block = chunk.to_vec();
        effect.process(&mut block, 2);
        reduction.push(effect.take_dynamics_meter().unwrap().current_reduction_db[0]);
    }
    let start = burst_frames / 48;
    reduction[start..]
        .iter()
        .position(|db| *db < 1.0)
        .expect("recovers") as f32
}

#[test]
fn release_sets_how_fast_the_gain_comes_back() {
    let fast = recovery_ms(&[(param::RELEASE_MS, 20.0)], 9_600);
    let slow = recovery_ms(&[(param::RELEASE_MS, 400.0)], 9_600);
    assert!(fast < 60.0 && slow > 3.0 * fast, "{fast} {slow}");
}

#[test]
fn auto_release_lets_a_transient_go_quickly_and_holds_a_long_push() {
    let transient = recovery_ms(&[], 480);
    let sustained = recovery_ms(&[], 96_000);
    assert!(transient < 120.0, "{transient}");
    assert!(sustained > 2.0 * transient, "{transient} {sustained}");
}

#[test]
fn punchy_recovers_faster_than_transparent() {
    let edits = |style: usize| [(param::STYLE, style as f32), (param::RELEASE_MS, 200.0)];
    let transparent = recovery_ms(&edits(style::TRANSPARENT), 9_600);
    let punchy = recovery_ms(&edits(style::PUNCHY), 9_600);
    assert!(punchy < transparent * 0.75, "{punchy} {transparent}");
}

#[test]
fn loud_is_denser_under_the_same_ceiling() {
    let level = |style: usize| {
        let mut effect = limiter(&[(param::STYLE, style as f32), (param::INPUT_DB, 12.0)]);
        let out = render(&mut effect, &drums(96_000, 0.9));
        let heard = left(&out)[9_600..].to_vec();
        (rms_db(&heard), peak(&heard))
    };
    let (transparent, _) = level(style::TRANSPARENT);
    let (loud, highest) = level(style::LOUD);
    assert!(loud > transparent + 0.5, "{loud} {transparent}");
    assert!(highest <= db_to_gain(-1.0) + 1e-6);
}

#[test]
fn stereo_link_carries_one_side_s_reduction_to_the_other() {
    let input: Vec<f32> = (0..24_000)
        .flat_map(|n| {
            let phase = std::f32::consts::TAU * 200.0 * n as f32 / RATE;
            [phase.sin(), 0.2 * phase.sin()]
        })
        .collect();
    let quiet_side = |link: f32| {
        let mut effect = limiter(&[(param::STEREO_LINK, link), (param::INPUT_DB, 6.0)]);
        let out = render(&mut effect, &input);
        rms_db(&right(&out)[9_600..]) - rms_db(&right(&input)[9_600..])
    };
    let unlinked = quiet_side(0.0);
    let linked = quiet_side(1.0);
    assert!((unlinked - 6.0).abs() < 0.2, "{unlinked}");
    assert!(linked < unlinked - 4.0, "{linked}");
}

#[test]
fn the_meter_reports_reduction_and_input_and_starts_over_once_read() {
    let mut effect = limiter(&[(param::INPUT_DB, 12.0)]);
    render(&mut effect, &sine(200.0, 0.8, 4_800));
    let meter = effect.take_dynamics_meter().unwrap();
    assert!(meter.peak_reduction_db[0] > 6.0, "{meter:?}");
    assert!(
        (meter.detector_peak[0] - 0.8 * db_to_gain(12.0)).abs() < 0.05,
        "{meter:?}"
    );
    let again = effect.take_dynamics_meter().unwrap();
    assert_eq!(again.peak_reduction_db, [0.0; 2]);
    assert_eq!(again.detector_peak, [0.0; 2]);
}

#[test]
fn edits_glide_rather_than_step() {
    for (index, value) in [
        (param::CEILING_DB, -12.0),
        (param::INPUT_DB, 12.0),
        (param::STYLE, style::LOUD as f32),
        (param::QUALITY, 0.0),
        (param::LOOKAHEAD_MS, 0.5),
    ] {
        let mut effect = limiter(&[(param::INPUT_DB, 6.0)]);
        let mut out = sine(100.0, 0.7, 9_600);
        let half = out.len() / 2;
        effect.process(&mut out[..half], 2);
        effect.set_param(index, value);
        effect.process(&mut out[half..], 2);
        let out = left(&out);
        let jump = out
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0f32, f32::max);
        assert!(jump < 0.06, "param {index}: {jump}");
    }
}

#[test]
fn choices_snap_and_ranges_clamp() {
    let mut effect = LimiterEffect::new(RATE);
    effect.set_param(param::STYLE, 1.6);
    assert_eq!(effect.get_param(param::STYLE), 2.0);
    effect.set_param(param::TRUE_PEAK, 0.2);
    assert_eq!(effect.get_param(param::TRUE_PEAK), 0.0);
    effect.set_param(param::CEILING_DB, 6.0);
    assert_eq!(effect.get_param(param::CEILING_DB), 0.0);
    assert!(!effect.set_param(param::INPUT_DB, f32::NAN));
    assert!(!effect.set_param(param::COUNT, 0.0));
    let mut params: Vec<f32> = LIMITER_PARAMS.iter().map(|d| d.default).collect();
    assert!(auto_release(&params));
    params[param::RELEASE_MS] = 50.0;
    assert!(!auto_release(&params));
}

#[test]
fn bad_input_never_reaches_the_output() {
    let mut effect = limiter(&[(param::INPUT_DB, 24.0)]);
    let mut input = sine(440.0, 0.5, 2_000);
    input[100] = f32::NAN;
    input[301] = f32::INFINITY;
    input[700] = f32::NEG_INFINITY;
    let out = render(&mut effect, &input);
    assert!(out.iter().all(|s| s.is_finite() && s.abs() <= 1.0));
}

#[test]
fn reset_forgets_what_was_heard() {
    let mut effect = limiter(&[(param::INPUT_DB, 12.0)]);
    render(&mut effect, &sine(440.0, 0.9, 4_800));
    effect.reset();
    let out = render(&mut effect, &vec![0.0; 2 * 1_024]);
    assert!(out.iter().all(|s| *s == 0.0));
    let quiet = sine(997.0, 0.1, 4_800);
    let heard = render(&mut effect, &quiet);
    let mut fresh = limiter(&[(param::INPUT_DB, 12.0)]);
    let _ = render(&mut fresh, &vec![0.0; 2 * 1_024]);
    assert_eq!(heard, render(&mut fresh, &quiet));
}

#[test]
fn mono_buffers_work() {
    let mut effect = limiter(&[(param::INPUT_DB, 12.0)]);
    let mut buffer: Vec<f32> = (0..4_800).map(|i| (i as f32 * 0.05).sin() * 0.9).collect();
    effect.process(&mut buffer, 1);
    assert!(buffer
        .iter()
        .all(|s| s.is_finite() && s.abs() <= db_to_gain(-1.0) + 1e-6));
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
    20.0 * amplitude.max(1e-12).log10() as f32
}

#[test]
fn lowering_the_ceiling_lowers_the_gain_rather_than_clipping() {
    let mut effect = limiter(&[(param::TRUE_PEAK, 0.0)]);
    let mut out = sine(1_000.0, 0.8, 9_600);
    let edit = 4_800;
    effect.process(&mut out[..edit * 2], 2);
    effect.set_param(param::CEILING_DB, -12.0);
    effect.process(&mut out[edit * 2..], 2);
    // The glide and the delay behind it, where a clip would show.
    let latency = effect.latency_samples() as usize;
    let glide = &left(&out)[edit..edit + latency + RAMP_FRAMES as usize];
    let distortion = level_db(glide, 3_000.0) - level_db(glide, 1_000.0);
    assert!(distortion < -40.0, "{distortion}");
}
