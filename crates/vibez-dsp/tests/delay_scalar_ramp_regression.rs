//! Requires sample-rate Mix ramps and exact automation without disturbing Delay timing.

use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
};

use serde_json::{json, Value};
use vibez_core::audio_context::DeviceAudioContext;
use vibez_dsp::{delay::{param, DelayEffect}, effect::AudioEffect};

const SAMPLE_TOLERANCE: f64 = 2.0e-7;

#[derive(Clone, Copy, Default)]
struct AllocationRecord {
    active: bool,
    counts: [usize; 3],
}

thread_local! {
    static ALLOCATIONS: Cell<AllocationRecord> = const {
        Cell::new(AllocationRecord { active: false, counts: [0; 3] })
    };
}

fn count(kind: usize) {
    let _ = ALLOCATIONS.try_with(|cell| {
        let mut record = cell.get();
        if record.active {
            record.counts[kind] += 1;
            cell.set(record);
        }
    });
}

struct ObservedAllocator;

// Copy thread-local counters exclude fixture/report allocation and other test threads.
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
    for (total, count) in total.iter_mut().zip(observed.counts) {
        *total += count;
    }
    result
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

fn prepared(rate: u32, time_ms: f32, mix: f32) -> DelayEffect {
    let mut effect = DelayEffect::new(rate as f32);
    assert!(effect.processing_configuration_valid());
    for (index, value) in [(param::TIME_MS, time_ms), (param::FEEDBACK, 0.0), (param::MIX, mix)] {
        assert!(effect.set_param(index, value));
    }
    effect.set_audio_context(context(rate, 120.0));
    effect
}

#[derive(Default)]
struct Observation {
    heap: [usize; 3],
    mismatches: usize,
    maximum_error: f64,
    rejected_expected_edits: usize,
    target_mismatches: usize,
    exact_endpoint_mismatches: usize,
    first_mismatch: Option<(usize, usize, f64, f32)>,
}

impl Observation {
    fn sample(&mut self, frame: usize, channel: usize, actual: f32, expected: f64) {
        let error = if actual.is_finite() {
            (f64::from(actual) - expected).abs()
        } else {
            f64::INFINITY
        };
        self.maximum_error = self.maximum_error.max(error);
        if error > SAMPLE_TOLERANCE {
            self.mismatches += 1;
            self.first_mismatch.get_or_insert((frame, channel, expected, actual));
        }
    }

    fn edit(&mut self, effect: &mut DelayEffect, index: usize, value: f32, automated: bool) {
        let accepted = measured(|| {
            if automated { effect.set_automated_param(index, value) }
            else { effect.set_param(index, value) }
        }, &mut self.heap);
        self.rejected_expected_edits += usize::from(!accepted);
        self.target_mismatches += usize::from(effect.get_param(index).to_bits() != value.to_bits());
    }

    fn finish(self, scenario: &str, rate: u32, parts: &[usize], rows: &mut Vec<Value>) -> bool {
        let matches = self.heap == [0; 3] && self.mismatches == 0
            && self.rejected_expected_edits == 0 && self.target_mismatches == 0
            && self.exact_endpoint_mismatches == 0;
        rows.push(json!({
            "scenario": scenario, "rate": rate, "partitions": parts,
            "ramp_frames": (rate as usize).div_ceil(100), "matches": matches,
            "heap_alloc_realloc_dealloc": self.heap, "sample_mismatches": self.mismatches,
            "maximum_error": self.maximum_error, "first_mismatch": self.first_mismatch,
            "unexpected_rejected_edits": self.rejected_expected_edits,
            "target_bit_mismatches": self.target_mismatches,
            "exact_endpoint_mismatches": self.exact_endpoint_mismatches,
        }));
        matches
    }
}

fn render(
    effect: &mut DelayEffect,
    frames: std::ops::Range<usize>,
    parts: &[usize],
    input: impl Fn(usize, usize) -> f32,
    expected: impl Fn(usize, usize) -> f64,
    observation: &mut Observation,
) {
    let mut scratch = [0.0; 1024];
    let mut frame = frames.start;
    let mut part = 0;
    while frame < frames.end {
        let end = (frame + parts[part % parts.len()]).min(frames.end);
        let block = &mut scratch[..2 * (end - frame)];
        for (index, sample) in block.iter_mut().enumerate() {
            *sample = input(frame + index / 2, index % 2);
        }
        measured(|| effect.process(block, 2), &mut observation.heap);
        for (index, &actual) in block.iter().enumerate() {
            let at = frame + index / 2;
            observation.sample(at, index % 2, actual, expected(at, index % 2));
        }
        frame = end;
        part += 1;
    }
}

fn dry(_: usize, channel: usize) -> f32 {
    [0.25, -0.125][channel]
}

fn dry_mixed(channel: usize, mix: f64) -> f64 {
    f64::from(dry(0, channel)) * (1.0 - mix) + 0.0 * mix
}

fn exact_dry_frame(effect: &mut DelayEffect, frame: usize, mix: f64, observation: &mut Observation) {
    let mut block = [dry(0, 0), dry(0, 1)];
    measured(|| effect.process(&mut block, 2), &mut observation.heap);
    for (channel, actual) in block.into_iter().enumerate() {
        let expected = dry_mixed(channel, mix);
        observation.sample(frame, channel, actual, expected);
        observation.exact_endpoint_mismatches += usize::from(actual.to_bits() != (expected as f32).to_bits());
    }
}

fn mix_ramp_and_lifecycle(rate: u32, parts: &[usize], rows: &mut Vec<Value>) -> bool {
    let n = (rate as usize).div_ceil(100);
    let repeated = n / 3;
    let mut effect = prepared(rate, 500.0, 0.0);
    let mut observation = Observation::default();
    render(&mut effect, 0..1, parts, dry, |_, channel| dry_mixed(channel, 0.0), &mut observation);
    observation.edit(&mut effect, param::MIX, 1.0, false);
    let ramp = |frame: usize, channel| dry_mixed(channel, ((frame + 1) as f64 / n as f64).min(1.0));
    render(&mut effect, 0..repeated, parts, dry, ramp, &mut observation);
    observation.edit(&mut effect, param::MIX, 1.0, false);
    let before: [u32; param::COUNT] = std::array::from_fn(|index| effect.get_param(index).to_bits());
    for automated in [false, true] {
        for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            let accepted = measured(|| {
                if automated { effect.set_automated_param(param::MIX, value) }
                else { effect.set_param(param::MIX, value) }
            }, &mut observation.heap);
            observation.rejected_expected_edits += usize::from(accepted);
        }
    }
    let after: [u32; param::COUNT] = std::array::from_fn(|index| effect.get_param(index).to_bits());
    observation.target_mismatches += usize::from(before != after);
    measured(|| effect.set_audio_context(context(rate, 240.0)), &mut observation.heap);
    render(&mut effect, repeated..n - 1, parts, dry, ramp, &mut observation);
    exact_dry_frame(&mut effect, n - 1, 1.0, &mut observation);
    render(&mut effect, n..n + 2, parts, dry, ramp, &mut observation);

    observation.edit(&mut effect, param::MIX, 0.25, false);
    render(&mut effect, 0..1, parts, dry,
        |_, channel| dry_mixed(channel, 1.0 - 0.75 / n as f64), &mut observation);
    measured(|| effect.reset(), &mut observation.heap);
    exact_dry_frame(&mut effect, 0, 0.25, &mut observation);
    observation.edit(&mut effect, param::MIX, 1.0, false);
    measured(|| effect.reset(), &mut observation.heap);
    observation.edit(&mut effect, param::MIX, 0.5, false);
    exact_dry_frame(&mut effect, 0, 0.5, &mut observation);
    observation.finish("mix_endpoint_same_rejected_context_reset_restoration", rate, parts, rows)
}

fn mix_retarget(rate: u32, parts: &[usize], automated: bool, rows: &mut Vec<Value>) -> bool {
    let n = (rate as usize).div_ceil(100);
    let retarget_at = n / 2;
    let automate_at = retarget_at + n / 2;
    let origin = retarget_at as f64 / n as f64;
    let mut effect = prepared(rate, 500.0, 0.0);
    let mut observation = Observation::default();
    render(&mut effect, 0..1, parts, dry, |_, channel| dry_mixed(channel, 0.0), &mut observation);
    observation.edit(&mut effect, param::MIX, 1.0, false);
    render(&mut effect, 0..retarget_at, parts, dry,
        |frame, channel| dry_mixed(channel, (frame + 1) as f64 / n as f64), &mut observation);
    observation.edit(&mut effect, param::MIX, 0.25, false);
    let ramp = |frame: usize, channel| {
        let progress = ((frame - retarget_at + 1) as f64 / n as f64).min(1.0);
        dry_mixed(channel, origin + (0.25 - origin) * progress)
    };
    render(&mut effect, retarget_at..automate_at, parts, dry, ramp, &mut observation);
    observation.edit(&mut effect, param::MIX, 0.25, automated);
    render(&mut effect, automate_at..retarget_at + n + 2, parts, dry,
        |frame, channel| if automated { dry_mixed(channel, 0.25) } else { ramp(frame, channel) },
        &mut observation);
    observation.finish(if automated { "same_automated_target_snaps_mix" } else { "retarget_from_last_applied_mix" }, rate, parts, rows)
}

fn rich(frame: usize, channel: usize) -> f32 {
    (((17 * frame + 29 * channel) % 97) as f32 - 48.0) / 128.0
}

fn past_dry(frame: usize, channel: usize, distance: f64) -> f64 {
    let whole = distance.floor() as usize;
    let fraction = distance.fract();
    let sample = |delay| frame.checked_sub(delay)
        .map(|at| f64::from(rich(at, channel))).unwrap_or(0.0);
    sample(whole) * (1.0 - fraction) + sample(whole + 1) * fraction
}

fn mix_automation_preserves_timing(rate: u32, parts: &[usize], rows: &mut Vec<Value>) -> bool {
    let n = (rate as usize).div_ceil(100);
    let fade_frames = (rate as usize).div_ceil(50);
    let warm = (f64::from(rate) * 0.04).ceil() as usize;
    let time_at = n / 4;
    let automate_at = n / 2;
    let old = 10.25 * f64::from(rate) / 1000.0;
    let new = 17.5 * f64::from(rate) / 1000.0;
    let mut effect = prepared(rate, 10.25, 0.25);
    let mut observation = Observation::default();
    render(&mut effect, 0..warm, parts, rich,
        |frame, channel| f64::from(rich(frame, channel)) * 0.75 + past_dry(frame, channel, old) * 0.25,
        &mut observation);
    observation.edit(&mut effect, param::MIX, 0.75, false);
    let expected = |frame: usize, channel| {
        let mix = if frame < automate_at { 0.25 + 0.5 * (frame + 1) as f64 / n as f64 } else { 0.75 };
        let wet = if frame < time_at {
            past_dry(warm + frame, channel, old)
        } else {
            let progress = ((frame - time_at) as f64 / (fade_frames - 1) as f64).min(1.0);
            past_dry(warm + frame, channel, old) * (1.0 - progress)
                + past_dry(warm + frame, channel, new) * progress
        };
        f64::from(rich(warm + frame, channel)) * (1.0 - mix) + wet * mix
    };
    let input = |frame, channel| rich(warm + frame, channel);
    render(&mut effect, 0..time_at, parts, input, expected, &mut observation);
    observation.edit(&mut effect, param::TIME_MS, 17.5, false);
    render(&mut effect, time_at..automate_at, parts, input, expected, &mut observation);
    observation.edit(&mut effect, param::MIX, 0.75, true);
    render(&mut effect, automate_at..time_at + fade_frames + 2, parts, input, expected, &mut observation);
    observation.finish("mix_automation_keeps_real_fixed_tap_crossfade", rate, parts, rows)
}

#[test]
fn manual_mix_ramps_for_ten_milliseconds_and_automation_owns_only_mix() {
    let mut rows = Vec::new();
    let mut failures = 0;
    for rate in [1000, 1001, 48_000] {
        for parts in [&[1][..], &[7, 31, 64, 1, 128, 512][..]] {
            failures += usize::from(!mix_ramp_and_lifecycle(rate, parts, &mut rows));
            for automated in [false, true] {
                failures += usize::from(!mix_retarget(rate, parts, automated, &mut rows));
            }
            failures += usize::from(!mix_automation_preserves_timing(rate, parts, &mut rows));
        }
    }
    println!("DELAY_MIX_RAMP_RECORD {}", json!({
        "execution_head": std::env::var("VIBEZ_VERIFICATION_HEAD").unwrap_or_else(|_| "UNLABELLED".into()),
        "oracle": "Independent k/N Mix envelope and past dry samples; endpoint-inclusive fixed-tap timing; Feedback0",
        "sample_tolerance": SAMPLE_TOLERANCE, "observed": rows,
    }));
    assert_eq!(failures, 0, "Mix ramp/ownership failures; see all observations above");
}

mod feedback {
    use super::*;

    fn input(frame: usize, channel: usize) -> f32 {
        rich(frame, channel) / 16.0
    }

    fn impulse(frame: usize, channel: usize) -> f32 {
        if frame == 0 { [0.03125, -0.015625][channel] } else { 0.0 }
    }

    fn reference(
        rate: u32,
        frames: usize,
        dry: impl Fn(usize, usize) -> f32,
        feedback: impl Fn(usize) -> f64,
        mix: impl Fn(usize) -> f64,
        timing: impl Fn(usize) -> (f64, f64, f64),
    ) -> Vec<[f64; 2]> {
        // Chronological writes avoid duplicating the production circular-buffer algorithm.
        let mut writes: Vec<[f64; 2]> = Vec::with_capacity(frames);
        let mut output = Vec::with_capacity(frames);
        for frame in 0..frames {
            let (old_ms, new_ms, blend) = timing(frame);
            let mut next_write = [0.0; 2];
            let mut mixed = [0.0; 2];
            for channel in 0..2 {
                let read = |milliseconds: f64| {
                    let distance = milliseconds * f64::from(rate) / 1000.0;
                    let whole = distance.floor() as usize;
                    let fraction = distance.fract();
                    let sample = |delay| frame.checked_sub(delay)
                        .map(|at| writes[at][channel]).unwrap_or(0.0);
                    sample(whole) * (1.0 - fraction) + sample(whole + 1) * fraction
                };
                let wet = read(old_ms) * (1.0 - blend) + read(new_ms) * blend;
                let sample = f64::from(dry(frame, channel));
                next_write[channel] = sample + feedback(frame) * wet;
                mixed[channel] = sample * (1.0 - mix(frame)) + wet * mix(frame);
            }
            writes.push(next_write);
            output.push(mixed);
        }
        output
    }

    fn prepare(rate: u32, time_ms: f32, mix: f32) -> DelayEffect {
        let mut effect = prepared(rate, time_ms, mix);
        assert!(effect.set_param(param::FEEDBACK, 0.25));
        effect
    }

    fn manual_ramp_and_lifecycle(rate: u32, parts: &[usize], rows: &mut Vec<Value>) -> bool {
        let n = (rate as usize).div_ceil(100);
        let distance = (rate as usize).div_ceil(1000);
        let warm = 2 * n;
        let repeated = warm + n / 3;
        let end = warm + n + 3 * distance + 3;
        let expected = reference(rate, end, input,
            |frame| if frame < warm { 0.25 }
                else { 0.25 + 0.5 * ((frame - warm + 1) as f64 / n as f64).min(1.0) },
            |_| 1.0, |_| (1.0, 1.0, 0.0));
        let oracle = |frame: usize, channel| expected[frame][channel];
        let mut effect = prepare(rate, 1.0, 1.0);
        let mut observation = Observation::default();
        render(&mut effect, 0..warm, parts, input, oracle, &mut observation);
        observation.edit(&mut effect, param::FEEDBACK, 0.75, false);
        render(&mut effect, warm..repeated, parts, input, oracle, &mut observation);
        observation.edit(&mut effect, param::FEEDBACK, 0.75, false);
        let before: [u32; param::COUNT] = std::array::from_fn(|index| effect.get_param(index).to_bits());
        for automated in [false, true] {
            for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
                let accepted = measured(|| {
                    if automated { effect.set_automated_param(param::FEEDBACK, value) }
                    else { effect.set_param(param::FEEDBACK, value) }
                }, &mut observation.heap);
                observation.rejected_expected_edits += usize::from(accepted);
            }
        }
        let after: [u32; param::COUNT] = std::array::from_fn(|index| effect.get_param(index).to_bits());
        observation.target_mismatches += usize::from(before != after);
        render(&mut effect, repeated..end, parts, input, oracle, &mut observation);

        for (target, pre_audio) in [(0.5_f32, false), (0.125, true)] {
            observation.edit(&mut effect, param::FEEDBACK, if pre_audio { 0.75 } else { target }, false);
            measured(|| effect.reset(), &mut observation.heap);
            if pre_audio { observation.edit(&mut effect, param::FEEDBACK, target, false); }
            let frames = 3 * distance + 3;
            let expected = reference(rate, frames, impulse, |_| f64::from(target),
                |_| 1.0, |_| (1.0, 1.0, 0.0));
            render(&mut effect, 0..frames, parts, impulse,
                |frame, channel| expected[frame][channel], &mut observation);
        }
        observation.finish("feedback_ramp_same_rejected_reset_pre_audio", rate, parts, rows)
    }

    fn manual_retarget(rate: u32, parts: &[usize], rows: &mut Vec<Value>) -> bool {
        let n = (rate as usize).div_ceil(100);
        let warm = 2 * n;
        let retarget = warm + n / 2;
        let repeated = retarget + n / 3;
        let end = retarget + n + 3 * (rate as usize).div_ceil(1000) + 3;
        let origin = 0.25 + 0.5 * (n / 2) as f64 / n as f64;
        let expected = reference(rate, end, input,
            |frame| if frame < warm { 0.25 }
                else if frame < retarget { 0.25 + 0.5 * (frame - warm + 1) as f64 / n as f64 }
                else { origin + (0.125 - origin) * ((frame - retarget + 1) as f64 / n as f64).min(1.0) },
            |_| 1.0, |_| (1.0, 1.0, 0.0));
        let oracle = |frame: usize, channel| expected[frame][channel];
        let mut effect = prepare(rate, 1.0, 1.0);
        let mut observation = Observation::default();
        render(&mut effect, 0..warm, parts, input, oracle, &mut observation);
        observation.edit(&mut effect, param::FEEDBACK, 0.75, false);
        render(&mut effect, warm..retarget, parts, input, oracle, &mut observation);
        observation.edit(&mut effect, param::FEEDBACK, 0.125, false);
        render(&mut effect, retarget..repeated, parts, input, oracle, &mut observation);
        observation.edit(&mut effect, param::FEEDBACK, 0.125, false);
        render(&mut effect, repeated..end, parts, input, oracle, &mut observation);
        observation.finish("feedback_retarget_last_applied_without_same_target_restart", rate, parts, rows)
    }

    fn exact_automation_ownership(rate: u32, parts: &[usize], automate_feedback: bool, rows: &mut Vec<Value>) -> bool {
        let n = (rate as usize).div_ceil(100);
        let fade = (rate as usize).div_ceil(50);
        let warm = 2 * n;
        let time_edit = warm + n / 4;
        let automation = warm + n / 2;
        let end = time_edit + fade + 3 * (3.5 * f64::from(rate) / 1000.0).ceil() as usize + 3;
        let amount = |frame: usize, automated| {
            if frame < warm { 0.25 }
            else if automated && frame >= automation { 0.75 }
            else { 0.25 + 0.5 * ((frame - warm + 1) as f64 / n as f64).min(1.0) }
        };
        let expected = reference(rate, end, input,
            |frame| amount(frame, automate_feedback), |frame| amount(frame, !automate_feedback),
            |frame| if frame < time_edit { (1.25, 1.25, 0.0) }
                else { (1.25, 3.5, ((frame - time_edit) as f64 / (fade - 1) as f64).min(1.0)) });
        let oracle = |frame: usize, channel| expected[frame][channel];
        let mut effect = prepare(rate, 1.25, 0.25);
        let mut observation = Observation::default();
        render(&mut effect, 0..warm, parts, input, oracle, &mut observation);
        observation.edit(&mut effect, param::FEEDBACK, 0.75, false);
        observation.edit(&mut effect, param::MIX, 0.75, false);
        render(&mut effect, warm..time_edit, parts, input, oracle, &mut observation);
        observation.edit(&mut effect, param::TIME_MS, 3.5, false);
        render(&mut effect, time_edit..automation, parts, input, oracle, &mut observation);
        observation.edit(&mut effect, if automate_feedback { param::FEEDBACK } else { param::MIX }, 0.75, true);
        render(&mut effect, automation..end, parts, input, oracle, &mut observation);
        observation.finish(if automate_feedback { "feedback_automation_keeps_mix_and_time_moving" }
            else { "mix_automation_keeps_feedback_and_time_moving" }, rate, parts, rows)
    }

    #[test]
    fn manual_feedback_ramps_for_ten_milliseconds_and_automation_owns_only_feedback() {
        let mut rows = Vec::new();
        let mut failures = 0;
        for rate in [1000, 1001, 48_000] {
            for parts in [&[1][..], &[7, 31, 64, 1, 128, 512][..]] {
                failures += usize::from(!manual_ramp_and_lifecycle(rate, parts, &mut rows));
                failures += usize::from(!manual_retarget(rate, parts, &mut rows));
                for automate_feedback in [false, true] {
                    failures += usize::from(!exact_automation_ownership(rate, parts, automate_feedback, &mut rows));
                }
            }
        }
        println!("DELAY_FEEDBACK_RAMP_RECORD {}", json!({
            "execution_head": std::env::var("VIBEZ_VERIFICATION_HEAD").unwrap_or_else(|_| "UNLABELLED".into()),
            "oracle": "Independent chronological f64 writes x[n]+g[n]*wet[n], fractional past writes and mixed wet output; k/N gain, i/(N-1) fixed-tap Crossfade",
            "sample_tolerance": SAMPLE_TOLERANCE, "observed": rows,
        }));
        assert_eq!(failures, 0, "Feedback ramp/ownership failures; see all observations above");
    }
}
