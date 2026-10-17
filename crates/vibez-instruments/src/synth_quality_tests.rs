//! The flaws the original synth shipped with: aliasing oscillators, a
//! resonance that did nothing, a release far longer than its time, and
//! stealing whichever voice came first.

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

fn peak(samples: &[f32]) -> f32 {
    samples
        .iter()
        .fold(0.0, |peak, sample| peak.max(sample.abs()))
}

fn db(ratio: f32) -> f32 {
    20.0 * ratio.max(1e-9).log10()
}

/// Power that lands away from the note's harmonics, against the power on
/// them, in dB.
fn alias_db(samples: &[f32], frequency: f32) -> f32 {
    let n = samples.len();
    let window = |i: usize| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / n as f32).cos();
    let bin_hz = RATE / n as f32;
    let (mut on, mut off) = (0.0f64, 0.0f64);
    for bin in 1..n / 2 {
        let (mut re, mut im) = (0.0f64, 0.0f64);
        for (i, sample) in samples.iter().enumerate() {
            let angle = std::f64::consts::TAU * (bin * i) as f64 / n as f64;
            let value = f64::from(sample * window(i));
            re += value * angle.cos();
            im -= value * angle.sin();
        }
        let power = re * re + im * im;
        let hz = bin as f32 * bin_hz;
        let harmonic = (hz / frequency).round().max(1.0) * frequency;
        if (hz - harmonic).abs() <= 5.0 * bin_hz {
            on += power;
        } else {
            off += power;
        }
    }
    10.0 * (off / on).log10() as f32
}

#[test]
fn a_high_saw_does_not_alias() {
    let mut synth = synth(&[(5, 20_000.0)]);
    synth.note_on(98, 127);
    render(&mut synth, 2_000);
    let samples = render(&mut synth, 4_096);
    let frequency = 440.0 * 2f32.powf((98.0 - 69.0) / 12.0);
    let aliasing = alias_db(&samples, frequency);
    assert!(aliasing < -24.0, "aliasing {aliasing:.1} dB");
}

#[test]
fn resonance_lifts_the_cutoff() {
    let level = |resonance: f32| {
        // A sine sitting on the cutoff.
        let mut synth = synth(&[(0, 0.0), (5, 440.0), (6, resonance)]);
        synth.note_on(69, 127);
        render(&mut synth, 4_800);
        peak(&render(&mut synth, 4_800))
    };
    let lift = db(level(8.0) / level(0.707));
    assert!(lift > 12.0, "resonance lifts the cutoff by {lift:.1} dB");
}

#[test]
fn a_release_fades_to_silence_in_its_time() {
    let mut synth = synth(&[(0, 0.0), (4, 0.3)]);
    synth.note_on(57, 127);
    render(&mut synth, 9_600);
    let held = peak(&render(&mut synth, 480));
    synth.note_off(57);
    let window = (RATE * 0.01) as usize;
    let at = |synth: &mut SubtractiveSynth, seconds: f32, until: &mut f32| {
        render(synth, ((seconds - *until) * RATE) as usize - window);
        *until = seconds;
        db(peak(&render(synth, window)) / held)
    };
    let mut until = 0.0;
    let halfway = at(&mut synth, 0.15, &mut until);
    let end = at(&mut synth, 0.31, &mut until);
    assert!((-40.0..-20.0).contains(&halfway), "halfway {halfway:.1} dB");
    assert!(end < -55.0, "at its release time {end:.1} dB");
}

#[test]
fn a_full_synth_steals_its_oldest_voice() {
    let mut synth = synth(&[(4, 0.01)]);
    for pitch in 60..68 {
        synth.note_on(pitch, 100);
        render(&mut synth, 100);
    }
    // Each takes the oldest note's voice: 60, then 61.
    synth.note_on(72, 100);
    render(&mut synth, 100);
    synth.note_on(74, 100);
    render(&mut synth, 100);
    for pitch in (60..68).chain([74]) {
        synth.note_off(pitch);
    }
    render(&mut synth, 9_600);
    let still = peak(&render(&mut synth, 2_400));
    assert!(still > 0.05, "the held 72 was stolen: {still}");
}
