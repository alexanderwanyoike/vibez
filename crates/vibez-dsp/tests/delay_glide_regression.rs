//! Requires bounded constant-step Glide audio, actual-position retargets and explicit policy handoffs.

use serde_json::{json, Value};
use vibez_core::audio_context::DeviceAudioContext;
use vibez_dsp::{delay::DelayEffect, effect::AudioEffect};

const SAMPLE_TOLERANCE: f64 = 2.0e-7;
// Output-frame crossing brackets quantize a finite-window frequency estimate independently of sample error.
const PITCH_RELATIVE_TOLERANCE: f64 = 0.01;

#[derive(Clone, Copy)]
enum Signal {
    Rich,
    Sine80,
}

fn dry(frame: usize, channel: usize, rate: u32, signal: Signal) -> f32 {
    match signal {
        Signal::Rich => (((17 * frame + 29 * channel) % 97) as f32 - 48.0) / 128.0,
        Signal::Sine80 => {
            let phase = std::f64::consts::TAU * 80.0 * frame as f64 / f64::from(rate)
                + channel as f64 * 0.37;
            (phase.sin() * [0.25, 0.125][channel]) as f32
        }
    }
}

fn past_input(
    frame: usize,
    channel: usize,
    distance: f64,
    rate: u32,
    signal: Signal,
    reset: usize,
) -> f64 {
    let whole = distance.floor() as usize;
    let fraction = distance.fract();
    let sample = |delay| {
        frame
            .checked_sub(delay)
            .filter(|at| *at >= reset)
            .map(|at| f64::from(dry(at, channel, rate, signal)))
            .unwrap_or(0.0)
    };
    if fraction == 0.0 {
        sample(whole)
    } else {
        sample(whole) * (1.0 - fraction) + sample(whole + 1) * fraction
    }
}

#[derive(Clone, Copy)]
enum Motion {
    Hold,
    Glide,
    Crossfade,
}

#[derive(Clone, Copy)]
struct Span {
    at: usize,
    from: f64,
    to: f64,
    frames: usize,
    motion: Motion,
}

fn hold(at: usize, distance: f64) -> Span {
    Span {
        at,
        from: distance,
        to: distance,
        frames: 0,
        motion: Motion::Hold,
    }
}

fn glide_frames(rate: u32, from: f64, to: f64) -> usize {
    (0.05 * f64::from(rate)).max(2.0 * (to - from).abs()).ceil() as usize
}

fn glide(at: usize, from: f64, to: f64, rate: u32) -> Span {
    Span {
        at,
        from,
        to,
        frames: glide_frames(rate, from, to),
        motion: Motion::Glide,
    }
}

fn fade(at: usize, from: f64, to: f64, rate: u32) -> Span {
    Span {
        at,
        from,
        to,
        frames: (rate as usize).div_ceil(50),
        motion: Motion::Crossfade,
    }
}

fn expected(
    frame: usize,
    channel: usize,
    rate: u32,
    signal: Signal,
    spans: &[Span],
    resets: &[usize],
) -> f64 {
    let span = spans.iter().rfind(|span| span.at <= frame).unwrap();
    let reset = resets
        .iter()
        .copied()
        .rfind(|reset| *reset <= frame)
        .unwrap_or(0);
    let read = |distance| past_input(frame, channel, distance, rate, signal, reset);
    match span.motion {
        Motion::Hold => read(span.to),
        Motion::Glide => {
            let steps = frame - span.at + 1;
            let distance = if steps >= span.frames {
                span.to
            } else {
                span.from + (span.to - span.from) * steps as f64 / span.frames as f64
            };
            read(distance)
        }
        Motion::Crossfade => {
            let index = frame - span.at;
            if index == 0 {
                read(span.from)
            } else if index >= span.frames - 1 {
                read(span.to)
            } else {
                let weight = index as f64 / (span.frames - 1) as f64;
                read(span.from) * (1.0 - weight) + read(span.to) * weight
            }
        }
    }
}

#[derive(Clone, Copy)]
enum Edit {
    Manual(usize, f32),
    Automated(usize, f32),
    Bpm(f64),
    Reset,
}

#[derive(Clone, Copy)]
struct Event {
    at: usize,
    edit: Edit,
}

fn event(at: usize, edit: Edit) -> Event {
    Event { at, edit }
}

struct Scenario {
    name: &'static str,
    targets: [f32; 17],
    bpm: u32,
    signal: Signal,
    spans: [Vec<Span>; 2],
    events: Vec<Event>,
    frames: usize,
    endpoint_frames: Vec<usize>,
    pitch: Option<(usize, usize, f64)>,
}

fn targets(linked: bool, policy: f32) -> [f32; 17] {
    [
        10.25,
        0.0,
        1.0,
        0.0,
        0.0,
        0.0,
        0.0,
        u8::from(linked) as f32,
        17.5,
        1.0,
        0.0,
        0.0,
        120.0,
        0.0,
        8000.0,
        0.0,
        policy,
    ]
}

fn context(rate: u32, bpm: f64) -> DeviceAudioContext {
    DeviceAudioContext {
        musical_sample: 700_001,
        continuous_sample: 900_003,
        sample_rate: rate,
        bpm,
        playing: true,
    }
}

#[derive(Clone, Copy, Default)]
struct Crossings {
    previous: f32,
    count: usize,
    first: usize,
    last: usize,
}

fn observe(rate: u32, scenario: Scenario, mixed: bool) -> (Value, bool) {
    let mut effect = DelayEffect::new(rate as f32);
    let mut retained = scenario.targets;
    for (index, value) in retained.into_iter().enumerate() {
        assert!(effect.set_param(index, value));
    }
    effect.set_audio_context(context(rate, f64::from(scenario.bpm)));
    let parts: &[usize] = if mixed {
        &[1, 7, 31, 64, 128, 512]
    } else {
        &[scenario.frames]
    };
    let mut scratch = vec![0.0; 2 * parts.iter().copied().max().unwrap()];
    let resets: Vec<_> = scenario
        .events
        .iter()
        .filter_map(|event| matches!(event.edit, Edit::Reset).then_some(event.at))
        .collect();
    let mut witness_frames: Vec<_> = scenario
        .spans
        .iter()
        .flatten()
        .flat_map(|span| {
            [
                span.at,
                span.at + span.frames / 2,
                span.at + span.frames.saturating_sub(1),
            ]
        })
        .chain(scenario.endpoint_frames.iter().copied())
        .collect();
    witness_frames.sort_unstable();
    witness_frames.dedup();
    let mut maximum_error = [0.0_f64; 2];
    let mut mismatches = [0; 2];
    let mut nonfinite = [0; 2];
    let mut endpoint_mismatches = Vec::new();
    let mut target_mismatches = Vec::new();
    let mut witnesses = Vec::new();
    let mut crossings = [Crossings::default(); 2];
    let mut next_event = 0;
    let mut frame = 0;
    let mut part = 0;
    let mut accepted = true;
    while frame < scenario.frames {
        while next_event < scenario.events.len() && scenario.events[next_event].at == frame {
            match scenario.events[next_event].edit {
                Edit::Manual(index, value) => {
                    accepted &= effect.set_param(index, value);
                    retained[index] = value;
                }
                Edit::Automated(index, value) => {
                    accepted &= effect.set_automated_param(index, value);
                    retained[index] = value;
                }
                Edit::Bpm(bpm) => effect.set_audio_context(context(rate, bpm)),
                Edit::Reset => effect.reset(),
            }
            for (index, value) in retained.iter().enumerate() {
                let actual = effect.get_param(index).to_bits();
                if actual != value.to_bits() {
                    target_mismatches.push(json!({ "frame": frame, "index": index, "expected_bits": value.to_bits(), "actual_bits": actual }));
                }
            }
            next_event += 1;
        }
        let mut end = (frame + parts[part % parts.len()]).min(scenario.frames);
        if next_event < scenario.events.len() {
            end = end.min(scenario.events[next_event].at);
        }
        assert!(end > frame);
        let block = &mut scratch[..2 * (end - frame)];
        for (index, sample) in block.iter_mut().enumerate() {
            *sample = dry(frame + index / 2, index % 2, rate, scenario.signal);
        }
        effect.process(block, 2);
        for (index, &actual) in block.iter().enumerate() {
            let at = frame + index / 2;
            let channel = index % 2;
            let expected = expected(
                at,
                channel,
                rate,
                scenario.signal,
                &scenario.spans[channel],
                &resets,
            );
            let error = if actual.is_finite() {
                (f64::from(actual) - expected).abs()
            } else {
                f64::INFINITY
            };
            maximum_error[channel] = maximum_error[channel].max(error);
            mismatches[channel] += usize::from(error > SAMPLE_TOLERANCE);
            nonfinite[channel] += usize::from(!actual.is_finite());
            if scenario.endpoint_frames.contains(&at)
                && actual.to_bits() != (expected as f32).to_bits()
            {
                endpoint_mismatches.push(json!([at, channel]));
            }
            if witness_frames.binary_search(&at).is_ok() {
                witnesses.push(json!({ "frame": at, "channel": channel, "expected_amplitude": expected, "actual_bits": actual.to_bits() }));
            }
            if let Some((start, count, _)) = scenario.pitch {
                let crossing = &mut crossings[channel];
                if (start..start + count).contains(&at) {
                    if at > start && crossing.previous <= 0.0 && actual > 0.0 {
                        if crossing.count == 0 {
                            crossing.first = at;
                        }
                        crossing.count += 1;
                        crossing.last = at;
                    }
                    crossing.previous = actual;
                }
            }
        }
        frame = end;
        part += 1;
    }
    assert_eq!(next_event, scenario.events.len());
    let mut pitch_matches = true;
    let pitch: Vec<_> = crossings.into_iter().enumerate().map(|(channel, crossing)| {
        let frequency = (crossing.count >= 2).then(|| (crossing.count - 1) as f64 * f64::from(rate) / (crossing.last - crossing.first) as f64);
        if let Some((_, _, expected)) = scenario.pitch {
            pitch_matches &= frequency.is_some_and(|actual| (actual - expected).abs() / expected <= PITCH_RELATIVE_TOLERANCE);
        }
        json!({ "channel": channel, "positive_crossings": crossing.count, "first_frame": crossing.first,
            "last_frame": crossing.last, "estimated_hz": frequency, "nominal_expected_hz": scenario.pitch.map(|(_, _, hz)| hz) })
    }).collect();
    let schedules: Vec<_> = scenario.spans.iter().map(|spans| spans.iter().map(|span| {
        let mode = match span.motion { Motion::Hold => "hold", Motion::Glide => "glide", Motion::Crossfade => "crossfade" };
        json!({ "start_frame": span.at, "from_samples": span.from, "target_samples": span.to, "frames": span.frames,
            "mode": mode, "glide_step": matches!(span.motion, Motion::Glide).then(|| (span.to - span.from) / span.frames as f64) })
    }).collect::<Vec<_>>()).collect();
    let edits: Vec<_> = scenario
        .events
        .iter()
        .map(|event| {
            let edit = match event.edit {
                Edit::Manual(index, value) => {
                    json!({ "manual_index": index, "value_bits": value.to_bits() })
                }
                Edit::Automated(index, value) => {
                    json!({ "automated_index": index, "value_bits": value.to_bits() })
                }
                Edit::Bpm(bpm) => json!({ "bpm_bits": bpm.to_bits() }),
                Edit::Reset => json!({ "reset": true }),
            };
            json!({ "frame": event.at, "edit": edit })
        })
        .collect();
    let matches = mismatches == [0; 2]
        && nonfinite == [0; 2]
        && endpoint_mismatches.is_empty()
        && target_mismatches.is_empty()
        && pitch_matches
        && accepted;
    let signal = match scenario.signal {
        Signal::Rich => "binary_exact_period97_stereo",
        Signal::Sine80 => "80Hz_sine_amplitudes_0.25_0.125_right_phase_0.37",
    };
    (
        json!({ "scenario": scenario.name, "rate": rate, "input_signal": signal, "mixed_partitions": mixed, "frames": scenario.frames,
        "maximum_block_frames": scratch.len() / 2, "maximum_error": maximum_error, "mismatched_samples": mismatches,
        "nonfinite_samples": nonfinite, "endpoint_mismatches": endpoint_mismatches, "target_mismatches": target_mismatches,
        "all_edits_accepted": accepted, "oracle_schedules": schedules, "edits": edits, "witnesses": witnesses,
        "pitch": pitch, "matches": matches }),
        matches,
    )
}

fn finish(case: &str, rows: Vec<Value>, failures: Vec<String>) {
    println!(
        "DELAY_GLIDE_RECORD {}",
        json!({ "case": case, "sample_tolerance": SAMPLE_TOLERANCE,
        "pitch_relative_tolerance": PITCH_RELATIVE_TOLERANCE,
        "execution_head": std::env::var("VIBEZ_VERIFICATION_HEAD").unwrap_or_else(|_| "UNLABELLED".into()),
        "oracle": "Explicit piecewise distances and past dry interpolation, Feedback0; Glide uses k/N, k1..N; Crossfade uses i/(N-1); no production timing helper or metadata",
        "observed": rows })
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn positive_and_negative_glide_steps_reach_exact_targets_and_independently_predicted_pitch() {
    let mut rows = Vec::new();
    let mut failures = Vec::new();
    for rate in [1000, 1001, 48_000] {
        let w = (0.3 * f64::from(rate)).ceil() as usize + 3;
        for growing in [false, true] {
            let (old_ms, new_ms) = if growing {
                (25.0_f32, 225.0_f32)
            } else {
                (225.0, 25.0)
            };
            let old = f64::from(old_ms) * f64::from(rate) / 1000.0;
            let new = f64::from(new_ms) * f64::from(rate) / 1000.0;
            let n = glide_frames(rate, old, new);
            let step = (new - old) / n as f64;
            assert!(step.abs() <= 0.5);
            for mixed in [false, true] {
                let mut retained = targets(true, 1.0);
                retained[0] = old_ms;
                let spans = vec![hold(0, old), glide(w, old, new, rate)];
                let scenario = Scenario {
                    name: if growing {
                        "growing_distance_slows_pitch"
                    } else {
                        "shrinking_distance_raises_pitch"
                    },
                    targets: retained,
                    bpm: 120,
                    signal: Signal::Sine80,
                    spans: [spans.clone(), spans],
                    events: vec![
                        event(w, Edit::Manual(0, new_ms)),
                        event(w + n / 3, Edit::Manual(0, new_ms)),
                    ],
                    frames: w + n + 64,
                    endpoint_frames: vec![w + n - 1],
                    pitch: Some((w, n, 80.0 * (1.0 - step))),
                };
                let (row, matches) = observe(rate, scenario, mixed);
                if !matches {
                    failures.push(format!("rate={rate}, growing={growing}, mixed={mixed}"));
                }
                rows.push(row);
            }
        }
    }
    finish(
        "constant_bounded_steps_and_analytic_sine_pitch",
        rows,
        failures,
    );
}

fn control_scenarios(rate: u32, w: usize) -> Vec<Scenario> {
    let samples = |ms: f64| ms * f64::from(rate) / 1000.0;
    let (a, b, c) = (samples(10.25), samples(23.75), samples(7.5));
    let n = glide_frames(rate, a, b);
    let k = n / 3;
    let current = a + (b - a) * k as f64 / n as f64;
    let retarget_frames = glide_frames(rate, current, c);
    let nf = (rate as usize).div_ceil(50);
    let ng = glide_frames(rate, b, c);
    let policy_frame = w + nf + ng / 3;
    let interrupted = b + (c - b) * (ng / 3) as f64 / ng as f64;
    let mut sync = targets(false, 1.0);
    sync[3] = 1.0;
    sync[9] = 0.0;
    sync[10] = 1.0;
    vec![
        Scenario {
            name: "retarget_from_last_processed_distance_unrelated_right_automation_preserves_left",
            targets: targets(false, 1.0),
            bpm: 120,
            signal: Signal::Rich,
            spans: [
                vec![
                    hold(0, a),
                    glide(w, a, b, rate),
                    glide(w + k, current, c, rate),
                ],
                vec![hold(0, samples(17.5))],
            ],
            events: vec![
                event(w, Edit::Manual(0, 23.75)),
                event(w + k, Edit::Manual(0, 7.5)),
                event(w + k + retarget_frames / 2, Edit::Automated(7, 0.0)),
                event(w + k + 2 * retarget_frames / 3, Edit::Manual(0, 7.5)),
            ],
            frames: w + k + retarget_frames + 64,
            endpoint_frames: vec![w + k + retarget_frames - 1],
            pitch: None,
        },
        Scenario {
            name: "finish_crossfade_then_latest_glide_then_crossfade_from_actual_glide_tap",
            targets: targets(true, 0.0),
            bpm: 120,
            signal: Signal::Rich,
            spans: [
                vec![
                    hold(0, a),
                    fade(w, a, b, rate),
                    glide(w + nf, b, c, rate),
                    fade(policy_frame, interrupted, c, rate),
                    fade(policy_frame + nf, c, samples(16.25), rate),
                ],
                vec![
                    hold(0, a),
                    fade(w, a, b, rate),
                    glide(w + nf, b, c, rate),
                    fade(policy_frame, interrupted, c, rate),
                    fade(policy_frame + nf, c, samples(16.25), rate),
                ],
            ],
            events: vec![
                event(w, Edit::Manual(0, 23.75)),
                event(w + nf / 4, Edit::Manual(0, 31.5)),
                event(w + nf / 2, Edit::Manual(16, 1.0)),
                event(w + 3 * nf / 4, Edit::Manual(0, 7.5)),
                event(policy_frame, Edit::Manual(16, 0.0)),
                event(policy_frame + nf / 3, Edit::Manual(0, 16.25)),
            ],
            frames: policy_frame + 2 * nf + 64,
            endpoint_frames: vec![],
            pitch: None,
        },
        Scenario {
            name: "inactive_edits_and_free_tempo_keep_glides_exact_active_and_policy_cancel",
            targets: targets(false, 1.0),
            bpm: 120,
            signal: Signal::Rich,
            spans: [
                vec![hold(0, a), glide(w, a, b, rate), hold(w + n / 3, b)],
                vec![
                    hold(0, samples(17.5)),
                    glide(w, samples(17.5), samples(31.5), rate),
                    hold(w + n / 2, samples(31.5)),
                ],
            ],
            events: vec![
                event(w, Edit::Manual(0, 23.75)),
                event(w, Edit::Manual(8, 31.5)),
                event(w + n / 4, Edit::Manual(4, 6.0)),
                event(w + n / 4, Edit::Automated(9, 0.0)),
                event(w + n / 4, Edit::Bpm(240.0)),
                event(w + n / 3, Edit::Automated(0, 23.75)),
                event(w + n / 2, Edit::Automated(16, 1.0)),
            ],
            frames: w + 2 * n + 64,
            endpoint_frames: vec![],
            pitch: None,
        },
        Scenario {
            name:
                "inactive_free_and_unchanged_invalid_sync_tempo_preserve_glide_changed_tempo_snaps",
            targets: sync,
            bpm: 120,
            signal: Signal::Rich,
            spans: [
                vec![
                    hold(0, samples(31.25)),
                    glide(w, samples(31.25), samples(46.875), rate),
                    hold(w + 2 * n / 3, samples(23.4375)),
                ],
                vec![
                    hold(0, samples(46.875)),
                    glide(w, samples(46.875), samples(125.0 / 6.0), rate),
                    hold(w + 2 * n / 3, samples(125.0 / 12.0)),
                ],
            ],
            events: vec![
                event(w, Edit::Manual(5, 1.0)),
                event(w, Edit::Manual(10, 2.0)),
                event(w + n / 4, Edit::Automated(0, 23.75)),
                event(w + n / 4, Edit::Manual(8, 31.5)),
                event(w + n / 3, Edit::Bpm(120.0)),
                event(w + n / 2, Edit::Bpm(f64::NAN)),
                event(w + 2 * n / 3, Edit::Bpm(240.0)),
            ],
            frames: w + 2 * n + 64,
            endpoint_frames: vec![],
            pitch: None,
        },
    ]
}

#[test]
fn retarget_policy_handoffs_and_exact_control_ownership_follow_explicit_distance_schedules() {
    let mut rows = Vec::new();
    let mut failures = Vec::new();
    for rate in [1000, 1001, 48_000] {
        let w = (0.128 * f64::from(rate)).ceil() as usize + 3;
        for mixed in [false, true] {
            for scenario in control_scenarios(rate, w) {
                let name = scenario.name;
                let (row, matches) = observe(rate, scenario, mixed);
                if !matches {
                    failures.push(format!("scenario={name}, rate={rate}, mixed={mixed}"));
                }
                rows.push(row);
            }
        }
    }
    finish(
        "actual_position_retarget_handoffs_and_active_dependency_ownership",
        rows,
        failures,
    );
}

#[test]
fn longest_sync_glide_really_takes_seconds_with_a_half_sample_step() {
    let rate = 1000;
    let w = 1003;
    let (old, new) = (187.5, 18_000.0);
    let n = glide_frames(rate, old, new);
    assert_eq!(n, 35_625);
    assert_eq!((new - old) / n as f64, 0.5);
    let mut rows = Vec::new();
    let mut failures = Vec::new();
    for mixed in [false, true] {
        let mut retained = targets(true, 1.0);
        retained[3] = 1.0;
        let spans = vec![hold(0, old), glide(w, old, new, rate)];
        let scenario = Scenario {
            name: "whole_dotted_at20bpm_needs_35625_ms_of_motion",
            targets: retained,
            bpm: 20,
            signal: Signal::Rich,
            spans: [spans.clone(), spans],
            events: vec![
                event(w, Edit::Manual(4, 6.0)),
                event(w, Edit::Manual(5, 1.0)),
            ],
            frames: w + n + 64,
            endpoint_frames: vec![w + n - 1],
            pitch: None,
        };
        let (mut row, matches) = observe(rate, scenario, mixed);
        row["required_motion_seconds"] = json!(n as f64 / f64::from(rate));
        row["required_read_speed"] = json!(0.5);
        if !matches {
            failures.push(format!("long_sync_glide, mixed={mixed}"));
        }
        rows.push(row);
    }
    finish(
        "bounded_long_sync_motion_not_a_fifty_millisecond_snap",
        rows,
        failures,
    );
}

#[test]
fn glide_pre_audio_restoration_and_reset_snap_without_exposing_old_history() {
    let mut rows = Vec::new();
    let mut failures = Vec::new();
    for rate in [1000, 1001, 48_000] {
        let sample = |ms: f64| ms * f64::from(rate) / 1000.0;
        let w = (0.128 * f64::from(rate)).ceil() as usize + 3;
        let n = glide_frames(rate, sample(16.25), sample(31.5));
        let reset = w + n / 3;
        for mixed in [false, true] {
            let mut retained = targets(false, 1.0);
            retained[0] = 16.25;
            retained[8] = 23.5;
            let scenario = Scenario {
                name: "pre_audio_glide_restoration_and_reset_snap_targets",
                targets: retained,
                bpm: 120,
                signal: Signal::Rich,
                spans: [
                    vec![
                        hold(0, sample(16.25)),
                        glide(w, sample(16.25), sample(31.5), rate),
                        hold(reset, sample(31.5)),
                    ],
                    vec![
                        hold(0, sample(23.5)),
                        glide(w, sample(23.5), sample(7.5), rate),
                        hold(reset, sample(7.5)),
                    ],
                ],
                events: vec![
                    event(w, Edit::Manual(0, 31.5)),
                    event(w, Edit::Manual(8, 7.5)),
                    event(reset, Edit::Reset),
                ],
                frames: reset + (0.064 * f64::from(rate)).ceil() as usize + 64,
                endpoint_frames: vec![],
                pitch: None,
            };
            let (row, matches) = observe(rate, scenario, mixed);
            if !matches {
                failures.push(format!("rate={rate}, mixed={mixed}"));
            }
            rows.push(row);
        }
    }
    finish("glide_restoration_and_reset_owned_history", rows, failures);
}
