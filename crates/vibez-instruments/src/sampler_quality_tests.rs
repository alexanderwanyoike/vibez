//! The flaws the original sampler shipped with: playing a file at the
//! project's rate instead of its own, a release far longer than its time,
//! stealing whichever voice came first, and a click where a trimmed sample
//! stops.

use std::sync::Arc;

use vibez_core::audio_buffer::DecodedAudio;

use super::*;

const RATE: f32 = 48_000.0;

fn sampler_with(audio: DecodedAudio, edits: &[(usize, f32)]) -> Sampler {
    let mut sampler = Sampler::new(RATE);
    sampler.load_sample(Arc::new(audio), "test.wav".into());
    for &(index, value) in edits {
        sampler.set_param(index, value);
    }
    sampler
}

fn constant(seconds: f32, value: f32) -> DecodedAudio {
    let frames = (seconds * 44_100.0) as usize;
    DecodedAudio {
        channels: vec![vec![value; frames], vec![value; frames]],
        sample_rate: 44_100,
    }
}

fn render(sampler: &mut Sampler, frames: usize) -> Vec<f32> {
    let mut buffer = vec![0.0; frames * 2];
    sampler.render(&mut buffer, 2);
    buffer.chunks(2).map(|frame| frame[0]).collect()
}

fn peak(samples: &[f32]) -> f32 {
    samples.iter().fold(0.0, |peak, s| peak.max(s.abs()))
}

fn db(ratio: f32) -> f32 {
    20.0 * ratio.max(1e-9).log10()
}

#[test]
fn a_sample_plays_at_its_own_pitch_whatever_the_project_rate() {
    let frames = 44_100 * 2;
    let sine: Vec<f32> = (0..frames)
        .map(|i| (std::f32::consts::TAU * 440.0 * i as f32 / 44_100.0).sin())
        .collect();
    let audio = DecodedAudio {
        channels: vec![sine.clone(), sine],
        sample_rate: 44_100,
    };
    let mut sampler = sampler_with(audio, &[(P_ROOT_NOTE, 69.0)]);
    sampler.note_on(69, 127);
    render(&mut sampler, 4_800);
    let samples = render(&mut sampler, 48_000);
    let rising = samples
        .windows(2)
        .filter(|pair| pair[0] <= 0.0 && pair[1] > 0.0)
        .count() as f32;
    assert!((rising - 440.0).abs() < 2.0, "{rising} Hz");
}

#[test]
fn a_release_fades_to_silence_in_its_time() {
    let mut sampler = sampler_with(constant(4.0, 0.5), &[(P_RELEASE, 0.3)]);
    sampler.note_on(60, 127);
    render(&mut sampler, 9_600);
    let held = peak(&render(&mut sampler, 480));
    sampler.note_off(60);
    let window = (RATE * 0.01) as usize;
    render(&mut sampler, (0.15 * RATE) as usize - window);
    let halfway = db(peak(&render(&mut sampler, window)) / held);
    render(&mut sampler, (0.16 * RATE) as usize - window);
    let end = db(peak(&render(&mut sampler, window)) / held);
    assert!((-40.0..-20.0).contains(&halfway), "halfway {halfway:.1} dB");
    assert!(end < -55.0, "at its release time {end:.1} dB");
}

#[test]
fn a_full_sampler_steals_its_oldest_voice() {
    let mut sampler = sampler_with(constant(4.0, 0.5), &[(P_RELEASE, 0.01)]);
    for pitch in 60..68 {
        sampler.note_on(pitch, 100);
        render(&mut sampler, 100);
    }
    // Each takes the oldest note's voice: 60, then 61.
    sampler.note_on(72, 100);
    render(&mut sampler, 100);
    sampler.note_on(74, 100);
    render(&mut sampler, 100);
    for pitch in (60..68).chain([74]) {
        sampler.note_off(pitch);
    }
    render(&mut sampler, 9_600);
    assert!(
        peak(&render(&mut sampler, 2_400)) > 0.05,
        "the held 72 was stolen"
    );
}

#[test]
fn a_trimmed_sample_fades_out_at_its_end_rather_than_clicking() {
    let mut sampler = sampler_with(constant(1.0, 0.5), &[(P_END, 0.5)]);
    sampler.note_on(60, 127);
    let samples = render(&mut sampler, 48_000);
    let biggest_step = samples
        .windows(2)
        .skip(480)
        .map(|pair| (pair[1] - pair[0]).abs())
        .fold(0.0, f32::max);
    assert!(
        biggest_step < 0.02,
        "a {biggest_step:.3} jump where it stops"
    );
}
