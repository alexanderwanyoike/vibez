//! How much of one core the subtractive synth takes playing eight held
//! voices for 20 s, one frame at a time as the engine renders it, best of
//! five runs. Run with
//! `cargo run --release -p vibez-instruments --example instrument_cost`.

use std::time::{Duration, Instant};

use vibez_instruments::synth::{param, SubtractiveSynth};

/// Settings worth timing: (label, parameter edits).
fn cases() -> Vec<(&'static str, Vec<(usize, f32)>)> {
    vec![
        ("defaults", vec![]),
        (
            "two oscillators, sub, noise, LP24, filter envelope, LFO on cutoff",
            vec![
                (param::LEVEL_2, 0.6),
                (param::SUB, 0.5),
                (param::NOISE, 0.2),
                (param::FILTER_TYPE, 1.0),
                (param::FILTER_ENV, 2.0),
                (param::FILTER_SUSTAIN, 0.4),
                (param::LFO_AMOUNT, 0.3),
            ],
        ),
    ]
}

fn run(edits: &[(usize, f32)]) -> Duration {
    let rate = 48_000.0;
    let mut synth = SubtractiveSynth::new(rate);
    for &(index, value) in edits {
        synth.set_param(index, value);
    }
    for pitch in [36, 48, 55, 60, 64, 67, 71, 74] {
        synth.note_on(pitch, 100);
    }
    let mut frame = [0.0f32; 2];
    let start = Instant::now();
    for _ in 0..(20.0 * rate) as usize {
        synth.render(&mut frame, 2);
    }
    std::hint::black_box(frame);
    start.elapsed()
}

fn main() {
    for (label, edits) in cases() {
        let best = (0..5).map(|_| run(&edits)).min().unwrap_or_default();
        println!(
            "Subtractive Synth {label}: {:.2}% of one core",
            best.as_secs_f64() / 20.0 * 100.0
        );
    }
}
