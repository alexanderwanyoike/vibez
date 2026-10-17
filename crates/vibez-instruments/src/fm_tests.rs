use super::*;
use crate::fm_params::LEVEL_FLOOR_DB;

const RATE: f32 = 48_000.0;
const SERIAL: f32 = 0.0;
const ADDITIVE: f32 = 7.0;

fn synth(edits: &[(usize, f32)]) -> FmSynth {
    let mut synth = FmSynth::new(RATE);
    for &(index, value) in edits {
        synth.set_param(index, value);
    }
    synth
}

/// `frames` of mono output.
fn play(synth: &mut FmSynth, frames: usize) -> Vec<f32> {
    let mut buffer = vec![0.0; frames];
    for block in buffer.chunks_mut(128) {
        synth.render(block, 1);
    }
    buffer
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

fn peak(signal: &[f32]) -> f32 {
    signal
        .iter()
        .fold(0.0f32, |peak, sample| peak.max(sample.abs()))
}

fn hz(pitch: u8) -> f32 {
    440.0 * ((f32::from(pitch) - 69.0) / 12.0).exp2()
}

/// A held note's steady part, after the attack, at full velocity.
fn held(edits: &[(usize, f32)], pitch: u8) -> Vec<f32> {
    held_at(edits, pitch, 127)
}

fn held_at(edits: &[(usize, f32)], pitch: u8, velocity: u8) -> Vec<f32> {
    let mut synth = synth(edits);
    synth.note_on(pitch, velocity);
    play(&mut synth, 2_400);
    play(&mut synth, 9_600)
}

fn of(operator: usize, control: usize) -> usize {
    param::of(operator, control)
}

/// Only A sounds, held at full level, unless the edits say otherwise.
fn sine_only() -> Vec<(usize, f32)> {
    (0..OPERATORS)
        .map(|operator| {
            let level = if operator == 0 { 0.0 } else { LEVEL_FLOOR_DB };
            (of(operator, op::LEVEL), level)
        })
        .chain((0..OPERATORS).map(|operator| (of(operator, op::SUSTAIN), 1.0)))
        .collect()
}

/// How far the third harmonic sits under the fundamental.
fn third_under_first(out: &[f32], pitch: u8) -> f32 {
    level_db(out, 3.0 * hz(pitch)) - level_db(out, hz(pitch))
}

#[test]
fn silent_without_notes() {
    assert!(play(&mut synth(&[]), 4_800).iter().all(|s| *s == 0.0));
}

#[test]
fn a_note_sounds_at_its_pitch() {
    let out = held(&[], 69);
    let fundamental = level_db(&out, 440.0);
    assert!(fundamental > -30.0, "{fundamental}");
    assert!(fundamental - level_db(&out, 466.16) > 20.0);
}

#[test]
fn a_lone_carrier_is_a_sine() {
    let out = held(&sine_only(), 57);
    assert!(third_under_first(&out, 57) < -60.0);
    assert!(level_db(&out, 2.0 * hz(57)) - level_db(&out, hz(57)) < -60.0);
}

#[test]
fn a_modulator_adds_sidebands_at_its_ratio() {
    let mut edits = sine_only();
    edits.extend([(of(1, op::LEVEL), 0.0), (of(1, op::RATIO), 2.0)]);
    let out = held(&edits, 57);
    // A carrier at f moved by 2f has sidebands at f + 2f and |f - 2f|,
    // odd harmonics only.
    assert!(
        third_under_first(&out, 57) > -25.0,
        "{}",
        third_under_first(&out, 57)
    );
    assert!(level_db(&out, 2.0 * hz(57)) - level_db(&out, hz(57)) < -60.0);
}

#[test]
fn brightness_deepens_the_modulation() {
    // A modulator at four times the note puts its first sideband alone at
    // five times it, weighted by J1 of the index, which rises up to 1.84.
    let fifth = |brightness: f32| {
        let mut edits = sine_only();
        edits.extend([
            (of(1, op::LEVEL), 0.0),
            (of(1, op::RATIO), 4.0),
            (param::BRIGHTNESS, brightness),
        ]);
        level_db(&held(&edits, 45), 5.0 * hz(45))
    };
    assert!(fifth(0.0) < -80.0, "{}", fifth(0.0));
    assert!(fifth(0.1) < fifth(0.2) - 3.0);
    assert!(fifth(0.2) < fifth(0.25));
}

#[test]
fn feedback_turns_a_sine_toward_a_saw() {
    let second = |feedback: f32| {
        let mut edits = sine_only();
        edits.push((of(0, op::FEEDBACK), feedback));
        let out = held(&edits, 57);
        level_db(&out, 2.0 * hz(57)) - level_db(&out, hz(57))
    };
    assert!(second(0.0) < -60.0);
    assert!(second(1.0) > -20.0, "{}", second(1.0));
}

#[test]
fn only_carriers_are_heard() {
    // D at twice the note, all else silent but A. Heard, D is a second
    // harmonic; moving A, it makes odd harmonics only.
    let mut edits = sine_only();
    edits.extend([(of(3, op::LEVEL), 0.0), (of(3, op::RATIO), 2.0)]);
    let out = |algorithm: f32| {
        let mut edits = edits.clone();
        edits.push((param::ALGORITHM, algorithm));
        held(&edits, 57)
    };
    let second = |out: &[f32]| level_db(out, 2.0 * hz(57)) - level_db(out, hz(57));
    let summed = out(ADDITIVE);
    assert!(second(&summed).abs() < 1.0, "{}", second(&summed));
    // Through a silent C and B, D never reaches A.
    let serial = out(SERIAL);
    assert!(second(&serial) < -60.0 && third_under_first(&serial, 57) < -60.0);
    // Straight into A, D moves it and is not heard as itself.
    let into_a = out(5.0);
    assert!(second(&into_a) < -60.0, "{}", second(&into_a));
    assert!(third_under_first(&into_a, 57) > -40.0);
}

#[test]
fn more_carriers_are_no_louder() {
    let one = peak(&held(&sine_only(), 57));
    let all = peak(&held(
        &[
            (param::ALGORITHM, ADDITIVE),
            (of(1, op::LEVEL), 0.0),
            (of(2, op::LEVEL), 0.0),
            (of(3, op::LEVEL), 0.0),
            (of(1, op::SUSTAIN), 1.0),
            (of(2, op::SUSTAIN), 1.0),
            (of(3, op::SUSTAIN), 1.0),
            (of(0, op::SUSTAIN), 1.0),
        ],
        57,
    ));
    assert!(all <= one * 1.01, "{one} {all}");
}

#[test]
fn key_track_softens_modulators_up_high() {
    let third = |key_track: f32| {
        let mut edits = sine_only();
        edits.extend([
            (of(1, op::LEVEL), 0.0),
            (of(1, op::RATIO), 2.0),
            (of(1, op::KEY_TRACK), key_track),
        ]);
        third_under_first(&held(&edits, 84), 84)
    };
    assert!(
        third(1.0) < third(0.0) - 6.0,
        "{} {}",
        third(1.0),
        third(0.0)
    );
}

#[test]
fn a_soft_note_is_darker_as_velocity_asks() {
    let third = |sense: f32| {
        let mut edits = sine_only();
        edits.extend([
            (of(1, op::LEVEL), 0.0),
            (of(1, op::RATIO), 2.0),
            (param::VELOCITY, sense),
        ]);
        third_under_first(&held_at(&edits, 57, 40), 57)
    };
    assert!(
        third(1.0) < third(0.0) - 6.0,
        "{} {}",
        third(1.0),
        third(0.0)
    );
}

#[test]
fn decay_lengthens_every_decay() {
    let late = |decay: f32| {
        let mut synth = synth(&[
            (of(0, op::DECAY), 0.5),
            (of(0, op::SUSTAIN), 0.0),
            (param::DECAY, decay),
        ]);
        synth.note_on(57, 127);
        play(&mut synth, 24_000);
        peak(&play(&mut synth, 4_800))
    };
    assert!(late(1.0) > 4.0 * late(0.5), "{} {}", late(1.0), late(0.5));
    assert!(late(0.0) < late(0.5));
}

#[test]
fn motion_moves_the_tone_at_the_lfo_rate() {
    let mut edits = sine_only();
    edits.extend([
        (of(1, op::LEVEL), 0.0),
        (of(1, op::RATIO), 2.0),
        (param::BRIGHTNESS, 0.2),
        (param::MOTION, 1.0),
        (param::LFO_RATE, 2.0),
        (param::LFO_DEST, dest::BRIGHTNESS as f32),
    ]);
    let mut synth = synth(&edits);
    synth.note_on(57, 127);
    // A quarter of the LFO's cycle in, at its height, then half a cycle
    // on, at its depth.
    play(&mut synth, 6_000 - 1_200);
    let high = play(&mut synth, 2_400);
    play(&mut synth, 12_000 - 2_400);
    let low = play(&mut synth, 2_400);
    assert!(
        third_under_first(&high, 57) > third_under_first(&low, 57) + 10.0,
        "{} {}",
        third_under_first(&high, 57),
        third_under_first(&low, 57)
    );
}

#[test]
fn a_released_note_falls_silent() {
    let mut synth = synth(&[]);
    synth.note_on(60, 127);
    play(&mut synth, 4_800);
    synth.note_off(60);
    play(&mut synth, 48_000);
    assert!(play(&mut synth, 4_800).iter().all(|s| *s == 0.0));
}

#[test]
fn one_voice_plays_one_note() {
    let mut synth = synth(&[(param::VOICES, 1.0)]);
    synth.note_on(57, 127);
    synth.note_on(64, 127);
    play(&mut synth, 4_800);
    let out = play(&mut synth, 9_600);
    assert!(level_db(&out, hz(64)) - level_db(&out, hz(57)) > 30.0);
}

#[test]
fn ratios_step_by_halves_and_choices_snap() {
    let mut synth = synth(&[]);
    synth.set_param(of(1, op::RATIO), 2.3);
    assert_eq!(synth.get_param(of(1, op::RATIO)), 2.5);
    synth.set_param(param::ALGORITHM, 2.6);
    assert_eq!(synth.get_param(param::ALGORITHM), 3.0);
    synth.set_param(param::ALGORITHM, 99.0);
    assert_eq!(synth.get_param(param::ALGORITHM), 7.0);
    assert!(!synth.set_param(param::COUNT, 1.0));
    assert_eq!(FM_PARAMS.len(), param::COUNT);
    assert_eq!(ALGORITHM_NAMES.len(), ALGORITHMS.len());
}

#[test]
fn level_edits_glide_rather_than_step() {
    let mut synth = synth(&sine_only());
    synth.note_on(57, 127);
    play(&mut synth, 4_800);
    synth.set_param(of(0, op::LEVEL), LEVEL_FLOOR_DB);
    let out = play(&mut synth, 4_800);
    let jump = out
        .windows(2)
        .fold(0.0f32, |jump, pair| jump.max((pair[1] - pair[0]).abs()));
    assert!(jump < 0.05, "{jump}");
}

#[test]
fn every_channel_gets_the_sound() {
    let mut synth = synth(&[]);
    synth.note_on(57, 127);
    let mut buffer = vec![0.0; 2 * 960];
    synth.render(&mut buffer, 2);
    assert!(buffer.chunks(2).all(|frame| frame[0] == frame[1]));
    assert!(peak(&buffer) > 0.01);
}

#[test]
fn reset_silences_everything() {
    let mut synth = synth(&[]);
    synth.note_on(57, 127);
    play(&mut synth, 480);
    synth.reset();
    assert!(play(&mut synth, 480).iter().all(|s| *s == 0.0));
}

#[test]
fn the_preview_draws_two_cycles_of_the_patch() {
    let mut params: Vec<f32> = FM_PARAMS.iter().map(|d| d.default).collect();
    params[of(1, op::LEVEL)] = LEVEL_FLOOR_DB;
    let sine = preview(&params, 400);
    assert_eq!(sine.len(), 400);
    let crossings = sine
        .windows(2)
        .filter(|pair| pair[0] < 0.0 && pair[1] >= 0.0)
        .count();
    assert_eq!(crossings, 2);
    params[of(1, op::LEVEL)] = 0.0;
    params[of(1, op::RATIO)] = 5.0;
    let bright = preview(&params, 400);
    let wiggles = bright
        .windows(2)
        .filter(|pair| pair[0] < 0.0 && pair[1] >= 0.0)
        .count();
    assert!(wiggles > crossings, "{wiggles}");
    assert!(preview(&params[..3], 400).is_empty());
}
