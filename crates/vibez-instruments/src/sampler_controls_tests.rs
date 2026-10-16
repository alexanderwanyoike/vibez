//! The controls added for the concept: loop points, reverse and crossfade,
//! tuning and glide, the key and velocity zone, the filter and the LFO.

use std::sync::Arc;

use vibez_core::audio_buffer::DecodedAudio;

use super::*;

const RATE: f32 = 48_000.0;

fn audio(samples: Vec<f32>) -> Arc<DecodedAudio> {
    Arc::new(DecodedAudio {
        channels: vec![samples.clone(), samples],
        sample_rate: 48_000,
    })
}

fn sine(hz: f32, seconds: f32) -> Arc<DecodedAudio> {
    let frames = (seconds * RATE) as usize;
    audio(
        (0..frames)
            .map(|i| (std::f32::consts::TAU * hz * i as f32 / RATE).sin())
            .collect(),
    )
}

/// 0 rising to 1 over the sample.
fn ramp(frames: usize) -> Arc<DecodedAudio> {
    audio((0..frames).map(|i| i as f32 / frames as f32).collect())
}

fn sampler(sample: Arc<DecodedAudio>, edits: &[(usize, f32)]) -> Sampler {
    let mut sampler = Sampler::new(RATE);
    sampler.load_sample(sample, "test.wav".into());
    for &(index, value) in edits {
        sampler.set_param(index, value);
    }
    sampler
}

fn render(sampler: &mut Sampler, frames: usize) -> Vec<f32> {
    let mut buffer = vec![0.0; frames * 2];
    sampler.render(&mut buffer, 2);
    buffer.chunks(2).map(|frame| frame[0]).collect()
}

fn rms(samples: &[f32]) -> f32 {
    (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt()
}

fn frequency(samples: &[f32]) -> f32 {
    let rising = samples
        .windows(2)
        .filter(|pair| pair[0] <= 0.0 && pair[1] > 0.0)
        .count();
    rising as f32 * RATE / samples.len() as f32
}

const LOOP: (usize, f32) = (param::LOOP, 1.0);

#[test]
fn a_loop_stays_between_its_own_points() {
    // A 1 s ramp looping 0.4 to 0.6: past its first pass it only reads
    // values from that stretch.
    let mut sampler = sampler(
        ramp(48_000),
        &[LOOP, (param::LOOP_START, 0.4), (param::LOOP_END, 0.6)],
    );
    sampler.note_on(60, 127);
    render(&mut sampler, 48_000);
    let looped = render(&mut sampler, 48_000);
    let gain = 0.8;
    assert!(looped
        .iter()
        .all(|sample| (0.39 * gain..=0.61 * gain).contains(sample)));
}

#[test]
fn reverse_plays_from_the_end_backwards() {
    let mut sampler = sampler(ramp(48_000), &[(param::REVERSE, 1.0)]);
    sampler.note_on(60, 127);
    let samples = render(&mut sampler, 24_000);
    assert!(
        samples[1_000] > samples[20_000],
        "falls as the ramp is read backwards"
    );
    assert!(samples[1_000] > 0.7);
}

#[test]
fn a_crossfade_smooths_the_loop_wrap() {
    let biggest_step = |crossfade: f32| {
        let mut sampler = sampler(
            ramp(48_000),
            &[
                LOOP,
                (param::LOOP_START, 0.25),
                (param::LOOP_END, 0.75),
                (param::CROSSFADE, crossfade),
            ],
        );
        sampler.note_on(60, 127);
        render(&mut sampler, 48_000)
            .windows(2)
            .skip(100)
            .map(|pair| (pair[1] - pair[0]).abs())
            .fold(0.0, f32::max)
    };
    assert!(biggest_step(0.0) > 0.3, "the bare wrap jumps");
    assert!(biggest_step(50.0) < 0.01, "the blended wrap does not");
}

#[test]
fn tune_and_fine_move_the_pitch() {
    for (edits, expected) in [
        (vec![(param::TUNE, 12.0)], 880.0),
        (vec![(param::FINE, 100.0)], 466.16),
        (vec![(param::TUNE, -12.0), (param::FINE, -50.0)], 213.7),
    ] {
        let mut sampler = sampler(sine(440.0, 4.0), &edits);
        sampler.note_on(60, 127);
        render(&mut sampler, 2_400);
        let measured = frequency(&render(&mut sampler, 48_000));
        assert!(
            (measured - expected).abs() < 2.0,
            "{measured} vs {expected}"
        );
    }
}

#[test]
fn glide_slides_from_the_last_note() {
    let mut sampler = sampler(sine(440.0, 4.0), &[(param::GLIDE, 200.0)]);
    sampler.note_on(48, 127);
    render(&mut sampler, 4_800);
    sampler.note_on(60, 127);
    let start = frequency(&render(&mut sampler, 2_400));
    render(&mut sampler, 9_600);
    let end = frequency(&render(&mut sampler, 9_600));
    assert!(start < 300.0, "starts near the octave below: {start}");
    assert!((end - 440.0).abs() < 6.0, "arrives: {end}");
}

#[test]
fn notes_outside_the_zone_stay_silent() {
    let zone = [
        (param::KEY_LOW, 48.0),
        (param::KEY_HIGH, 72.0),
        (param::VELOCITY_LOW, 40.0),
        (param::VELOCITY_HIGH, 100.0),
    ];
    for (pitch, velocity, heard) in [
        (60, 80, true),
        (40, 80, false),
        (80, 80, false),
        (60, 20, false),
        (60, 120, false),
    ] {
        let mut sampler = sampler(sine(440.0, 1.0), &zone);
        sampler.note_on(pitch, velocity);
        let level = rms(&render(&mut sampler, 4_800));
        assert_eq!(level > 0.01, heard, "{pitch} at {velocity}");
    }
}

#[test]
fn velocity_amount_sets_how_much_velocity_matters() {
    let level = |amount: f32, velocity: u8| {
        let mut sampler = sampler(sine(440.0, 1.0), &[(param::VELOCITY_AMOUNT, amount)]);
        sampler.note_on(60, velocity);
        rms(&render(&mut sampler, 4_800))
    };
    assert!((level(0.0, 20) / level(0.0, 127) - 1.0).abs() < 0.01);
    assert!(level(1.0, 20) < level(1.0, 127) * 0.25);
}

#[test]
fn each_filter_type_keeps_its_band() {
    let level = |filter: usize, hz: f32| {
        let mut sampler = sampler(
            sine(hz, 1.0),
            &[
                (param::FILTER_TYPE, filter as f32),
                (param::CUTOFF, 1_000.0),
            ],
        );
        sampler.note_on(60, 127);
        render(&mut sampler, 4_800);
        rms(&render(&mut sampler, 9_600))
    };
    let open = level(filter_type::OFF, 4_000.0);
    assert!(level(filter_type::LP12, 4_000.0) < open * 0.2);
    assert!(level(filter_type::LP24, 4_000.0) < level(filter_type::LP12, 4_000.0) * 0.5);
    assert!(level(filter_type::HP12, 200.0) < level(filter_type::OFF, 200.0) * 0.1);
    assert!(level(filter_type::BP12, 1_000.0) > level(filter_type::BP12, 4_000.0) * 2.0);
}

#[test]
fn the_filter_envelope_opens_the_cutoff_then_closes_it() {
    let mut sampler = sampler(
        sine(2_000.0, 2.0),
        &[
            (param::FILTER_TYPE, filter_type::LP24 as f32),
            (param::CUTOFF, 200.0),
            (param::FILTER_ENV, 4.0),
            (param::FILTER_ATTACK, 0.001),
            (param::FILTER_DECAY, 0.2),
        ],
    );
    sampler.note_on(60, 127);
    render(&mut sampler, 480);
    let opened = rms(&render(&mut sampler, 2_400));
    render(&mut sampler, 24_000);
    let closed = rms(&render(&mut sampler, 2_400));
    assert!(opened > closed * 4.0, "{opened} then {closed}");
}

#[test]
fn the_lfo_makes_a_tremolo() {
    let mut sampler = sampler(
        sine(440.0, 2.0),
        &[
            (param::LFO_TARGET, lfo_target::VOLUME as f32),
            (param::LFO_RATE, 4.0),
            (param::LFO_AMOUNT, 0.5),
        ],
    );
    sampler.note_on(60, 127);
    render(&mut sampler, 4_800);
    let levels: Vec<f32> = (0..8).map(|_| rms(&render(&mut sampler, 1_500))).collect();
    let max = levels.iter().copied().fold(f32::MIN, f32::max);
    let min = levels.iter().copied().fold(f32::MAX, f32::min);
    assert!(max / min > 1.5);
}

#[test]
fn bad_values_are_refused_and_extremes_stay_finite() {
    let mut sampler = sampler(sine(440.0, 1.0), &[]);
    for index in 0..param::COUNT {
        let before = sampler.get_param(index);
        assert!(!sampler.set_param(index, f32::NAN));
        assert_eq!(sampler.get_param(index), before);
    }
    for (index, descriptor) in SAMPLER_PARAMS.iter().enumerate() {
        sampler.set_param(index, descriptor.max);
    }
    for pitch in [0, 64, 127] {
        sampler.note_on(pitch, 127);
    }
    assert!(render(&mut sampler, 9_600).iter().all(|s| s.is_finite()));
}
