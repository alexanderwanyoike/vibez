//! How much of one core a native instrument takes playing eight held voices
//! for 20 s, one frame at a time as the engine renders it, best of five
//! runs. Run with
//! `cargo run --release -p vibez-instruments --example instrument_cost -- Sampler`
//! (`DrumRack` for the rack, which holds sixteen looping pads instead,
//! `Wavetable` for the Wavetable Synth, `Fm` for the FM Synth, or no
//! argument for the Subtractive Synth).

use std::sync::Arc;
use std::time::{Duration, Instant};

use vibez_core::audio_buffer::DecodedAudio;
use vibez_core::midi::InstrumentKind;
use vibez_core::track::{DrumPadState, DRUM_RACK_BASE_NOTE};
use vibez_instruments::{drum_rack, fm, sampler, synth, wavetable};

/// Settings worth timing for each instrument: (label, parameter edits).
fn cases(kind: InstrumentKind) -> Vec<(&'static str, Vec<(usize, f32)>)> {
    match kind {
        InstrumentKind::Sampler | InstrumentKind::DrumRack => {
            use sampler::{filter_type, param};
            vec![
                ("defaults, looping", vec![(param::LOOP, 1.0)]),
                (
                    "looping with a crossfade, reverse, LP24, filter envelope, LFO on cutoff",
                    vec![
                        (param::LOOP, 1.0),
                        (param::CROSSFADE, 20.0),
                        (param::REVERSE, 1.0),
                        (param::FILTER_TYPE, filter_type::LP24 as f32),
                        (param::CUTOFF, 2_000.0),
                        (param::FILTER_ENV, 2.0),
                        (param::FILTER_SUSTAIN, 0.4),
                        (param::LFO_AMOUNT, 0.3),
                    ],
                ),
            ]
        }
        InstrumentKind::WavetableSynth => {
            use vibez_instruments::wavetable_params::osc;
            use wavetable::{dest, param};
            vec![
                ("defaults", vec![]),
                (
                    "both oscillators at 8 unison, warp, LP24, drive, LFO on position, Env 2 on cutoff",
                    vec![
                        (param::of(0, osc::UNISON), 8.0),
                        (param::of(0, osc::DETUNE), 20.0),
                        (param::of(0, osc::WARP), 0.4),
                        (param::of(1, osc::LEVEL), 0.6),
                        (param::of(1, osc::UNISON), 8.0),
                        (param::of(1, osc::DETUNE), 15.0),
                        (param::CUTOFF, 3_000.0),
                        (param::DRIVE, 0.3),
                        (param::LFO_AMOUNT, 0.3),
                        (param::ENV2_AMOUNT, 0.5),
                        (param::ENV2_DEST, dest::CUTOFF as f32),
                    ],
                ),
            ]
        }
        InstrumentKind::FmSynth => {
            use fm::{op, param};
            vec![
                ("defaults", vec![]),
                (
                    "all four operators sounding, feedback on each, Motion on brightness",
                    (0..fm::OPERATORS)
                        .flat_map(|operator| {
                            [
                                (param::of(operator, op::LEVEL), -6.0),
                                (param::of(operator, op::FEEDBACK), 0.5),
                            ]
                        })
                        .chain([(param::MOTION, 0.4)])
                        .collect(),
                ),
            ]
        }
        _ => {
            use synth::param;
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
    }
}

/// Two seconds of stereo noise at 44.1 kHz, so playback resamples.
fn sample() -> Arc<DecodedAudio> {
    let mut seed = 0x2545_f491_u32;
    let mut channel = || -> Vec<f32> {
        (0..88_200)
            .map(|_| {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                seed as f32 / u32::MAX as f32 - 0.5
            })
            .collect()
    };
    let left = channel();
    let right = channel();
    Arc::new(DecodedAudio {
        channels: vec![left, right],
        sample_rate: 44_100,
    })
}

fn run(kind: InstrumentKind, edits: &[(usize, f32)]) -> Duration {
    let rate = 48_000.0;
    let mut instrument = vibez_instruments::create_instrument(kind, rate);
    if kind == InstrumentKind::DrumRack {
        let mut params = drum_rack::pad_params(&DrumPadState::default());
        params[sampler::param::ONE_SHOT] = 0.0;
        for &(index, value) in edits {
            params[index] = value;
        }
        let sample = sample();
        for pad in 0..16 {
            drop(instrument.load_drum_pad_sample(pad, Arc::clone(&sample), "noise.wav".into()));
            drop(
                instrument.set_drum_pad_state(
                    pad,
                    DrumPadState {
                        params: params.to_vec(),
                        ..DrumPadState::default()
                    }
                    .into(),
                ),
            );
            instrument.note_on(DRUM_RACK_BASE_NOTE + pad as u8, 100);
        }
    } else {
        drop(instrument.load_sample(sample(), "noise.wav".into()));
        for &(index, value) in edits {
            instrument.set_param(index, value);
        }
        for pitch in [36, 48, 55, 60, 64, 67, 71, 74] {
            instrument.note_on(pitch, 100);
        }
    }
    let mut frame = [0.0f32; 2];
    let start = Instant::now();
    for _ in 0..(20.0 * rate) as usize {
        instrument.render(&mut frame, 2);
    }
    std::hint::black_box(frame);
    start.elapsed()
}

fn main() {
    let kind = match std::env::args().nth(1).as_deref() {
        Some("Sampler") => InstrumentKind::Sampler,
        Some("DrumRack") => InstrumentKind::DrumRack,
        Some("Wavetable") => InstrumentKind::WavetableSynth,
        Some("Fm") => InstrumentKind::FmSynth,
        _ => InstrumentKind::SubtractiveSynth,
    };
    for (label, edits) in cases(kind) {
        let best = (0..5).map(|_| run(kind, &edits)).min().unwrap_or_default();
        println!(
            "{} {label}: {:.2}% of one core",
            kind.name(),
            best.as_secs_f64() / 20.0 * 100.0
        );
    }
}
