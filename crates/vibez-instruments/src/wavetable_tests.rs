use super::*;
use crate::wavetable_params::osc;

const RATE: f32 = 48_000.0;

fn synth(edits: &[(usize, f32)]) -> WavetableSynth {
    let mut synth = WavetableSynth::new(RATE);
    // The tests reason about Basic Shapes' sine, saw and square, whichever
    // table a new synth starts on.
    synth.set_param(
        param::of(0, osc::TABLE),
        crate::wavetables::table::BASIC_SHAPES as f32,
    );
    for &(index, value) in edits {
        synth.set_param(index, value);
    }
    synth
}

/// `frames` of stereo output.
fn play(synth: &mut WavetableSynth, frames: usize) -> Vec<f32> {
    let mut buffer = vec![0.0; frames * 2];
    for block in buffer.chunks_mut(2 * 128) {
        synth.render(block, 2);
    }
    buffer
}

fn left(samples: &[f32]) -> Vec<f32> {
    samples.iter().step_by(2).copied().collect()
}

fn right(samples: &[f32]) -> Vec<f32> {
    samples.iter().skip(1).step_by(2).copied().collect()
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

fn rms_db(signal: &[f32]) -> f32 {
    let power = signal.iter().map(|s| s * s).sum::<f32>() / signal.len() as f32;
    10.0 * power.max(1e-24).log10()
}

fn hz(pitch: u8) -> f32 {
    440.0 * ((f32::from(pitch) - 69.0) / 12.0).exp2()
}

/// A held note's steady part, after the attack.
fn held(edits: &[(usize, f32)], pitch: u8) -> Vec<f32> {
    let mut synth = synth(edits);
    synth.note_on(pitch, 127);
    play(&mut synth, 2_400);
    play(&mut synth, 9_600)
}

const OSC_A_POSITION: usize = param::of(0, osc::POSITION);

#[test]
fn silent_without_notes() {
    let mut synth = synth(&[]);
    assert!(play(&mut synth, 4_800).iter().all(|s| *s == 0.0));
}

#[test]
fn a_note_sounds_at_its_pitch() {
    let out = left(&held(&[], 69));
    let fundamental = level_db(&out, 440.0);
    assert!(fundamental > -30.0, "{fundamental}");
    assert!(fundamental - level_db(&out, 466.16) > 20.0);
}

#[test]
fn high_notes_do_not_alias() {
    // A saw at C8: its seventh harmonic would fold back to 18.7 kHz.
    let out = left(&held(&[(OSC_A_POSITION, 0.66)], 108));
    let fundamental = level_db(&out, hz(108));
    let alias = level_db(&out, RATE - 7.0 * hz(108));
    assert!(fundamental - alias > 60.0, "{fundamental} {alias}");
}

#[test]
fn position_morphs_from_sine_to_square() {
    let at = |position: f32| {
        let out = left(&held(&[(OSC_A_POSITION, position)], 57));
        level_db(&out, 3.0 * hz(57)) - level_db(&out, hz(57))
    };
    let sine = at(0.0);
    let square = at(1.0);
    assert!(sine < -50.0, "{sine}");
    // A square's third harmonic is a third of its first, 9.5 dB down.
    assert!((square - -9.54).abs() < 1.0, "{square}");
}

#[test]
fn warp_bends_a_sine_into_harmonics() {
    let second = |warp: f32| {
        let out = left(&held(
            &[(OSC_A_POSITION, 0.0), (param::of(0, osc::WARP), warp)],
            57,
        ));
        level_db(&out, 2.0 * hz(57)) - level_db(&out, hz(57))
    };
    assert!(second(0.0) < -50.0);
    assert!(second(0.8) > -20.0, "{}", second(0.8));
}

#[test]
fn unison_spreads_across_the_stereo_field_without_getting_louder() {
    let single = held(&[], 57);
    assert_eq!(left(&single), right(&single));
    let stacked = held(
        &[
            (param::of(0, osc::UNISON), 5.0),
            (param::of(0, osc::DETUNE), 20.0),
        ],
        57,
    );
    let (l, r) = (left(&stacked), right(&stacked));
    let difference: f32 = l.iter().zip(&r).map(|(a, b)| (a - b).abs()).sum();
    assert!(difference > 1.0, "{difference}");
    let louder = rms_db(&l) - rms_db(&left(&single));
    assert!(louder.abs() < 4.0, "{louder}");
}

#[test]
fn the_filter_takes_the_top_off() {
    let top = |cutoff: f32| {
        let out = left(&held(&[(param::CUTOFF, cutoff)], 45));
        level_db(&out, 30.0 * hz(45)) - level_db(&out, hz(45))
    };
    assert!(top(20_000.0) - top(400.0) > 30.0);
}

#[test]
fn env2_opens_the_filter_and_then_lets_it_close() {
    let mut synth = synth(&[
        (param::CUTOFF, 300.0),
        (param::ENV2_AMOUNT, 1.0),
        (param::ENV2_DEST, dest::CUTOFF as f32),
        (param::ENV2_DECAY, 1.0),
    ]);
    synth.note_on(45, 127);
    let early = left(&play(&mut synth, 2_400));
    play(&mut synth, 72_000);
    let late = left(&play(&mut synth, 2_400));
    let bright = |out: &[f32]| level_db(out, 20.0 * hz(45)) - level_db(out, hz(45));
    assert!(
        bright(&early) - bright(&late) > 12.0,
        "{} {}",
        bright(&early),
        bright(&late)
    );
}

#[test]
fn the_lfo_moves_the_wavetable_position() {
    let mut synth = synth(&[
        (OSC_A_POSITION, 0.5),
        (param::LFO_AMOUNT, 0.5),
        (param::LFO_RATE, 1.0),
        (param::LFO_DEST, dest::OSC_A_POSITION as f32),
    ]);
    synth.note_on(57, 127);
    let third = |out: &[f32]| level_db(out, 3.0 * hz(57)) - level_db(out, hz(57));
    // A quarter and three quarters of the way through the LFO's cycle.
    play(&mut synth, 10_800);
    let up = third(&left(&play(&mut synth, 2_400)));
    play(&mut synth, 21_600);
    let down = third(&left(&play(&mut synth, 2_400)));
    assert!((up - down).abs() > 6.0, "{up} {down}");
}

#[test]
fn a_released_note_falls_silent() {
    let mut synth = synth(&[(param::RELEASE, 0.1)]);
    synth.note_on(60, 127);
    play(&mut synth, 4_800);
    synth.note_off(60);
    play(&mut synth, 9_600);
    let tail = play(&mut synth, 4_800);
    assert!(tail.iter().all(|s| s.abs() < 1e-4));
}

#[test]
fn voices_limit_how_many_notes_sound_and_the_oldest_gives_way() {
    let mut mono = synth(&[(param::VOICES, 1.0)]);
    mono.note_on(57, 127);
    mono.note_on(64, 127);
    let out = left(&play(&mut mono, 9_600));
    assert!(level_db(&out, hz(64)) - level_db(&out, hz(57)) > 20.0);
    let mut poly = synth(&[]);
    poly.note_on(57, 127);
    poly.note_on(64, 127);
    let out = left(&play(&mut poly, 9_600));
    assert!((level_db(&out, hz(64)) - level_db(&out, hz(57))).abs() < 6.0);
}

#[test]
fn osc_b_joins_when_turned_up() {
    let alone = left(&held(&[], 57));
    let both = left(&held(
        &[
            (param::of(1, osc::LEVEL), 0.8),
            (param::of(1, osc::OCTAVE), 1.0),
        ],
        57,
    ));
    // Osc B an octave up adds to the second harmonic.
    let second = |out: &[f32]| level_db(out, 2.0 * hz(57));
    assert!(second(&both) - second(&alone) > 3.0);
}

#[test]
fn drive_adds_harmonics() {
    let third = |drive: f32| {
        let out = left(&held(&[(OSC_A_POSITION, 0.0), (param::DRIVE, drive)], 57));
        level_db(&out, 3.0 * hz(57)) - level_db(&out, hz(57))
    };
    assert!(third(1.0) - third(0.0) > 20.0);
}

#[test]
fn edits_glide_rather_than_step() {
    for (index, value) in [
        (OSC_A_POSITION, 0.0),
        (param::CUTOFF, 200.0),
        (param::OUTPUT_DB, 6.0),
    ] {
        let mut synth = synth(&[(param::CUTOFF, 4_000.0)]);
        synth.note_on(45, 127);
        let mut out = play(&mut synth, 4_800);
        synth.set_param(index, value);
        out.extend(play(&mut synth, 4_800));
        let out = left(&out);
        let jump = out[4_000..5_600]
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0f32, f32::max);
        // Steady before and after, the larger: a louder sound moves more.
        let steepest = |range: std::ops::Range<usize>| {
            out[range]
                .windows(2)
                .map(|w| (w[1] - w[0]).abs())
                .fold(0.0f32, f32::max)
        };
        let usual = steepest(3_000..4_600).max(steepest(8_000..9_600));
        assert!(jump < usual * 2.5 + 0.01, "param {index}: {jump} {usual}");
    }
}

#[test]
fn choices_snap_and_ranges_clamp() {
    let mut synth = WavetableSynth::new(RATE);
    synth.set_param(param::of(0, osc::TABLE), 2.6);
    assert_eq!(synth.get_param(param::of(0, osc::TABLE)), 3.0);
    synth.set_param(param::of(1, osc::UNISON), 20.0);
    assert_eq!(
        synth.get_param(param::of(1, osc::UNISON)),
        MAX_UNISON as f32
    );
    synth.set_param(param::LFO_DEST, 1.4);
    assert_eq!(synth.get_param(param::LFO_DEST), 1.0);
    assert!(!synth.set_param(param::CUTOFF, f32::NAN));
    assert!(!synth.set_param(param::COUNT, 0.0));
    assert_eq!(WAVETABLE_PARAMS.len(), param::COUNT);
}

#[test]
fn a_mono_buffer_gets_both_sides() {
    let mut synth = synth(&[]);
    synth.note_on(60, 127);
    let mut buffer = vec![0.0; 4_800];
    synth.render(&mut buffer, 1);
    assert!(buffer.iter().any(|s| s.abs() > 0.05));
}

#[test]
fn output_sets_the_level() {
    let quiet = rms_db(&left(&held(&[(param::OUTPUT_DB, -18.0)], 57)));
    let loud = rms_db(&left(&held(&[(param::OUTPUT_DB, -6.0)], 57)));
    assert!((loud - quiet - 12.0).abs() < 0.2, "{quiet} {loud}");
}

#[test]
fn reset_silences_everything() {
    let mut synth = synth(&[]);
    synth.note_on(60, 127);
    play(&mut synth, 2_400);
    synth.reset();
    assert!(play(&mut synth, 2_400).iter().all(|s| *s == 0.0));
}
