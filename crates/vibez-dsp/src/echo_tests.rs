use super::*;

const RATE: f32 = 48_000.0;

fn echo(edits: &[(usize, f32)]) -> EchoEffect {
    let mut echo = EchoEffect::new(RATE);
    for &(index, value) in [
        (param::SYNC, 0.0),
        (param::STYLE, style::CLEAN as f32),
        (param::DUCK, 0.0),
        (param::LOW_CUT, 20.0),
        (param::HIGH_CUT, 20_000.0),
        (param::TONE, 1.0),
        (param::MIX, 1.0),
    ]
    .iter()
    .chain(edits)
    {
        echo.set_param(index, value);
    }
    echo
}

#[test]
fn a_dry_mix_passes_the_input_untouched() {
    let mut echo = echo(&[(param::MIX, 0.0)]);
    let mut buffer = vec![0.5, -0.25, 0.1, 0.2];
    let input = buffer.clone();
    echo.process(&mut buffer, 2);
    assert_eq!(buffer, input);
}

#[test]
fn repeats_arrive_at_the_time_and_fall_by_the_feedback() {
    let mut echo = echo(&[(param::TIME_MS, 100.0), (param::FEEDBACK, 0.5)]);
    // A smoothly windowed 20 ms burst of 1 kHz: no DC for the low cut to
    // answer, and well inside every filter's pass band.
    let mut buffer: Vec<f32> = (0..16_000)
        .flat_map(|i| {
            let s = if i < 960 {
                let window = 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / 960.0).cos();
                (std::f32::consts::TAU * 1_000.0 * i as f32 / RATE).sin() * 0.5 * window
            } else {
                0.0
            };
            [s, s]
        })
        .collect();
    echo.process(&mut buffer, 2);
    let out: Vec<f32> = buffer.chunks(2).map(|frame| frame[0]).collect();
    let burst = |start: usize| {
        out[start..start + 960]
            .iter()
            .fold(0.0f32, |peak, s| peak.max(s.abs()))
    };
    let (first, second) = (burst(4_800), burst(9_600));
    assert!((first - 0.5).abs() < 0.02, "{first}");
    assert!((second / first - 0.5).abs() < 0.02, "{first} then {second}");
    assert!(burst(2_400) < 1e-3, "nothing between");
}

#[test]
fn a_synced_time_follows_the_tempo() {
    let mut params: Vec<f32> = ECHO_PARAMS.iter().map(|d| d.default).collect();
    // A dotted quarter at 120 BPM.
    assert_eq!(echo_time_ms(&params, 120.0), 750.0);
    params[param::SYNC] = 0.0;
    params[param::TIME_MS] = 250.0;
    assert_eq!(echo_time_ms(&params, 90.0), 250.0);
}

#[test]
fn ping_pong_alternates_the_sides() {
    let mut echo = echo(&[
        (param::TIME_MS, 50.0),
        (param::FEEDBACK, 0.7),
        (param::MODE, mode::PING_PONG as f32),
    ]);
    let mut buffer = vec![0.0; 12_000 * 2];
    buffer[0] = 1.0;
    buffer[1] = 1.0;
    echo.process(&mut buffer, 2);
    let side = |channel: usize, frame: usize| {
        buffer[(frame - 40) * 2..(frame + 40) * 2]
            .chunks(2)
            .fold(0.0f32, |peak, pair| peak.max(pair[channel].abs()))
    };
    assert!(side(0, 2_400) > 0.3 && side(1, 2_400) < 0.05);
    assert!(side(1, 4_800) > 0.2 && side(0, 4_800) < 0.05);
    assert!(side(0, 7_200) > 0.1 && side(1, 7_200) < 0.05);
}

/// Rising zero crossings a second of the echo of a sine.
fn echoed_frequency(edits: &[(usize, f32)]) -> f32 {
    let mut echo = echo(edits);
    let hz = 440.0;
    let frames = (RATE * 2.0) as usize;
    let mut buffer: Vec<f32> = (0..frames)
        .flat_map(|i| {
            let s = if i < frames / 4 {
                (std::f32::consts::TAU * hz * i as f32 / RATE).sin() * 0.5
            } else {
                0.0
            };
            [s, s]
        })
        .collect();
    echo.process(&mut buffer, 2);
    // The echo of the tone, a second after it started.
    let echoed: Vec<f32> = buffer[(frames / 2) * 2..(frames / 2 + frames / 8) * 2]
        .chunks(2)
        .map(|frame| frame[0])
        .collect();
    let rising = echoed
        .windows(2)
        .filter(|pair| pair[0] <= 0.0 && pair[1] > 0.0)
        .count();
    rising as f32 * RATE / echoed.len() as f32
}

#[test]
fn clean_repeats_keep_their_pitch_and_wow_moves_it() {
    let base = [(param::TIME_MS, 1_000.0), (param::FEEDBACK, 0.0)];
    let clean = echoed_frequency(&base);
    assert!((clean - 440.0).abs() < 3.0, "{clean}");
    let mut wobbly = base.to_vec();
    wobbly.extend([
        (param::STYLE, style::TAPE as f32),
        (param::CHARACTER, 1.0),
        (param::WOW, 1.0),
        (param::FLUTTER, 1.0),
    ]);
    let moved = echoed_frequency(&wobbly);
    assert!((moved - 440.0).abs() > 1.0, "{moved}");
}

#[test]
fn the_loop_stays_bounded_and_falls_silent_at_its_most_extreme() {
    let mut echo = echo(&[
        (param::TIME_MS, 30.0),
        (param::FEEDBACK, 0.98),
        (param::STYLE, style::DIFFUSE as f32),
        (param::CHARACTER, 1.0),
        (param::SATURATION, 1.0),
        (param::DIFFUSION, 1.0),
        (param::WEAR, 1.0),
        (param::WOW, 1.0),
    ]);
    let mut seed = 1u32;
    let mut loud: Vec<f32> = (0..9_600)
        .map(|_| {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            seed as f32 / u32::MAX as f32 * 2.0 - 1.0
        })
        .collect();
    echo.process(&mut loud, 2);
    let mut tail = vec![0.0; 48_000 * 2 * 20];
    echo.process(&mut tail, 2);
    assert!(tail.iter().all(|s| s.is_finite() && s.abs() < 2.0));
    let last = &tail[tail.len() - 4_800..];
    assert!(last.iter().all(|s| s.abs() < 0.01), "still ringing");
}

#[test]
fn the_plotted_repeats_fall_as_the_loop_does() {
    let mut params: Vec<f32> = ECHO_PARAMS.iter().map(|d| d.default).collect();
    params[param::STYLE] = style::CLEAN as f32;
    params[param::FEEDBACK] = 0.5;
    let taps = echo_taps_db(&params, 3);
    assert!((taps[0]).abs() < 1e-6);
    assert!((taps[1] + 6.02).abs() < 0.01);
    assert!((taps[2] + 12.04).abs() < 0.01);
}

#[test]
fn the_grid_lengthens_with_every_step() {
    let beats: Vec<f64> = GRIDS
        .iter()
        .map(|&(division, modifier)| crate::delay::synced_beats(division, modifier))
        .collect();
    assert!(beats.windows(2).all(|pair| pair[0] < pair[1]), "{beats:?}");
    assert_eq!(beats[DOTTED_QUARTER as usize], 1.5);
}

#[test]
fn bad_values_are_refused() {
    let mut echo = EchoEffect::new(RATE);
    assert!(!echo.set_param(param::FEEDBACK, f32::NAN));
    assert!(!echo.set_param(param::COUNT, 0.5));
    assert!(echo.set_param(param::FEEDBACK, 5.0));
    assert_eq!(echo.get_param(param::FEEDBACK), 0.98);
}

#[test]
fn the_soft_clip_follows_tanh_and_holds_at_one() {
    for i in -400..=400 {
        let x = i as f32 / 100.0;
        assert!((soft_clip(x) - x.tanh()).abs() < 0.025, "{x}");
    }
    assert_eq!(soft_clip(50.0), 1.0);
}

#[test]
fn the_tape_reads_between_frames_smoothly() {
    let mut tape = Tape::new(64);
    for i in 0..100 {
        tape.write(i as f32);
    }
    // A ramp read back at whole and half frames.
    assert_eq!(tape.read(1.0), 99.0);
    assert_eq!(tape.read(10.0), 90.0);
    assert!((tape.read(10.5) - 89.5).abs() < 1e-4);
}
