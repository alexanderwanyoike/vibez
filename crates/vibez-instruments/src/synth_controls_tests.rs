//! The controls added for the concept: the second oscillator, sub and
//! noise, filter types and envelope, the LFO, voice modes, voice count and
//! glide.

use super::*;

const RATE: f32 = 48_000.0;

fn synth(edits: &[(usize, f32)]) -> SubtractiveSynth {
    let mut synth = SubtractiveSynth::new(RATE);
    for &(index, value) in edits {
        synth.set_param(index, value);
    }
    synth
}

fn render(synth: &mut SubtractiveSynth, frames: usize) -> Vec<f32> {
    let mut buffer = vec![0.0; frames * 2];
    synth.render(&mut buffer, 2);
    buffer.chunks(2).map(|frame| frame[0]).collect()
}

fn rms(samples: &[f32]) -> f32 {
    (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt()
}

fn db(ratio: f32) -> f32 {
    20.0 * ratio.max(1e-9).log10()
}

/// Rising zero crossings a second.
fn frequency(samples: &[f32]) -> f32 {
    let rising = samples
        .windows(2)
        .filter(|pair| pair[0] <= 0.0 && pair[1] > 0.0)
        .count();
    rising as f32 * RATE / samples.len() as f32
}

/// The level of one frequency in `samples`.
fn level_at(samples: &[f32], hz: f32) -> f32 {
    let (mut re, mut im) = (0.0f32, 0.0f32);
    for (i, sample) in samples.iter().enumerate() {
        let angle = std::f32::consts::TAU * hz * i as f32 / RATE;
        re += sample * angle.cos();
        im += sample * angle.sin();
    }
    2.0 * (re * re + im * im).sqrt() / samples.len() as f32
}

const OPEN: (usize, f32) = (param::CUTOFF, 20_000.0);
const SINE: (usize, f32) = (param::WAVEFORM, 0.0);

#[test]
fn octave_and_detune_move_the_first_oscillator() {
    for (edits, expected) in [
        (vec![SINE, OPEN, (param::OCTAVE, 1.0)], 880.0),
        (vec![SINE, OPEN, (param::DETUNE, 7.0)], 659.26),
    ] {
        let mut synth = synth(&edits);
        synth.note_on(69, 127);
        render(&mut synth, 2_400);
        let measured = frequency(&render(&mut synth, 48_000));
        assert!(
            (measured - expected).abs() < 2.0,
            "{measured} vs {expected}"
        );
    }
}

#[test]
fn the_second_oscillator_sub_and_noise_join_in_at_their_levels() {
    let at = |edits: &[(usize, f32)], hz: f32| {
        let mut synth = synth(edits);
        synth.note_on(69, 127);
        render(&mut synth, 4_800);
        level_at(&render(&mut synth, 9_600), hz)
    };
    let base = [SINE, OPEN];
    let second = [
        SINE,
        OPEN,
        (param::WAVEFORM_2, 0.0),
        (param::OCTAVE_2, 1.0),
        (param::LEVEL_2, 0.5),
    ];
    assert!(at(&base, 880.0) < 0.001);
    assert!((at(&second, 880.0) / at(&second, 440.0) - 0.5).abs() < 0.05);
    let sub = [SINE, OPEN, (param::SUB, 1.0)];
    assert!((at(&sub, 220.0) / at(&sub, 440.0) - 1.0).abs() < 0.05);
    let mut noisy = synth(&[SINE, OPEN, (param::NOISE, 1.0)]);
    noisy.note_on(69, 127);
    render(&mut noisy, 4_800);
    let samples = render(&mut noisy, 9_600);
    assert!(level_at(&samples, 5_000.0) > 0.002, "noise is broadband");
}

#[test]
fn each_filter_type_keeps_its_band() {
    let level_q = |filter: usize, pitch: u8, q: f32| {
        let mut synth = synth(&[
            SINE,
            (param::CUTOFF, 1_000.0),
            (param::RESONANCE, q),
            (param::FILTER_TYPE, filter as f32),
        ]);
        synth.note_on(pitch, 127);
        render(&mut synth, 4_800);
        rms(&render(&mut synth, 9_600))
    };
    let level = |filter: usize, pitch: u8| level_q(filter, pitch, 0.707);
    // A3 (220 Hz) sits two octaves under the cutoff, A6 (1760 Hz) above it.
    let (low, high) = (57, 93);
    assert!(db(level(filter_type::LP12, high) / level(filter_type::LP12, low)) < -6.0);
    assert!(
        db(level(filter_type::LP24, high) / level(filter_type::LP12, high)) < -4.0,
        "24 dB falls further"
    );
    assert!(db(level(filter_type::HP12, low) / level(filter_type::HP12, high)) < -18.0);
    let band = |pitch: u8| level_q(filter_type::BP12, pitch, 2.0);
    let centre = band(83);
    assert!(db(band(low) / centre) < -10.0);
    assert!(db(band(105) / centre) < -10.0);
}

#[test]
fn the_filter_envelope_opens_the_cutoff_then_closes_it() {
    let mut synth = synth(&[
        (param::CUTOFF, 200.0),
        (param::FILTER_ENV, 4.0),
        (param::FILTER_ATTACK, 0.001),
        (param::FILTER_DECAY, 0.2),
        (param::FILTER_SUSTAIN, 0.0),
    ]);
    synth.note_on(45, 127);
    render(&mut synth, 480);
    let opened = render(&mut synth, 2_400);
    render(&mut synth, 24_000);
    let closed = render(&mut synth, 2_400);
    assert!(level_at(&opened, 1_320.0) > 4.0 * level_at(&closed, 1_320.0));
}

#[test]
fn the_lfo_moves_its_target() {
    let spread = |target: usize| {
        let mut synth = synth(&[
            SINE,
            OPEN,
            (param::LFO_TARGET, target as f32),
            (param::LFO_RATE, 4.0),
            (param::LFO_AMOUNT, 0.5),
        ]);
        synth.note_on(69, 127);
        render(&mut synth, 4_800);
        let pieces: Vec<Vec<f32>> = (0..8).map(|_| render(&mut synth, 1_500)).collect();
        let values: Vec<f32> = pieces
            .iter()
            .map(|piece| {
                if target == lfo_target::PITCH {
                    frequency(piece)
                } else {
                    rms(piece)
                }
            })
            .collect();
        let max = values.iter().copied().fold(f32::MIN, f32::max);
        let min = values.iter().copied().fold(f32::MAX, f32::min);
        max / min
    };
    assert!(spread(lfo_target::PITCH) > 1.3, "vibrato");
    assert!(spread(lfo_target::VOLUME) > 1.5, "tremolo");
    let mut steady = synth(&[SINE, OPEN]);
    steady.note_on(69, 127);
    render(&mut steady, 4_800);
    let a = rms(&render(&mut steady, 1_500));
    let b = rms(&render(&mut steady, 1_500));
    assert!((a / b - 1.0).abs() < 0.02);
}

#[test]
fn mono_plays_one_note_and_returns_to_the_one_still_held() {
    let mut synth = synth(&[SINE, OPEN, (param::VOICE_MODE, voice_mode::MONO as f32)]);
    synth.note_on(57, 127);
    render(&mut synth, 2_400);
    synth.note_on(69, 127);
    render(&mut synth, 2_400);
    let upper = render(&mut synth, 24_000);
    assert!((frequency(&upper) - 440.0).abs() < 3.0, "only the new note");
    synth.note_off(69);
    render(&mut synth, 2_400);
    let back = render(&mut synth, 24_000);
    assert!(
        (frequency(&back) - 220.0).abs() < 3.0,
        "back to the held note"
    );
}

#[test]
fn legato_changes_note_without_starting_the_envelope_again() {
    // Mono climbs from where it is back up to full, legato stays put.
    let soon_after = |mode: usize| {
        let mut synth = synth(&[
            SINE,
            OPEN,
            (param::VOICE_MODE, mode as f32),
            (param::ATTACK, 0.2),
        ]);
        synth.note_on(57, 127);
        render(&mut synth, 24_000);
        synth.note_on(69, 127);
        render(&mut synth, 2_400);
        rms(&render(&mut synth, 960))
    };
    let legato = soon_after(voice_mode::LEGATO);
    let mono = soon_after(voice_mode::MONO);
    assert!(
        mono > legato * 1.25,
        "mono attacks again: {mono} against {legato}"
    );
}

#[test]
fn glide_slides_between_notes_in_its_time() {
    let mut synth = synth(&[
        SINE,
        OPEN,
        (param::VOICE_MODE, voice_mode::LEGATO as f32),
        (param::GLIDE, 200.0),
    ]);
    synth.note_on(57, 127);
    render(&mut synth, 4_800);
    synth.note_on(69, 127);
    let start = frequency(&render(&mut synth, 2_400));
    render(&mut synth, 2_400);
    let middle = frequency(&render(&mut synth, 1_200));
    render(&mut synth, 4_800);
    let end = frequency(&render(&mut synth, 9_600));
    assert!(start < 280.0, "starts near the old note: {start}");
    assert!((280.0..400.0).contains(&middle), "halfway: {middle}");
    assert!((end - 440.0).abs() < 3.0, "arrives: {end}");
}

#[test]
fn the_voice_count_limits_polyphony() {
    let mut synth = synth(&[SINE, OPEN, (param::VOICES, 2.0), (param::RELEASE, 0.01)]);
    for pitch in [57, 64, 69] {
        synth.note_on(pitch, 127);
        render(&mut synth, 100);
    }
    synth.note_off(64);
    synth.note_off(69);
    render(&mut synth, 4_800);
    assert!(
        rms(&render(&mut synth, 2_400)) < 1e-4,
        "57 gave up its voice"
    );
}

#[test]
fn changing_the_voice_mode_keeps_held_notes_sounding() {
    let mut synth = synth(&[SINE, OPEN]);
    synth.note_on(57, 127);
    synth.note_on(64, 127);
    render(&mut synth, 4_800);
    let before = rms(&render(&mut synth, 2_400));
    synth.set_param(param::VOICE_MODE, voice_mode::MONO as f32);
    render(&mut synth, 4_800);
    let after = rms(&render(&mut synth, 2_400));
    assert!((after / before - 1.0).abs() < 0.05, "{before} then {after}");
}

#[test]
fn a_volume_move_glides_even_mid_legato() {
    let mut synth = synth(&[SINE, OPEN, (param::VOICE_MODE, voice_mode::LEGATO as f32)]);
    synth.note_on(57, 127);
    render(&mut synth, 4_800);
    synth.set_param(param::VOLUME, 0.1);
    synth.note_on(81, 127);
    let next = render(&mut synth, 60);
    let peak = next.iter().fold(0.0f32, |peak, s| peak.max(s.abs()));
    assert!(peak > 0.3, "no snap to the new volume: {peak}");
}

#[test]
fn bad_values_are_refused_and_bad_settings_stay_finite() {
    let mut synth = synth(&[]);
    for index in 0..param::COUNT {
        let before = synth.get_param(index);
        assert!(!synth.set_param(index, f32::NAN));
        assert_eq!(synth.get_param(index), before);
    }
    let mut synth = synth_extremes();
    for pitch in [0, 64, 127] {
        synth.note_on(pitch, 127);
    }
    let samples = render(&mut synth, 9_600);
    assert!(samples.iter().all(|sample| sample.is_finite()));
}

fn synth_extremes() -> SubtractiveSynth {
    let mut synth = SubtractiveSynth::new(RATE);
    for (index, descriptor) in SYNTH_PARAMS.iter().enumerate() {
        synth.set_param(index, descriptor.max);
    }
    synth
}
