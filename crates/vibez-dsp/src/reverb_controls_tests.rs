use super::*;

const RATE: f32 = 48_000.0;

fn reverb(edits: &[(usize, f32)]) -> ReverbEffect {
    let mut fx = ReverbEffect::new(RATE);
    fx.set_param(param::MIX, 1.0);
    for &(index, value) in edits {
        fx.set_param(index, value);
    }
    fx
}

fn run(fx: &mut ReverbEffect, input: &[f32]) -> Vec<f32> {
    let mut output = input.to_vec();
    for block in output.chunks_mut(512) {
        fx.process(block, 2);
    }
    output
}

fn impulse(frames: usize) -> Vec<f32> {
    let mut buffer = vec![0.0; frames * 2];
    buffer[0] = 1.0;
    buffer[1] = 1.0;
    buffer
}

fn onset(output: &[f32]) -> usize {
    output.iter().position(|s| s.abs() > 1e-6).unwrap() / 2
}

fn energy(samples: &[f32]) -> f32 {
    samples.iter().map(|s| s * s).sum()
}

fn sine(frequency: f32, frames: usize) -> Vec<f32> {
    (0..frames)
        .flat_map(|frame| {
            let value = 0.5 * (std::f32::consts::TAU * frequency * frame as f32 / RATE).sin();
            [value, value]
        })
        .collect()
}

#[test]
fn pre_delay_moves_the_tail_later_by_its_length() {
    let plain = onset(&run(&mut reverb(&[]), &impulse(4000)));
    let delayed = onset(&run(
        &mut reverb(&[(param::PREDELAY_MS, 10.0)]),
        &impulse(4000),
    ));
    assert_eq!(delayed - plain, 480);
}

#[test]
fn synced_pre_delay_follows_the_tempo() {
    let plain = onset(&run(&mut reverb(&[]), &impulse(16_000)));
    let mut fx = reverb(&[(param::PREDELAY_SYNC, 5.0)]);
    fx.set_audio_context(DeviceAudioContext {
        musical_sample: 0,
        continuous_sample: 0,
        sample_rate: RATE as u32,
        bpm: 120.0,
        playing: true,
    });
    let delayed = onset(&run(&mut fx, &impulse(16_000)));
    assert_eq!(delayed - plain, 12_000, "an eighth note at 120 BPM");
}

#[test]
fn no_width_is_mono() {
    let output = run(&mut reverb(&[(param::WIDTH, 0.0)]), &impulse(8000));
    assert!(output.chunks(2).all(|frame| frame[0] == frame[1]));
}

#[test]
fn freeze_holds_the_tail_and_ignores_new_input() {
    let frames = 48_000;
    let mut held = impulse(frames * 3);
    let mut fed = held.clone();
    for (index, sample) in fed.iter_mut().enumerate().skip(frames) {
        *sample = if index % 7 == 0 { 0.8 } else { -0.2 };
    }
    let mut a = reverb(&[]);
    let mut b = reverb(&[]);
    let mut out_a = Vec::new();
    let mut out_b = Vec::new();
    for (block, (x, y)) in held.chunks_mut(512).zip(fed.chunks_mut(512)).enumerate() {
        // Freeze a quarter second in, before the new input starts at half.
        if block == 47 {
            a.set_param(param::FREEZE, 1.0);
            b.set_param(param::FREEZE, 1.0);
        }
        a.process(x, 2);
        b.process(y, 2);
        out_a.extend_from_slice(x);
        out_b.extend_from_slice(y);
    }
    let late = frames * 2 * 2;
    assert_eq!(&out_a[late..], &out_b[late..], "new input is shut out");
    let early = energy(&out_a[frames..frames + 4800]);
    let after = energy(&out_a[late..late + 4800]);
    assert!(after > early * 0.5, "the tail holds: {early} then {after}");
}

#[test]
fn the_cuts_trim_the_wet_signal() {
    let level = |edits: &[(usize, f32)], frequency: f32| {
        let output = run(&mut reverb(edits), &sine(frequency, 24_000));
        energy(&output[24_000..])
    };
    let lows = level(&[], 60.0) / level(&[(param::LOW_CUT_HZ, 600.0)], 60.0);
    assert!(lows > 16.0, "low cut takes {lows}x off 60 Hz");
    let highs = level(&[], 9000.0) / level(&[(param::HIGH_CUT_HZ, 1500.0)], 9000.0);
    assert!(highs > 16.0, "high cut takes {highs}x off 9 kHz");
}

#[test]
fn decay_seconds_and_room_size_convert_both_ways() {
    let mut previous = 0.0;
    for step in 0..=10 {
        let room = step as f32 / 10.0;
        let seconds = decay_seconds(room);
        assert!(seconds > previous);
        previous = seconds;
        assert!((room_for_decay(seconds) - room).abs() < 1e-4);
    }
    assert_eq!(room_for_decay(1000.0), 1.0);
    assert_eq!(room_for_decay(0.01), 0.0);
}

#[test]
fn bad_values_and_audio_cannot_poison_the_reverb() {
    let mut fx = reverb(&[]);
    assert!(!fx.set_param(param::MIX, f32::NAN));
    let mut buffer = vec![f32::NAN, f32::INFINITY, 1e30, -1e30];
    buffer.extend(sine(200.0, 2000));
    fx.process(&mut buffer, 2);
    assert!(buffer.iter().all(|s| s.is_finite()));
}
