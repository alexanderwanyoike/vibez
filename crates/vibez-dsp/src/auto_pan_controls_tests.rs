//! The controls added for the concept: shapes, mode, phase, offset,
//! smoothing, tempo sync and locking to the song position.

use super::*;
use crate::effect::AudioEffect;

fn params(edits: &[(usize, f32)]) -> Vec<f32> {
    let mut params: Vec<f32> = AUTO_PAN_PARAMS.iter().map(|d| d.default).collect();
    for &(index, value) in edits {
        params[index] = value;
    }
    params
}

fn effect(edits: &[(usize, f32)]) -> AutoPanEffect {
    let mut effect = AutoPanEffect::new(48_000.0);
    for &(index, value) in edits {
        effect.set_param(index, value);
    }
    effect
}

fn context(musical_sample: u64, bpm: f64, playing: bool) -> DeviceAudioContext {
    DeviceAudioContext {
        musical_sample,
        continuous_sample: musical_sample,
        sample_rate: 48_000,
        bpm,
        playing,
    }
}

fn cycle(edits: &[(usize, f32)]) -> Vec<[f32; 2]> {
    let params = params(edits);
    (0..400)
        .map(|step| gains_at(&params, step as f32 / 400.0))
        .collect()
}

#[test]
fn pan_mode_moves_the_sound_between_the_sides() {
    let gains = cycle(&[(param::DEPTH, 1.0)]);
    let quarter = gains[100];
    assert!(
        quarter[0] < 0.01 && (quarter[1] - 2.0).abs() < 0.01,
        "{quarter:?}"
    );
    for [left, right] in gains {
        assert!((left + right - 2.0).abs() < 1e-5);
    }
}

#[test]
fn tremolo_mode_only_turns_the_sound_down() {
    let gains = cycle(&[(param::DEPTH, 1.0), (param::MODE, 1.0)]);
    assert!(gains.iter().flatten().all(|gain| *gain <= 1.0 + 1e-6));
    assert!(gains.iter().flatten().any(|gain| *gain < 0.01));
}

#[test]
fn zero_phase_moves_both_sides_together() {
    for [left, right] in cycle(&[(param::PHASE, 0.0), (param::MODE, 1.0)]) {
        assert!((left - right).abs() < 1e-6);
    }
}

#[test]
fn each_shape_has_its_form() {
    let at = |shape: usize, cycles: f32| {
        gains_at(
            &params(&[(param::DEPTH, 1.0), (param::SHAPE, shape as f32)]),
            cycles,
        )[1] - 1.0
    };
    assert!((at(shape::SINE, 0.125) - (std::f32::consts::FRAC_PI_4).sin()).abs() < 1e-4);
    assert!((at(shape::TRIANGLE, 0.125) - 0.5).abs() < 1e-4);
    assert!((at(shape::SQUARE, 0.125) - 1.0).abs() < 1e-6);
    assert!((at(shape::SAW_UP, 0.25) - 0.5).abs() < 1e-4);
    assert!((at(shape::SAW_DOWN, 0.25) + 0.5).abs() < 1e-4);
}

#[test]
fn smoothing_rounds_the_square_edges() {
    let biggest_step = |smoothing: f32| {
        let gains = cycle(&[
            (param::DEPTH, 1.0),
            (param::SHAPE, shape::SQUARE as f32),
            (param::SMOOTHING, smoothing),
        ]);
        gains
            .windows(2)
            .map(|pair| (pair[1][0] - pair[0][0]).abs())
            .fold(0.0, f32::max)
    };
    assert!(biggest_step(0.0) > 1.9);
    assert!(biggest_step(0.5) < 0.1);
}

#[test]
fn offset_starts_the_lfo_later_in_its_cycle() {
    let shifted = params(&[(param::OFFSET, 0.25)]);
    let plain = params(&[]);
    for step in 0..20 {
        let cycles = step as f32 / 20.0;
        let a = gains_at(&shifted, cycles);
        let b = gains_at(&plain, cycles + 0.25);
        assert!((a[0] - b[0]).abs() < 1e-4 && (a[1] - b[1]).abs() < 1e-4);
    }
}

#[test]
fn a_synced_rate_follows_the_tempo() {
    // 1/4: one cycle a beat.
    let quarter = params(&[(param::SYNC, 4.0)]);
    assert!((rate_hz(&quarter, 120.0) - 2.0).abs() < 1e-9);
    assert!((rate_hz(&quarter, 90.0) - 1.5).abs() < 1e-9);
    assert_eq!(rate_hz(&params(&[]), 120.0), 2.0);
}

#[test]
fn locked_to_the_song_two_instances_agree_wherever_they_started() {
    let edits = [
        (param::SYNC, 4.0),
        (param::RETRIGGER, 1.0),
        (param::DEPTH, 1.0),
    ];
    let mut early = effect(&edits);
    let mut late = effect(&edits);
    let mut buffer = vec![0.5; 3_000 * 2];
    early.process(&mut buffer, 2);
    let at = 96_123;
    early.set_audio_context(context(at, 120.0, true));
    late.set_audio_context(context(at, 120.0, true));
    let (mut a, mut b) = (vec![0.5; 512], vec![0.5; 512]);
    early.process(&mut a, 2);
    late.process(&mut b, 2);
    for (a, b) in a.iter().zip(&b) {
        assert!((a - b).abs() < 1e-5);
    }
}

#[test]
fn free_running_ignores_the_song_position() {
    let mut fx = effect(&[(param::SYNC, 4.0), (param::DEPTH, 1.0)]);
    fx.set_audio_context(context(24_000, 120.0, true));
    let mut buffer = vec![0.5; 2];
    fx.process(&mut buffer, 2);
    assert_eq!(buffer, [0.5, 0.5], "starts from the phase it had");
}

#[test]
fn a_moved_depth_glides_rather_than_clicks() {
    let mut fx = effect(&[(param::RATE, 1.0), (param::DEPTH, 0.0)]);
    fx.process(&mut vec![1.0; 12_000 * 2], 2);
    fx.set_param(param::DEPTH, 1.0);
    let mut frame = [1.0, 1.0];
    fx.process(&mut frame, 2);
    assert!((frame[0] - 1.0).abs() < 0.01, "{frame:?}");
    fx.process(&mut vec![1.0; 2_000], 2);
    let mut frame = [1.0, 1.0];
    fx.process(&mut frame, 2);
    assert!(frame[0] < 0.05, "a quarter cycle in, full depth: {frame:?}");
}

#[test]
fn mono_audio_gets_a_tremolo() {
    let mut fx = effect(&[(param::RATE, 1.0), (param::DEPTH, 1.0)]);
    let mut buffer = vec![1.0; 48_000];
    fx.process(&mut buffer, 1);
    assert!(buffer.iter().all(|sample| (0.0..=1.0).contains(sample)));
    assert!(buffer.iter().any(|sample| *sample < 0.01));
}

#[test]
fn bad_input_and_values_stay_safe() {
    let mut fx = effect(&[(param::DEPTH, 1.0)]);
    for index in 0..param::COUNT {
        let before = fx.get_param(index);
        assert!(!fx.set_param(index, f32::NAN));
        assert_eq!(fx.get_param(index), before);
    }
    let mut buffer = [f32::NAN, f32::INFINITY, 0.5, -0.5];
    fx.process(&mut buffer, 2);
    assert!(buffer.iter().all(|sample| sample.is_finite()));
}
