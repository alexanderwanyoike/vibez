//! How much of one core a built-in effect takes on 20 s of pulsing stereo
//! audio, best of five runs. Run with
//! `cargo run --release -p vibez-dsp --example effect_cost -- Compressor`.

use std::time::{Duration, Instant};

use vibez_core::effect::EffectType;

/// Settings worth timing for each effect: (label, parameter edits).
fn cases(effect: EffectType) -> Vec<(&'static str, Vec<(usize, f32)>)> {
    match effect {
        EffectType::Compressor => vec![
            ("defaults", vec![]),
            (
                "RMS, unlinked, auto release",
                vec![(11, 1.0), (12, 1.0), (13, 0.0)],
            ),
        ],
        EffectType::Filter => vec![
            ("defaults", vec![]),
            (
                "24 dB, LFO at 4 Hz",
                vec![(3, 1.0), (6, 1.0), (7, 4.0), (8, 2.0)],
            ),
            ("envelope follower", vec![(6, 2.0), (8, 2.0)]),
        ],
        EffectType::Gate => vec![
            ("defaults", vec![]),
            (
                "expander, range, both detector filters",
                vec![(5, 1.0), (7, 1.0), (9, 1.0), (11, 1.0)],
            ),
        ],
        EffectType::Drive => vec![
            ("defaults (Classic)", vec![]),
            ("Soft at 1x", vec![(4, 1.0), (5, 0.0)]),
            ("Soft at 2x", vec![(4, 1.0), (5, 1.0)]),
            (
                "Hard at 4x, both filters",
                vec![(4, 3.0), (5, 2.0), (6, 1.0), (8, 1.0)],
            ),
        ],
        EffectType::Bitcrush => vec![
            ("defaults", vec![]),
            (
                "8 bit, 1/4 rate, drive, dither, jitter, post filter",
                vec![(0, 8.0), (1, 4.0), (3, 6.0), (5, 1.0), (6, 0.5), (7, 1.0)],
            ),
        ],
        EffectType::Delay => vec![
            ("defaults", vec![]),
            (
                "ping-pong, unlinked, both filters, duck",
                vec![
                    (6, 1.0),
                    (7, 0.0),
                    (8, 375.0),
                    (11, 1.0),
                    (13, 1.0),
                    (15, 0.5),
                ],
            ),
        ],
        EffectType::Reverb => vec![
            ("defaults", vec![]),
            (
                "pre-delay, narrow, both cuts",
                vec![(3, 24.0), (5, 0.6), (7, 120.0), (8, 9500.0)],
            ),
        ],
        _ => vec![("defaults", vec![])],
    }
}

fn run(effect: EffectType, edits: &[(usize, f32)]) -> Duration {
    let rate = 48_000.0;
    let mut device = vibez_dsp::factory::create_effect(effect, rate);
    for &(index, value) in edits {
        device.set_automated_param(index, value);
    }
    let mut buffer = vec![0.0f32; 512 * 2];
    let mut phase = 0.0f32;
    let start = Instant::now();
    for _ in 0..(20.0 * rate / 512.0) as usize {
        for frame in buffer.chunks_mut(2) {
            phase += 0.01;
            let pulse = if (phase * 0.01).sin() > 0.0 { 1.0 } else { 0.2 };
            frame.fill(phase.sin() * 0.5 * pulse);
        }
        device.process(&mut buffer, 2);
    }
    start.elapsed()
}

fn main() {
    let name = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "Compressor".into());
    let Some(effect) = EffectType::all()
        .iter()
        .copied()
        .find(|effect| effect.name().eq_ignore_ascii_case(&name))
    else {
        eprintln!("unknown effect {name}");
        std::process::exit(2);
    };
    for (label, edits) in cases(effect) {
        let best = (0..5)
            .map(|_| run(effect, &edits))
            .min()
            .unwrap_or_default();
        println!(
            "{} {label}: {:.2}% of one core",
            effect.name(),
            best.as_secs_f64() / 20.0 * 100.0
        );
    }
}
