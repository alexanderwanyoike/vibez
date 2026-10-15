//! Checks declared Delay feedback stress, interpolation loss and transition quality gates.

use super::*;

const AMPLITUDE: f32 = 1.0 / 64.0;
const FEEDBACK: f32 = 0.95;
const MIXED: &[usize] = &[7, 31, 64, 1, 128, 512];

#[derive(Clone, Copy)]
enum Signal {
    Dc,
    Sine,
    HighSine,
    Impulse,
    Polarity,
    Transient,
}

impl Signal {
    fn name(self) -> &'static str {
        match self {
            Self::Dc => "dc",
            Self::Sine => "sine_fs_over_32",
            Self::HighSine => "sine_045fs",
            Self::Impulse => "left_impulse",
            Self::Polarity => "alternating_polarity",
            Self::Transient => "analytic_transients",
        }
    }

    fn sample(self, frame: usize, channel: usize, rate: u32) -> f32 {
        let sign = if channel == 0 { 1.0 } else { -1.0 };
        let value = match self {
            Self::Dc => sign,
            Self::Sine => {
                (std::f64::consts::TAU * frame as f64 / 32.0 + channel as f64 * 0.37).sin()
            }
            Self::HighSine => {
                (std::f64::consts::TAU * 0.45 * frame as f64 + channel as f64 * 0.37).sin()
            }
            Self::Impulse => f64::from(u8::from(frame == 0 && channel == 0)),
            Self::Polarity => sign * if frame % 2 == 0 { 1.0 } else { -1.0 },
            Self::Transient => {
                let period = (f64::from(rate) * 0.2).ceil() as usize;
                let time = (frame % period) as f64 / f64::from(rate);
                (-100.0 * time).exp()
                    * (std::f64::consts::TAU * 60.0 * time + channel as f64 * 0.37).sin()
            }
        };
        (f64::from(AMPLITUDE) * value) as f32
    }
}

fn span(rate: u32, seconds: f64) -> usize {
    (f64::from(rate) * seconds).ceil() as usize
}

fn prepare(
    rate: u32,
    times: [f32; 2],
    routing: f32,
    cuts: Option<[f32; 2]>,
    duck: f32,
) -> DelayEffect {
    let mut effect = prepared(rate, times[0], 1.0);
    for (index, value) in [
        (param::FEEDBACK, FEEDBACK),
        (param::LINK, 0.0),
        (param::RIGHT_TIME_MS, times[1]),
        (param::ROUTING, routing),
        (param::DUCK, duck),
    ] {
        assert!(effect.set_param(index, value));
    }
    if let Some([hp, lp]) = cuts {
        for (index, value) in [
            (param::FEEDBACK_HP_HZ, hp),
            (param::FEEDBACK_LP_HZ, lp),
            (param::FEEDBACK_HP_ENABLED, 1.0),
            (param::FEEDBACK_LP_ENABLED, 1.0),
        ] {
            assert!(effect.set_param(index, value));
        }
    }
    effect
}

fn fingerprint(mut hash: u64, sample: f32) -> u64 {
    for byte in sample.to_bits().to_le_bytes() {
        hash = (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3);
    }
    hash
}

struct Limits {
    peak: f64,
    energy_factor: f64,
}

struct Statistics {
    samples: usize,
    peak: f64,
    peak_frame: usize,
    energy: f64,
    input_energy: f64,
    drive_peak: f64,
    drive_energy: f64,
    drive_samples: usize,
    tail_peak: f64,
    tail_energy: f64,
    tail_samples: usize,
    nonfinite: usize,
    hash: u64,
}

impl Default for Statistics {
    fn default() -> Self {
        Self {
            samples: 0,
            peak: 0.0,
            peak_frame: 0,
            energy: 0.0,
            input_energy: 0.0,
            drive_peak: 0.0,
            drive_energy: 0.0,
            drive_samples: 0,
            tail_peak: 0.0,
            tail_energy: 0.0,
            tail_samples: 0,
            nonfinite: 0,
            hash: 0xcbf29ce484222325,
        }
    }
}

impl Statistics {
    fn observe(
        &mut self,
        frame: usize,
        input: f32,
        output: f32,
        drive_end: usize,
        tail_start: usize,
    ) {
        self.samples += 1;
        self.hash = fingerprint(self.hash, output);
        self.input_energy += f64::from(input).powi(2);
        if !output.is_finite() {
            self.nonfinite += 1;
            return;
        }
        let magnitude = f64::from(output).abs();
        let square = f64::from(output).powi(2);
        self.energy += square;
        if magnitude > self.peak {
            self.peak = magnitude;
            self.peak_frame = frame;
        }
        if frame < drive_end {
            self.drive_peak = self.drive_peak.max(magnitude);
            self.drive_energy += square;
            self.drive_samples += 1;
        }
        if frame >= tail_start {
            self.tail_peak = self.tail_peak.max(magnitude);
            self.tail_energy += square;
            self.tail_samples += 1;
        }
    }

    fn record(
        self,
        label: &str,
        rate: u32,
        parts: &[usize],
        limits: Limits,
        observation: Observation,
        rows: &mut Vec<Value>,
    ) -> (bool, u64) {
        let Limits {
            peak: peak_bound,
            energy_factor,
        } = limits;
        let drive_rms = (self.drive_energy / self.drive_samples as f64).sqrt();
        let tail_rms = (self.tail_energy / self.tail_samples as f64).sqrt();
        let energy_bound = energy_factor * self.input_energy / (1.0 - f64::from(FEEDBACK)).powi(2);
        let passed = self.nonfinite == 0
            && self.peak <= peak_bound
            && self.energy <= energy_bound
            && self.drive_peak > f64::from(AMPLITUDE) * 1e-3
            && self.tail_peak <= self.drive_peak * 1e-3 + 1e-10
            && tail_rms <= drive_rms * 1e-4 + 1e-10
            && observation.heap == [0; 3]
            && observation.rejected_expected_edits == 0
            && observation.target_mismatches == 0
            && observation.mismatches == 0;
        rows.push(json!({ "scenario": label, "rate": rate, "frames": self.samples / 2, "partitions": parts,
            "passed": passed, "peak": self.peak, "peak_frame": self.peak_frame, "peak_bound": peak_bound,
            "energy": self.energy, "input_energy": self.input_energy, "energy_bound": energy_bound,
            "drive_peak": self.drive_peak, "drive_rms": drive_rms, "tail_peak": self.tail_peak, "tail_rms": tail_rms,
            "nonfinite_samples": self.nonfinite, "runtime_fingerprint": format!("{:016x}", self.hash),
            "heap_alloc_realloc_dealloc": observation.heap, "control_errors": observation.rejected_expected_edits + observation.target_mismatches,
            "paired_duck_errors": observation.mismatches, "first_paired_error": observation.first_mismatch }));
        (passed, self.hash)
    }
}

fn stationary(rate: u32, signal: Signal, rows: &mut Vec<Value>) -> bool {
    let drive_end = rate as usize;
    let end = drive_end + span(rate, 240.0 * 0.029);
    let tail_start = end - span(rate, 0.25);
    let (routing, cuts, duck) = match signal {
        Signal::Dc | Signal::Sine => (0.0, None, 0.0),
        Signal::HighSine | Signal::Impulse => (1.0, None, 0.0),
        Signal::Polarity => (0.0, Some([1000.0, 1000.0]), 0.0),
        Signal::Transient => (1.0, Some([20.0, 20000.0]), 1.0),
    };
    let mut effect = prepare(rate, [17.0, 29.0], routing, cuts, duck);
    let mut statistics = Statistics::default();
    let mut observation = Observation::default();
    let mut scratch = [0.0; 1024];
    let (mut frame, mut part) = (0, 0);
    while frame < end {
        let count = MIXED[part % MIXED.len()].min(end - frame);
        let block = &mut scratch[..2 * count];
        for (i, sample) in block.iter_mut().enumerate() {
            *sample = if frame + i / 2 < drive_end {
                signal.sample(frame + i / 2, i % 2, rate)
            } else {
                0.0
            };
        }
        measured(|| effect.process(block, 2), &mut observation.heap);
        for (i, &sample) in block.iter().enumerate() {
            let at = frame + i / 2;
            let input = if at < drive_end {
                signal.sample(at, i % 2, rate)
            } else {
                0.0
            };
            statistics.observe(at, input, sample, drive_end, tail_start);
        }
        frame += count;
        part += 1;
    }
    let bound = if cuts.is_none() {
        f64::from(AMPLITUDE) / (1.0 - f64::from(FEEDBACK)) + 1e-6
    } else {
        1.0
    };
    let (passed, _) = statistics.record(
        signal.name(),
        rate,
        MIXED,
        Limits {
            peak: bound,
            energy_factor: 1.0001,
        },
        observation,
        rows,
    );
    let row = rows.last_mut().unwrap();
    row["routing"] = json!(routing);
    row["cuts_hz"] = json!(cuts);
    row["duck"] = json!(duck);
    row["times_ms"] = json!([17.0, 29.0]);
    row["silent_tail_frames"] = json!(end - drive_end);
    passed
}

fn moving_edits(
    effect: &mut DelayEffect,
    step: usize,
    final_target: bool,
    probe: bool,
    observation: &mut Observation,
) {
    let left = [3.0, 17.5, 41.0, 5.25][step % 4];
    let right = [5.0, 29.25, 43.0, 7.0][step % 4];
    let enabled = [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)][step % 4];
    let values = if final_target {
        [43.0, 29.0, 20.0, 1000.0, 1.0, 1.0, 1.0, 0.0]
    } else {
        [
            left,
            right,
            if step % 2 == 0 { 20.0 } else { 1000.0 },
            if step % 2 == 0 { 20000.0 } else { 1000.0 },
            enabled.0,
            enabled.1,
            (step % 2) as f32,
            (step % 2) as f32,
        ]
    };
    for (index, value) in [
        param::TIME_MS,
        param::RIGHT_TIME_MS,
        param::FEEDBACK_HP_HZ,
        param::FEEDBACK_LP_HZ,
        param::FEEDBACK_HP_ENABLED,
        param::FEEDBACK_LP_ENABLED,
        param::ROUTING,
        param::DUCK,
    ]
    .into_iter()
    .zip(values)
    {
        if !probe || index != param::DUCK {
            observation.edit(effect, index, value, false);
        }
    }
}

fn moving(rate: u32, glide: bool, parts: &[usize], rows: &mut Vec<Value>) -> (bool, u64) {
    let drive_end = 2 * rate as usize;
    let final_edit = span(rate, 1.8);
    let interval = span(rate, 0.007);
    let end = drive_end + span(rate, 240.0 * 0.043);
    let tail_start = end - span(rate, 0.25);
    let mut effect = prepare(rate, [3.0, 5.0], 0.0, None, 0.0);
    let mut probe = prepare(rate, [3.0, 5.0], 0.0, None, 0.0);
    for effect in [&mut effect, &mut probe] {
        assert!(effect.set_param(param::TIME_CHANGE, f32::from(u8::from(glide))));
    }
    let mut statistics = Statistics::default();
    let mut observation = Observation::default();
    let (mut audible, mut unducked) = ([0.0; 1024], [0.0; 1024]);
    let (mut frame, mut part, mut step, mut next_edit, mut settled) = (0, 0, 0, interval, false);
    let input = |at: usize, channel| {
        let signal = [
            Signal::Dc,
            Signal::Sine,
            Signal::HighSine,
            Signal::Polarity,
            Signal::Transient,
        ][(at / span(rate, 0.125)) % 5];
        if at < drive_end {
            signal.sample(at, channel, rate)
        } else {
            0.0
        }
    };
    while frame < end {
        if frame == final_edit {
            moving_edits(&mut effect, step, true, false, &mut observation);
            moving_edits(&mut probe, step, true, true, &mut observation);
            settled = true;
        } else if frame == next_edit && !settled {
            moving_edits(&mut effect, step, false, false, &mut observation);
            moving_edits(&mut probe, step, false, true, &mut observation);
            step += 1;
            next_edit += interval;
        }
        let boundary = if settled {
            end
        } else {
            next_edit.min(final_edit)
        };
        let count = parts[part % parts.len()].min(boundary - frame);
        for i in 0..2 * count {
            audible[i] = input(frame + i / 2, i % 2);
            unducked[i] = audible[i];
        }
        measured(
            || effect.process(&mut audible[..2 * count], 2),
            &mut observation.heap,
        );
        measured(
            || probe.process(&mut unducked[..2 * count], 2),
            &mut observation.heap,
        );
        for i in 0..2 * count {
            let at = frame + i / 2;
            statistics.observe(at, input(at, i % 2), unducked[i], drive_end, tail_start);
            let violates = !audible[i].is_finite()
                || audible[i].abs() > unducked[i].abs() + SAMPLE_TOLERANCE as f32
                || (at >= drive_end && (audible[i] - unducked[i]).abs() > SAMPLE_TOLERANCE as f32);
            if violates {
                observation.mismatches += 1;
                observation.first_mismatch.get_or_insert((
                    at,
                    i % 2,
                    f64::from(unducked[i]),
                    audible[i],
                ));
            }
        }
        frame += count;
        part += 1;
    }
    let result = statistics.record(
        if glide {
            "joint_glide_filters_routing_duck"
        } else {
            "joint_crossfade_filters_routing_duck"
        },
        rate,
        parts,
        Limits {
            peak: 64.0 * f64::from(AMPLITUDE),
            energy_factor: 16.0,
        },
        observation,
        rows,
    );
    let row = rows.last_mut().unwrap();
    row["last_edit_frame"] = json!(final_edit);
    row["input_stop_frame"] = json!(drive_end);
    row["silent_tail_frames"] = json!(end - drive_end);
    row["final_times_ms"] = json!([43.0, 29.0]);
    row["final_hp_lp_hz"] = json!([20.0, 1000.0]);
    result
}

#[test]
fn maximum_feedback_static_and_moving_networks_stay_bounded_and_decay_after_edits_stop() {
    let mut rows = Vec::new();
    let mut failures = 0;
    for rate in [1001, 48000] {
        for signal in [
            Signal::Dc,
            Signal::Sine,
            Signal::HighSine,
            Signal::Impulse,
            Signal::Polarity,
            Signal::Transient,
        ] {
            failures += usize::from(!stationary(rate, signal, &mut rows));
        }
    }
    let mut mixed_hashes = [0; 2];
    for rate in [1001, 48000, 192000] {
        for (index, glide) in [false, true].into_iter().enumerate() {
            let (passed, hash) = moving(rate, glide, MIXED, &mut rows);
            failures += usize::from(!passed);
            if rate == 48000 {
                mixed_hashes[index] = hash;
            }
        }
    }
    for (index, glide) in [false, true].into_iter().enumerate() {
        let (passed, hash) = moving(48000, glide, &[1], &mut rows);
        let same = hash == mixed_hashes[index];
        let row = rows.last_mut().unwrap();
        row["partition_peer_matches"] = json!(same);
        row["passed"] = json!(passed && same);
        failures += usize::from(!passed || !same);
    }
    assert_eq!(rows.len(), 20);
    println!(
        "DELAY_STRESS_RECORD {}",
        json!({
            "execution_head": std::env::var("VIBEZ_VERIFICATION_HEAD").unwrap_or_else(|_| "UNLABELLED".into()),
            "amplitude": AMPLITUDE, "feedback": FEEDBACK, "observed": rows,
            "scope": "Neutral infinity/fixed-system energy bounds; separate empirical moving ceilings, finite fixtures and240-period tails, no universal moving stability claim",
        })
    );
    assert_eq!(
        failures, 0,
        "Declared peak/energy/decay/partition/heap stress gates failed"
    );
}

fn interpolation_loss(rate: u32, rows: &mut Vec<Value>) -> bool {
    let time = if rate == 48000 { 1.03125 } else { 1.0078125 };
    let distance = f64::from(time) * f64::from(rate) / 1000.0;
    assert_eq!(distance.fract(), 0.5);
    let whole = distance.floor() as usize;
    let frames = 8 * whole + 9;
    let mut effect = prepare(rate, [time; 2], 0.0, None, 0.0);
    let mut audio = [0.0; 4096];
    audio[0] = 1.0;
    let mut observation = Observation::default();
    let (mut frame, mut part) = (0, 0);
    while frame < frames {
        let end = (frame + MIXED[part % MIXED.len()]).min(frames);
        measured(
            || effect.process(&mut audio[2 * frame..2 * end], 2),
            &mut observation.heap,
        );
        frame = end;
        part += 1;
    }
    let mut responses = Vec::new();
    let mut maximum_error = 0.0_f64;
    for repeat in [1, 4, 8] {
        for frequency in [0.01, 0.1, 0.25, 0.4, 0.45] {
            let omega = std::f64::consts::TAU * frequency;
            let kernel = (0.5 + 0.5 * omega.cos(), -0.5 * omega.sin());
            let mut power = (1.0, 0.0);
            for _ in 0..repeat {
                power = (
                    power.0 * kernel.0 - power.1 * kernel.1,
                    power.0 * kernel.1 + power.1 * kernel.0,
                );
            }
            let feedback = f64::from(FEEDBACK).powi(repeat as i32 - 1);
            let mut actual = (0.0, 0.0);
            for tap in 0..=repeat {
                let sample = f64::from(audio[2 * (repeat * whole + tap)]);
                actual.0 += sample * (omega * tap as f64).cos();
                actual.1 -= sample * (omega * tap as f64).sin();
            }
            let error = (actual.0 - feedback * power.0).hypot(actual.1 - feedback * power.1);
            maximum_error = maximum_error.max(error);
            let magnitude = actual.0.hypot(actual.1) / feedback;
            responses.push(json!({ "repeat": repeat, "frequency_over_rate": frequency,
                "complex_error": error, "normalized_interpolation_magnitude": magnitude,
                "measured_db_above_floor": (magnitude >= 1e-4).then(|| 20.0 * magnitude.log10()),
                "predicted_db": 20.0 * repeat as f64 * (std::f64::consts::PI * frequency).cos().abs().log10() }));
        }
    }
    let passed = maximum_error <= 5e-6
        && observation.heap == [0; 3]
        && audio[..2 * frames].iter().all(|sample| sample.is_finite())
        && audio[..2 * frames]
            .as_chunks::<2>()
            .0
            .iter()
            .all(|pair| pair[1] == 0.0);
    rows.push(json!({ "scenario": "measured_half_sample_interpolation_loss", "rate": rate,
        "distance_samples": distance, "frames": frames, "passed": passed,
        "maximum_complex_error": maximum_error, "complex_tolerance": 5e-6,
        "db_reporting_amplitude_floor": 1e-4, "heap_alloc_realloc_dealloc": observation.heap,
        "responses": responses, "limitation": "Linear interpolation compounds HF loss, not transparent or Hi-Fi" }));
    passed
}

fn carrier(frame: usize) -> f32 {
    (0.25 * (std::f64::consts::TAU * 1000.0 * frame as f64 / 48000.0).sin()) as f32
}

fn tap(frame: usize, delay: usize) -> f64 {
    frame
        .checked_sub(delay)
        .map(|at| f64::from(carrier(at)))
        .unwrap_or(0.0)
}

fn transition_residual(automated: bool, rows: &mut Vec<Value>) -> bool {
    let rate = 48000;
    let edit = 48012;
    let fade_frames = (rate as usize).div_ceil(50);
    let end = edit + fade_frames + 2;
    let mut effect = prepared(rate, 17.0, 1.0);
    let mut observation = Observation::default();
    let mut scratch = [0.0; 1024];
    let (mut frame, mut part) = (0, 0);
    let mut previous = 0.0;
    let mut maximum_residual = 0.0_f64;
    while frame < end {
        if frame == edit {
            observation.edit(&mut effect, param::TIME_MS, 29.5, automated);
        }
        let boundary = if frame < edit { edit } else { end };
        let count = MIXED[part % MIXED.len()].min(boundary - frame);
        for (i, sample) in scratch[..2 * count].iter_mut().enumerate() {
            *sample = carrier(frame + i / 2);
        }
        measured(
            || effect.process(&mut scratch[..2 * count], 2),
            &mut observation.heap,
        );
        for offset in 0..count {
            let at = frame + offset;
            let progress = if at < edit {
                0.0
            } else if automated {
                1.0
            } else {
                ((at - edit) as f64 / (fade_frames - 1) as f64).min(1.0)
            };
            let old = tap(at, 816);
            let new = tap(at, 1416);
            for channel in 0..2 {
                observation.sample(
                    at,
                    channel,
                    scratch[2 * offset + channel],
                    (1.0 - progress) * old + progress * new,
                );
            }
            let actual = f64::from(scratch[2 * offset]);
            if at >= edit {
                let fixed_tap_derivative = (1.0 - progress) * (old - tap(at - 1, 816))
                    + progress * (new - tap(at - 1, 1416));
                maximum_residual =
                    maximum_residual.max((actual - previous - fixed_tap_derivative).abs());
            }
            previous = actual;
        }
        frame += count;
        part += 1;
    }
    let manual_bound = 2.0 * 0.25 / (fade_frames - 1) as f64 + SAMPLE_TOLERANCE;
    let residual_passed = if automated {
        maximum_residual >= 0.25
    } else {
        maximum_residual <= manual_bound
    };
    let passed = residual_passed
        && observation.heap == [0; 3]
        && observation.mismatches == 0
        && observation.rejected_expected_edits == 0
        && observation.target_mismatches == 0;
    rows.push(json!({ "scenario": if automated { "opposing_peak_hard_jump_control" } else { "manual_crossfade_derivative_excess" },
        "rate": rate, "edit_frame": edit, "fade_frames": fade_frames, "passed": passed,
        "maximum_derivative_excess": maximum_residual, "manual_excess_bound": manual_bound,
        "hard_jump_required_minimum": 0.25, "sample_mismatches": observation.mismatches,
        "first_sample_mismatch": observation.first_mismatch, "heap_alloc_realloc_dealloc": observation.heap }));
    passed
}

#[test]
fn interpolation_loss_and_transition_excess_match_declared_quality_models() {
    let mut rows = Vec::new();
    let mut failures = 0;
    for rate in [48000, 192000] {
        failures += usize::from(!interpolation_loss(rate, &mut rows));
    }
    for automated in [false, true] {
        failures += usize::from(!transition_residual(automated, &mut rows));
    }
    assert_eq!(rows.len(), 4);
    println!(
        "DELAY_QUALITY_RECORD {}",
        json!({
            "execution_head": std::env::var("VIBEZ_VERIFICATION_HEAD").unwrap_or_else(|_| "UNLABELLED".into()),
            "oracle": "Half-sample FIR powers and weighted fixed-tap derivative bound; opposing-peak exact-automation control",
            "observed": rows, "scope": "Measured coloration and controlled transitions, not listening acceptance",
        })
    );
    assert_eq!(
        failures, 0,
        "Declared interpolation/transition gates failed"
    );
}

mod cpu;
