//! Requires endpoint-inclusive fixed-tap Crossfades, bounded latest intent and exact timing ownership.

use serde_json::{json, Value};
use std::{alloc::{GlobalAlloc, Layout, System}, cell::Cell};
use vibez_core::audio_context::DeviceAudioContext;
use vibez_dsp::{delay::DelayEffect, effect::AudioEffect};

const SAMPLE_TOLERANCE: f64 = 2.0e-7;

#[derive(Clone, Copy, Default)]
struct AllocationRecord { active: bool, counts: [usize; 3] }

thread_local! {
    static ALLOCATIONS: Cell<AllocationRecord> = const { Cell::new(AllocationRecord { active: false, counts: [0; 3] }) };
}

fn count(kind: usize) {
    let _ = ALLOCATIONS.try_with(|cell| {
        let mut record = cell.get();
        if record.active { record.counts[kind] += 1; cell.set(record); }
    });
}

struct ObservedAllocator;

// Fixed Copy thread-local accounting neither allocates nor observes another test thread.
unsafe impl GlobalAlloc for ObservedAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        count(0);
        unsafe { System.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        count(0);
        unsafe { System.alloc_zeroed(layout) }
    }
    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        count(1);
        unsafe { System.realloc(pointer, layout, size) }
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        count(2);
        unsafe { System.dealloc(pointer, layout) };
    }
}

#[global_allocator]
static ALLOCATOR: ObservedAllocator = ObservedAllocator;

fn measured<T>(action: impl FnOnce() -> T, total: &mut [usize; 3]) -> T {
    ALLOCATIONS.set(AllocationRecord { active: true, counts: [0; 3] });
    let result = action();
    let observed = ALLOCATIONS.get();
    ALLOCATIONS.set(AllocationRecord::default());
    for (total, count) in total.iter_mut().zip(observed.counts) { *total += count; }
    result
}

fn context(rate: u32, bpm: f64) -> DeviceAudioContext {
    DeviceAudioContext { musical_sample: 700_001, continuous_sample: 900_003, sample_rate: rate, bpm, playing: true }
}

fn free_targets(linked: bool) -> [f32; 17] {
    [10.25, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, u8::from(linked) as f32,
        17.5, 1.0, 0.0, 0.0, 120.0, 0.0, 8000.0, 0.0, 0.0]
}

fn prepared(rate: u32, targets: [f32; 17]) -> DelayEffect {
    let mut effect = DelayEffect::new(rate as f32);
    for (index, value) in targets.into_iter().enumerate() { assert!(effect.set_param(index, value)); }
    effect.set_audio_context(context(rate, 120.0));
    effect
}

fn rich_input(frames: usize) -> Vec<f32> {
    (0..2 * frames).map(|index| {
        let frame = index / 2;
        let channel = index % 2;
        (((17 * frame + 29 * channel) % 97) as f32 - 48.0) / 128.0
    }).collect()
}

fn past_dry(input: &[f32], frame: usize, channel: usize, distance: f64) -> f64 {
    let whole = distance.floor() as usize;
    let fraction = distance.fract();
    let recent = frame.checked_sub(whole).map(|at| f64::from(input[2 * at + channel])).unwrap_or(0.0);
    if fraction == 0.0 { recent } else {
        let older = frame.checked_sub(whole + 1).map(|at| f64::from(input[2 * at + channel])).unwrap_or(0.0);
        recent * (1.0 - fraction) + older * fraction
    }
}

#[derive(Clone, Copy)]
struct Span { at: usize, old_ms: f64, new_ms: f64 }

fn span(at: usize, old_ms: f64, new_ms: f64) -> Span { Span { at, old_ms, new_ms } }

fn expected_sample(input: &[f32], frame: usize, channel: usize, rate: u32, fade_frames: usize, spans: &[Span]) -> f64 {
    let span = spans.iter().rfind(|span| span.at <= frame).unwrap();
    let progress = ((frame - span.at) as f64 / (fade_frames - 1) as f64).min(1.0);
    let old = span.old_ms * f64::from(rate) / 1000.0;
    let new = span.new_ms * f64::from(rate) / 1000.0;
    if span.old_ms == span.new_ms || progress == 1.0 { past_dry(input, frame, channel, new) }
    else if progress == 0.0 { past_dry(input, frame, channel, old) }
    else { past_dry(input, frame, channel, old) * (1.0 - progress) + past_dry(input, frame, channel, new) * progress }
}

#[derive(Clone, Copy)]
enum Edit { Manual(usize, f32), Automated(usize, f32), Bpm(f64) }

#[derive(Clone, Copy)]
struct Event { at: usize, edit: Edit }

fn event(at: usize, edit: Edit) -> Event { Event { at, edit } }

struct Scenario {
    label: &'static str,
    targets: [f32; 17],
    events: Vec<Event>,
    spans: [Vec<Span>; 2],
    frames: usize,
    exact_frames: Vec<usize>,
}

fn compare(actual: &[f32], input: &[f32], rate: u32, fade_frames: usize, spans: &[Vec<Span>; 2]) -> (Value, bool) {
    let mut maximum_error = [0.0_f64; 2];
    let mut mismatches = [0; 2];
    let mut nonfinite = [0; 2];
    for (index, actual) in actual.iter().enumerate() {
        let channel = index % 2;
        let expected = expected_sample(input, index / 2, channel, rate, fade_frames, &spans[channel]);
        let error = if actual.is_finite() { (f64::from(*actual) - expected).abs() } else { f64::INFINITY };
        maximum_error[channel] = maximum_error[channel].max(error);
        mismatches[channel] += usize::from(error > SAMPLE_TOLERANCE);
        nonfinite[channel] += usize::from(!actual.is_finite());
    }
    (json!({ "maximum_error": maximum_error, "mismatched_samples": mismatches, "nonfinite_samples": nonfinite }),
        mismatches == [0, 0] && nonfinite == [0, 0])
}

fn observe(rate: u32, fade_frames: usize, scenario: Scenario, mixed: bool) -> (Value, bool) {
    let input = rich_input(scenario.frames);
    let mut audio = input.clone();
    let mut effect = prepared(rate, scenario.targets);
    let mut targets = scenario.targets;
    let mut counts = [0; 3];
    let mut target_mismatches = Vec::new();
    let mut accepted = true;
    let mut next_event = 0;
    let mut frame = 0;
    let parts: &[usize] = if mixed { &[1, 7, 31, 64, 128, 512] } else { &[scenario.frames] };
    let mut part = 0;
    while frame < scenario.frames {
        while next_event < scenario.events.len() && scenario.events[next_event].at == frame {
            match scenario.events[next_event].edit {
                Edit::Manual(index, value) => {
                    accepted &= measured(|| effect.set_param(index, value), &mut counts);
                    targets[index] = value;
                }
                Edit::Automated(index, value) => {
                    accepted &= measured(|| effect.set_automated_param(index, value), &mut counts);
                    targets[index] = value;
                }
                Edit::Bpm(bpm) => measured(|| effect.set_audio_context(context(rate, bpm)), &mut counts),
            }
            for (index, value) in targets.iter().enumerate() {
                let actual = effect.get_param(index).to_bits();
                if actual != value.to_bits() {
                    target_mismatches.push(json!({ "frame": frame, "event": next_event, "index": index,
                        "expected_bits": value.to_bits(), "actual_bits": actual }));
                }
            }
            next_event += 1;
        }
        let mut end = (frame + parts[part % parts.len()]).min(scenario.frames);
        if next_event < scenario.events.len() { end = end.min(scenario.events[next_event].at); }
        assert!(end > frame);
        measured(|| effect.process(&mut audio[2 * frame..2 * end], 2), &mut counts);
        frame = end;
        part += 1;
    }
    assert_eq!(next_event, scenario.events.len());
    let (errors, audio_matches) = compare(&audio, &input, rate, fade_frames, &scenario.spans);
    let mut endpoint_mismatches = Vec::new();
    let mut witnesses = Vec::new();
    for frame in scenario.exact_frames {
        for channel in 0..2 {
            let expected = expected_sample(&input, frame, channel, rate, fade_frames, &scenario.spans[channel]) as f32;
            let actual = audio[2 * frame + channel];
            witnesses.push(json!({ "frame": frame, "channel": channel, "expected_bits": expected.to_bits(), "actual_bits": actual.to_bits() }));
            if expected.to_bits() != actual.to_bits() { endpoint_mismatches.push(json!([frame, channel])); }
        }
    }
    let oracle_spans: Vec<_> = scenario.spans.iter().map(|spans| spans.iter().map(|span|
        json!({ "start_frame": span.at, "old_ms": span.old_ms, "destination_ms": span.new_ms })
    ).collect::<Vec<_>>()).collect();
    let mut transition_witnesses = Vec::new();
    for (channel, spans) in scenario.spans.iter().enumerate() {
        for span in spans.iter().skip(1) {
            for frame in [span.at, span.at + fade_frames / 2, span.at + fade_frames - 1] {
                if frame < scenario.frames {
                    transition_witnesses.push(json!({ "frame": frame, "channel": channel,
                        "expected_amplitude": expected_sample(&input, frame, channel, rate, fade_frames, spans),
                        "actual_bits": audio[2 * frame + channel].to_bits() }));
                }
            }
        }
    }
    let edits: Vec<_> = scenario.events.iter().map(|event| {
        let edit = match event.edit {
            Edit::Manual(index, value) => json!({ "manual_index": index, "value_bits": value.to_bits() }),
            Edit::Automated(index, value) => json!({ "automated_index": index, "value_bits": value.to_bits() }),
            Edit::Bpm(bpm) => json!({ "bpm_context_bits": bpm.to_bits() }),
        };
        json!({ "frame": event.at, "edit": edit })
    }).collect();
    let matches = audio_matches && endpoint_mismatches.is_empty() && target_mismatches.is_empty() && accepted && counts == [0; 3];
    (json!({ "scenario": scenario.label, "rate": rate, "fade_frames": fade_frames,
        "mixed_partitions": mixed, "frames": scenario.frames, "errors": errors,
        "endpoint_witnesses": witnesses, "endpoint_mismatches": endpoint_mismatches,
        "transition_witnesses": transition_witnesses, "oracle_spans": oracle_spans, "edits": edits,
        "target_mismatches": target_mismatches, "all_edits_accepted": accepted,
        "callback_alloc_realloc_dealloc": counts, "matches": matches }), matches)
}

fn finish(case: &str, rows: Vec<Value>, failures: Vec<String>) {
    println!("DELAY_CROSSFADE_RECORD {}", json!({
        "case": case, "sample_tolerance": SAMPLE_TOLERANCE,
        "execution_head": std::env::var("VIBEZ_VERIFICATION_HEAD").unwrap_or_else(|_| "UNLABELLED".into()),
        "oracle": "Explicit piecewise fixed past-dry taps with inclusive i/(N-1) convex weights, zero feedback; no production timing helper or transition-state oracle",
        "observed": rows,
    }));
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn manual_crossfade_finishes_fixed_taps_then_only_the_latest_pending_target() {
    let mut rows = Vec::new();
    let mut failures = Vec::new();
    for rate in [1000, 1001, 48_000] {
        let n = (rate as usize).div_ceil(50);
        assert!(((2 * n) as f64) < 0.04 * f64::from(rate) + 2.0);
        let w = (0.128 * f64::from(rate)).ceil() as usize + 3;
        for pending_returns_to_destination in [false, true] {
            for mixed in [false, true] {
                let latest = if pending_returns_to_destination { 23.75 } else { 7.5 };
                let mut spans = vec![span(0, 10.25, 10.25), span(w, 10.25, 23.75)];
                if !pending_returns_to_destination { spans.push(span(w + n, 23.75, latest)); }
                let scenario = Scenario {
                    label: if pending_returns_to_destination { "latest_equals_active_destination" } else { "latest_replaces_older_pending" },
                    targets: free_targets(true),
                    events: vec![event(w, Edit::Manual(0, 23.75)), event(w + n / 4, Edit::Manual(0, 31.5)),
                        event(w + n / 2, Edit::Manual(0, latest as f32)), event(w + 3 * n / 4, Edit::Manual(0, latest as f32)),
                        event(w + n + n / 3, Edit::Manual(0, latest as f32))],
                    spans: [spans.clone(), spans], frames: w + 3 * n + 31,
                    exact_frames: vec![w, w + n - 1, w + 2 * n - 1],
                };
                let (mut row, matches) = observe(rate, n, scenario, mixed);
                row["required_latest_settle_frame"] = json!(w + if pending_returns_to_destination { n } else { 2 * n } - 1);
                row["literal_two_fade_bound_frames"] = json!(0.04 * f64::from(rate) + 2.0);
                if !matches { failures.push(format!("rate={rate}, returns_to_destination={pending_returns_to_destination}, mixed={mixed}")); }
                rows.push(row);
            }
        }
    }
    finish("fixed_taps_inclusive_endpoints_and_bounded_latest_pending", rows, failures);
}

fn ownership_scenarios(w: usize, n: usize) -> Vec<Scenario> {
    let mut sync_targets = free_targets(false);
    sync_targets[3] = 1.0;
    sync_targets[9] = 0.0;
    sync_targets[10] = 1.0;
    vec![
        Scenario {
            label: "active_left_exact_automation_then_policy_cancels_both",
            targets: free_targets(false),
            events: vec![event(w, Edit::Manual(0, 23.75)), event(w, Edit::Manual(8, 31.5)),
                event(w + n / 4, Edit::Automated(4, 6.0)), event(w + n / 4, Edit::Manual(9, 0.0)),
                event(w + n / 3, Edit::Automated(0, 23.75)), event(w + n / 2, Edit::Bpm(240.0)),
                event(w + 2 * n / 3, Edit::Automated(16, 0.0))],
            spans: [vec![span(0, 10.25, 10.25), span(w, 10.25, 23.75), span(w + n / 3, 23.75, 23.75)],
                vec![span(0, 17.5, 17.5), span(w, 17.5, 31.5), span(w + 2 * n / 3, 31.5, 31.5)]],
            frames: w + 4 * n + 31, exact_frames: vec![],
        },
        Scenario {
            label: "linked_right_intent_is_inactive_and_repeated_link_automation_owns_right_only",
            targets: free_targets(true),
            events: vec![event(w, Edit::Manual(0, 23.75)), event(w + n / 4, Edit::Automated(8, 31.5)),
                event(w + n / 4, Edit::Manual(9, 0.0)), event(w + n / 4, Edit::Automated(10, 1.0)),
                event(w + n / 3, Edit::Bpm(120.0)), event(w + n / 2, Edit::Manual(4, 6.0)),
                event(w + 2 * n / 3, Edit::Automated(7, 1.0))],
            spans: [vec![span(0, 10.25, 10.25), span(w, 10.25, 23.75)],
                vec![span(0, 10.25, 10.25), span(w, 10.25, 23.75), span(w + 2 * n / 3, 23.75, 23.75)]],
            frames: w + 4 * n + 31, exact_frames: vec![],
        },
        Scenario {
            label: "sync_inactive_free_and_unchanged_invalid_context_keep_fades_changed_bpm_snaps",
            targets: sync_targets,
            events: vec![event(w, Edit::Manual(5, 1.0)), event(w, Edit::Manual(10, 2.0)),
                event(w + n / 4, Edit::Automated(0, 23.75)), event(w + n / 4, Edit::Manual(8, 31.5)),
                event(w + n / 3, Edit::Bpm(120.0)), event(w + n / 2, Edit::Bpm(f64::NAN)),
                event(w + 2 * n / 3, Edit::Bpm(240.0))],
            spans: [vec![span(0, 31.25, 31.25), span(w, 31.25, 46.875), span(w + 2 * n / 3, 23.4375, 23.4375)],
                vec![span(0, 46.875, 46.875), span(w, 46.875, 125.0 / 6.0), span(w + 2 * n / 3, 125.0 / 12.0, 125.0 / 12.0)]],
            frames: w + 4 * n + 31, exact_frames: vec![],
        },
        Scenario {
            label: "manual_unlink_and_sync_follow_crossfade_without_changing_retained_values",
            targets: free_targets(true),
            events: vec![event(w, Edit::Manual(7, 0.0)), event(w + n / 3, Edit::Manual(8, 31.5)),
                event(w + n / 2, Edit::Manual(0, 23.75)), event(w + 2 * n, Edit::Manual(3, 1.0))],
            spans: [vec![span(0, 10.25, 10.25), span(w + n / 2, 10.25, 23.75), span(w + 2 * n, 23.75, 31.25)],
                vec![span(0, 10.25, 10.25), span(w, 10.25, 17.5), span(w + n, 17.5, 31.5), span(w + 2 * n, 31.5, 62.5)]],
            frames: w + 4 * n + 31, exact_frames: vec![],
        },
    ]
}

#[test]
fn exact_automation_and_context_cancel_only_their_active_timing_dependencies() {
    let mut rows = Vec::new();
    let mut failures = Vec::new();
    for rate in [1000, 1001, 48_000] {
        let n = (rate as usize).div_ceil(50);
        let w = (0.128 * f64::from(rate)).ceil() as usize + 3;
        for mixed in [false, true] {
            for scenario in ownership_scenarios(w, n) {
                let label = scenario.label;
                let (row, matches) = observe(rate, n, scenario, mixed);
                if !matches { failures.push(format!("scenario={label}, rate={rate}, mixed={mixed}")); }
                rows.push(row);
            }
        }
    }
    finish("crossfade_active_inactive_and_exact_control_ownership", rows, failures);
}

fn process_partitioned(effect: &mut DelayEffect, audio: &mut [f32], mixed: bool, counts: &mut [usize; 3]) {
    let parts: &[usize] = if mixed { &[1, 7, 31, 64, 128, 512] } else { &[audio.len() / 2] };
    let mut frame = 0;
    let mut part = 0;
    while frame < audio.len() / 2 {
        let end = (frame + parts[part % parts.len()]).min(audio.len() / 2);
        measured(|| effect.process(&mut audio[2 * frame..2 * end], 2), counts);
        frame = end;
        part += 1;
    }
}

#[test]
fn restoration_without_owned_audio_and_reset_snap_targets_without_callback_allocation() {
    let mut rows = Vec::new();
    let mut failures = Vec::new();
    for rate in [1000, 1001, 48_000] {
        let n = (rate as usize).div_ceil(50);
        let frames = (0.128 * f64::from(rate)).ceil() as usize + n + 31;
        for mixed in [false, true] {
            let mut effect = prepared(rate, free_targets(false));
            let mut counts = [0; 3];
            let payload = 0x7fc1_2345;
            let mut unowned = [f32::from_bits(payload)];
            measured(|| effect.process(&mut unowned, 0), &mut counts);
            measured(|| effect.process(&mut [], 2), &mut counts);
            measured(|| effect.process(&mut unowned, 2), &mut counts);
            let mut accepted = true;
            for (index, value) in [(0, 23.75), (8, 31.5)] { accepted &= measured(|| effect.set_param(index, value), &mut counts); }
            let input = rich_input(frames);
            let mut before = input.clone();
            process_partitioned(&mut effect, &mut before, mixed, &mut counts);
            let (restoration_errors, restoration_matches) = compare(&before, &input, rate, n,
                &[vec![span(0, 23.75, 23.75)], vec![span(0, 31.5, 31.5)]]);
            for (index, value) in [(0, 7.5), (8, 11.25)] { accepted &= measured(|| effect.set_param(index, value), &mut counts); }
            let mut moving = rich_input(n / 3);
            process_partitioned(&mut effect, &mut moving, mixed, &mut counts);
            for (index, value) in [(0, 13.25), (8, 19.75)] { accepted &= measured(|| effect.set_param(index, value), &mut counts); }
            measured(|| effect.reset(), &mut counts);
            let mut after = input.clone();
            process_partitioned(&mut effect, &mut after, mixed, &mut counts);
            let (reset_errors, reset_matches) = compare(&after, &input, rate, n,
                &[vec![span(0, 13.25, 13.25)], vec![span(0, 19.75, 19.75)]]);
            let target_bits = [effect.get_param(0).to_bits(), effect.get_param(8).to_bits()];
            let targets_match = target_bits == [13.25_f32.to_bits(), 19.75_f32.to_bits()];
            let matches = restoration_matches && reset_matches && unowned[0].to_bits() == payload
                && accepted && targets_match && counts == [0; 3];
            rows.push(json!({ "rate": rate, "fade_frames": n, "mixed_partitions": mixed,
                "restoration_errors": restoration_errors, "reset_errors": reset_errors,
                "unowned_bits": unowned[0].to_bits(), "post_reset_target_bits": target_bits,
                "callback_alloc_realloc_dealloc": counts, "all_edits_accepted": accepted, "matches": matches }));
            if !matches { failures.push(format!("rate={rate}, mixed={mixed}")); }
        }
    }
    finish("restoration_reset_and_owned_frame_lifecycle", rows, failures);
}
