use super::*;

const RATE: f32 = 48_000.0;

fn phaser(edits: &[(usize, f32)]) -> PhaserEffect {
    let mut fx = PhaserEffect::new(RATE);
    for &(index, value) in edits {
        fx.set_param(index, value);
    }
    fx
}

fn params(edits: &[(usize, f32)]) -> Vec<f32> {
    let mut params: Vec<f32> = PHASER_PARAMS.iter().map(|d| d.default).collect();
    for &(index, value) in edits {
        params[index] = value;
    }
    params
}

fn noise(frames: usize) -> Vec<f32> {
    let mut seed = 0x9e37_79b9_u32;
    (0..frames)
        .flat_map(|_| {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let v = ((seed >> 9) as f32 / (1u32 << 23) as f32 * 2.0 - 1.0) * 0.5;
            [v, v]
        })
        .collect()
}

#[test]
fn the_response_is_flat_with_no_mix_and_the_sweep_moves_up_with_the_lfo() {
    let dry = params(&[(param::MIX, 0.0)]);
    assert!(response_db(&dry, 0.5, 1000.0, RATE).abs() < 1e-4);
    let swept = params(&[]);
    assert!(break_hz(&swept, 1.0) > break_hz(&swept, 0.0) * 4.0);
    assert_eq!(break_hz(&swept, 0.0), 800.0);
}

#[test]
fn stereo_phase_splits_the_channels() {
    let input = noise(9600);
    let mut same = input.clone();
    phaser(&[]).process(&mut same, 2);
    assert!(same.chunks(2).all(|f| f[0] == f[1]));
    let mut split = input;
    phaser(&[(param::STEREO_PHASE, 90.0)]).process(&mut split, 2);
    assert!(split
        .chunks(2)
        .skip(4800)
        .any(|f| (f[0] - f[1]).abs() > 1e-3));
}

#[test]
fn the_triangle_lfo_is_linear_between_its_ends() {
    assert_eq!(lfo(1.0, 0.0), 0.0);
    assert_eq!(lfo(1.0, 0.25), 0.5);
    assert_eq!(lfo(1.0, 0.5), 1.0);
    assert!((lfo(0.0, 0.25) - 1.0).abs() < 1e-6);
}

#[test]
fn synced_rate_follows_the_tempo() {
    let mut fx = phaser(&[(param::SYNC, 4.0)]);
    fx.set_audio_context(DeviceAudioContext {
        musical_sample: 0,
        continuous_sample: 0,
        sample_rate: RATE as u32,
        bpm: 120.0,
        playing: true,
    });
    assert_eq!(fx.rate_hz(), 2.0, "one beat a cycle at 120 BPM");
}

#[test]
fn a_silent_cascade_never_produces_denormal_numbers() {
    let mut fx = phaser(&[(param::FEEDBACK, 0.9), (param::STAGES, 4.0)]);
    let mut buf = vec![0.0f32; 1024];
    buf[0] = 1.0;
    buf[1] = 1.0;
    fx.process(&mut buf, 2);
    let mut denormals = 0;
    for _ in 0..(10 * 48_000 / 512) {
        let mut silence = vec![0.0f32; 1024];
        fx.process(&mut silence, 2);
        denormals += silence.iter().filter(|s| s.is_subnormal()).count();
    }
    assert_eq!(denormals, 0);
}

#[test]
fn bad_values_and_audio_cannot_poison_the_phaser() {
    let mut fx = phaser(&[(param::FEEDBACK, 0.95), (param::STAGES, 4.0)]);
    assert!(!fx.set_param(param::RATE, f32::NAN));
    assert!(!fx.set_param(param::COUNT, 1.0));
    let mut buffer = vec![f32::NAN, f32::INFINITY, 1e30, -1e30];
    buffer.extend(noise(4000));
    fx.process(&mut buffer, 2);
    assert!(buffer.iter().all(|s| s.is_finite()));
}
