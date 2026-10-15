//! Verifies Delay control trajectories and feedback networks through public audio.

use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
};

use serde_json::{json, Value};
use vibez_core::audio_context::DeviceAudioContext;
use vibez_dsp::{
    delay::{param, DelayEffect},
    effect::AudioEffect,
};

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
    ALLOCATIONS.set(AllocationRecord {
        active: true,
        counts: [0; 3],
    });
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
    for (index, value) in [
        (param::TIME_MS, time_ms),
        (param::FEEDBACK, 0.0),
        (param::MIX, mix),
    ] {
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
            self.first_mismatch
                .get_or_insert((frame, channel, expected, actual));
        }
    }

    fn edit(&mut self, effect: &mut DelayEffect, index: usize, value: f32, automated: bool) {
        let accepted = measured(
            || {
                if automated {
                    effect.set_automated_param(index, value)
                } else {
                    effect.set_param(index, value)
                }
            },
            &mut self.heap,
        );
        self.rejected_expected_edits += usize::from(!accepted);
        self.target_mismatches += usize::from(effect.get_param(index).to_bits() != value.to_bits());
    }

    fn finish(self, scenario: &str, rate: u32, parts: &[usize], rows: &mut Vec<Value>) -> bool {
        let matches = self.heap == [0; 3]
            && self.mismatches == 0
            && self.rejected_expected_edits == 0
            && self.target_mismatches == 0
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

fn exact_dry_frame(
    effect: &mut DelayEffect,
    frame: usize,
    mix: f64,
    observation: &mut Observation,
) {
    let mut block = [dry(0, 0), dry(0, 1)];
    measured(|| effect.process(&mut block, 2), &mut observation.heap);
    for (channel, actual) in block.into_iter().enumerate() {
        let expected = dry_mixed(channel, mix);
        observation.sample(frame, channel, actual, expected);
        observation.exact_endpoint_mismatches +=
            usize::from(actual.to_bits() != (expected as f32).to_bits());
    }
}

fn mix_ramp_and_lifecycle(rate: u32, parts: &[usize], rows: &mut Vec<Value>) -> bool {
    let n = (rate as usize).div_ceil(100);
    let repeated = n / 3;
    let mut effect = prepared(rate, 500.0, 0.0);
    let mut observation = Observation::default();
    render(
        &mut effect,
        0..1,
        parts,
        dry,
        |_, channel| dry_mixed(channel, 0.0),
        &mut observation,
    );
    observation.edit(&mut effect, param::MIX, 1.0, false);
    let ramp = |frame: usize, channel| dry_mixed(channel, ((frame + 1) as f64 / n as f64).min(1.0));
    render(&mut effect, 0..repeated, parts, dry, ramp, &mut observation);
    observation.edit(&mut effect, param::MIX, 1.0, false);
    let before: [u32; param::COUNT] =
        std::array::from_fn(|index| effect.get_param(index).to_bits());
    for automated in [false, true] {
        for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            let accepted = measured(
                || {
                    if automated {
                        effect.set_automated_param(param::MIX, value)
                    } else {
                        effect.set_param(param::MIX, value)
                    }
                },
                &mut observation.heap,
            );
            observation.rejected_expected_edits += usize::from(accepted);
        }
    }
    let after: [u32; param::COUNT] = std::array::from_fn(|index| effect.get_param(index).to_bits());
    observation.target_mismatches += usize::from(before != after);
    measured(
        || effect.set_audio_context(context(rate, 240.0)),
        &mut observation.heap,
    );
    render(
        &mut effect,
        repeated..n - 1,
        parts,
        dry,
        ramp,
        &mut observation,
    );
    exact_dry_frame(&mut effect, n - 1, 1.0, &mut observation);
    render(&mut effect, n..n + 2, parts, dry, ramp, &mut observation);

    observation.edit(&mut effect, param::MIX, 0.25, false);
    render(
        &mut effect,
        0..1,
        parts,
        dry,
        |_, channel| dry_mixed(channel, 1.0 - 0.75 / n as f64),
        &mut observation,
    );
    measured(|| effect.reset(), &mut observation.heap);
    exact_dry_frame(&mut effect, 0, 0.25, &mut observation);
    observation.edit(&mut effect, param::MIX, 1.0, false);
    measured(|| effect.reset(), &mut observation.heap);
    observation.edit(&mut effect, param::MIX, 0.5, false);
    exact_dry_frame(&mut effect, 0, 0.5, &mut observation);
    observation.finish(
        "mix_endpoint_same_rejected_context_reset_restoration",
        rate,
        parts,
        rows,
    )
}

fn mix_retarget(rate: u32, parts: &[usize], automated: bool, rows: &mut Vec<Value>) -> bool {
    let n = (rate as usize).div_ceil(100);
    let retarget_at = n / 2;
    let automate_at = retarget_at + n / 2;
    let origin = retarget_at as f64 / n as f64;
    let mut effect = prepared(rate, 500.0, 0.0);
    let mut observation = Observation::default();
    render(
        &mut effect,
        0..1,
        parts,
        dry,
        |_, channel| dry_mixed(channel, 0.0),
        &mut observation,
    );
    observation.edit(&mut effect, param::MIX, 1.0, false);
    render(
        &mut effect,
        0..retarget_at,
        parts,
        dry,
        |frame, channel| dry_mixed(channel, (frame + 1) as f64 / n as f64),
        &mut observation,
    );
    observation.edit(&mut effect, param::MIX, 0.25, false);
    let ramp = |frame: usize, channel| {
        let progress = ((frame - retarget_at + 1) as f64 / n as f64).min(1.0);
        dry_mixed(channel, origin + (0.25 - origin) * progress)
    };
    render(
        &mut effect,
        retarget_at..automate_at,
        parts,
        dry,
        ramp,
        &mut observation,
    );
    observation.edit(&mut effect, param::MIX, 0.25, automated);
    render(
        &mut effect,
        automate_at..retarget_at + n + 2,
        parts,
        dry,
        |frame, channel| {
            if automated {
                dry_mixed(channel, 0.25)
            } else {
                ramp(frame, channel)
            }
        },
        &mut observation,
    );
    observation.finish(
        if automated {
            "same_automated_target_snaps_mix"
        } else {
            "retarget_from_last_applied_mix"
        },
        rate,
        parts,
        rows,
    )
}

fn rich(frame: usize, channel: usize) -> f32 {
    (((17 * frame + 29 * channel) % 97) as f32 - 48.0) / 128.0
}

fn past_dry(frame: usize, channel: usize, distance: f64) -> f64 {
    let whole = distance.floor() as usize;
    let fraction = distance.fract();
    let sample = |delay| {
        frame
            .checked_sub(delay)
            .map(|at| f64::from(rich(at, channel)))
            .unwrap_or(0.0)
    };
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
    render(
        &mut effect,
        0..warm,
        parts,
        rich,
        |frame, channel| {
            f64::from(rich(frame, channel)) * 0.75 + past_dry(frame, channel, old) * 0.25
        },
        &mut observation,
    );
    observation.edit(&mut effect, param::MIX, 0.75, false);
    let expected = |frame: usize, channel| {
        let mix = if frame < automate_at {
            0.25 + 0.5 * (frame + 1) as f64 / n as f64
        } else {
            0.75
        };
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
    render(
        &mut effect,
        0..time_at,
        parts,
        input,
        expected,
        &mut observation,
    );
    observation.edit(&mut effect, param::TIME_MS, 17.5, false);
    render(
        &mut effect,
        time_at..automate_at,
        parts,
        input,
        expected,
        &mut observation,
    );
    observation.edit(&mut effect, param::MIX, 0.75, true);
    render(
        &mut effect,
        automate_at..time_at + fade_frames + 2,
        parts,
        input,
        expected,
        &mut observation,
    );
    observation.finish(
        "mix_automation_keeps_real_fixed_tap_crossfade",
        rate,
        parts,
        rows,
    )
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
    println!(
        "DELAY_MIX_RAMP_RECORD {}",
        json!({
            "execution_head": std::env::var("VIBEZ_VERIFICATION_HEAD").unwrap_or_else(|_| "UNLABELLED".into()),
            "oracle": "Independent k/N Mix envelope and past dry samples; endpoint-inclusive fixed-tap timing; Feedback0",
            "sample_tolerance": SAMPLE_TOLERANCE, "observed": rows,
        })
    );
    assert_eq!(
        failures, 0,
        "Mix ramp/ownership failures; see all observations above"
    );
}

mod feedback {
    use super::*;

    fn input(frame: usize, channel: usize) -> f32 {
        rich(frame, channel) / 16.0
    }

    fn impulse(frame: usize, channel: usize) -> f32 {
        if frame == 0 {
            [0.03125, -0.015625][channel]
        } else {
            0.0
        }
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
                    let sample = |delay| {
                        frame
                            .checked_sub(delay)
                            .map(|at| writes[at][channel])
                            .unwrap_or(0.0)
                    };
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
        let expected = reference(
            rate,
            end,
            input,
            |frame| {
                if frame < warm {
                    0.25
                } else {
                    0.25 + 0.5 * ((frame - warm + 1) as f64 / n as f64).min(1.0)
                }
            },
            |_| 1.0,
            |_| (1.0, 1.0, 0.0),
        );
        let oracle = |frame: usize, channel| expected[frame][channel];
        let mut effect = prepare(rate, 1.0, 1.0);
        let mut observation = Observation::default();
        render(&mut effect, 0..warm, parts, input, oracle, &mut observation);
        observation.edit(&mut effect, param::FEEDBACK, 0.75, false);
        render(
            &mut effect,
            warm..repeated,
            parts,
            input,
            oracle,
            &mut observation,
        );
        observation.edit(&mut effect, param::FEEDBACK, 0.75, false);
        let before: [u32; param::COUNT] =
            std::array::from_fn(|index| effect.get_param(index).to_bits());
        for automated in [false, true] {
            for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
                let accepted = measured(
                    || {
                        if automated {
                            effect.set_automated_param(param::FEEDBACK, value)
                        } else {
                            effect.set_param(param::FEEDBACK, value)
                        }
                    },
                    &mut observation.heap,
                );
                observation.rejected_expected_edits += usize::from(accepted);
            }
        }
        let after: [u32; param::COUNT] =
            std::array::from_fn(|index| effect.get_param(index).to_bits());
        observation.target_mismatches += usize::from(before != after);
        render(
            &mut effect,
            repeated..end,
            parts,
            input,
            oracle,
            &mut observation,
        );

        for (target, pre_audio) in [(0.5_f32, false), (0.125, true)] {
            observation.edit(
                &mut effect,
                param::FEEDBACK,
                if pre_audio { 0.75 } else { target },
                false,
            );
            measured(|| effect.reset(), &mut observation.heap);
            if pre_audio {
                observation.edit(&mut effect, param::FEEDBACK, target, false);
            }
            let frames = 3 * distance + 3;
            let expected = reference(
                rate,
                frames,
                impulse,
                |_| f64::from(target),
                |_| 1.0,
                |_| (1.0, 1.0, 0.0),
            );
            render(
                &mut effect,
                0..frames,
                parts,
                impulse,
                |frame, channel| expected[frame][channel],
                &mut observation,
            );
        }
        observation.finish(
            "feedback_ramp_same_rejected_reset_pre_audio",
            rate,
            parts,
            rows,
        )
    }

    fn manual_retarget(rate: u32, parts: &[usize], rows: &mut Vec<Value>) -> bool {
        let n = (rate as usize).div_ceil(100);
        let warm = 2 * n;
        let retarget = warm + n / 2;
        let repeated = retarget + n / 3;
        let end = retarget + n + 3 * (rate as usize).div_ceil(1000) + 3;
        let origin = 0.25 + 0.5 * (n / 2) as f64 / n as f64;
        let expected = reference(
            rate,
            end,
            input,
            |frame| {
                if frame < warm {
                    0.25
                } else if frame < retarget {
                    0.25 + 0.5 * (frame - warm + 1) as f64 / n as f64
                } else {
                    origin + (0.125 - origin) * ((frame - retarget + 1) as f64 / n as f64).min(1.0)
                }
            },
            |_| 1.0,
            |_| (1.0, 1.0, 0.0),
        );
        let oracle = |frame: usize, channel| expected[frame][channel];
        let mut effect = prepare(rate, 1.0, 1.0);
        let mut observation = Observation::default();
        render(&mut effect, 0..warm, parts, input, oracle, &mut observation);
        observation.edit(&mut effect, param::FEEDBACK, 0.75, false);
        render(
            &mut effect,
            warm..retarget,
            parts,
            input,
            oracle,
            &mut observation,
        );
        observation.edit(&mut effect, param::FEEDBACK, 0.125, false);
        render(
            &mut effect,
            retarget..repeated,
            parts,
            input,
            oracle,
            &mut observation,
        );
        observation.edit(&mut effect, param::FEEDBACK, 0.125, false);
        render(
            &mut effect,
            repeated..end,
            parts,
            input,
            oracle,
            &mut observation,
        );
        observation.finish(
            "feedback_retarget_last_applied_without_same_target_restart",
            rate,
            parts,
            rows,
        )
    }

    fn exact_automation_ownership(
        rate: u32,
        parts: &[usize],
        automate_feedback: bool,
        rows: &mut Vec<Value>,
    ) -> bool {
        let n = (rate as usize).div_ceil(100);
        let fade = (rate as usize).div_ceil(50);
        let warm = 2 * n;
        let time_edit = warm + n / 4;
        let automation = warm + n / 2;
        let end = time_edit + fade + 3 * (3.5 * f64::from(rate) / 1000.0).ceil() as usize + 3;
        let amount = |frame: usize, automated| {
            if frame < warm {
                0.25
            } else if automated && frame >= automation {
                0.75
            } else {
                0.25 + 0.5 * ((frame - warm + 1) as f64 / n as f64).min(1.0)
            }
        };
        let expected = reference(
            rate,
            end,
            input,
            |frame| amount(frame, automate_feedback),
            |frame| amount(frame, !automate_feedback),
            |frame| {
                if frame < time_edit {
                    (1.25, 1.25, 0.0)
                } else {
                    (
                        1.25,
                        3.5,
                        ((frame - time_edit) as f64 / (fade - 1) as f64).min(1.0),
                    )
                }
            },
        );
        let oracle = |frame: usize, channel| expected[frame][channel];
        let mut effect = prepare(rate, 1.25, 0.25);
        let mut observation = Observation::default();
        render(&mut effect, 0..warm, parts, input, oracle, &mut observation);
        observation.edit(&mut effect, param::FEEDBACK, 0.75, false);
        observation.edit(&mut effect, param::MIX, 0.75, false);
        render(
            &mut effect,
            warm..time_edit,
            parts,
            input,
            oracle,
            &mut observation,
        );
        observation.edit(&mut effect, param::TIME_MS, 3.5, false);
        render(
            &mut effect,
            time_edit..automation,
            parts,
            input,
            oracle,
            &mut observation,
        );
        observation.edit(
            &mut effect,
            if automate_feedback {
                param::FEEDBACK
            } else {
                param::MIX
            },
            0.75,
            true,
        );
        render(
            &mut effect,
            automation..end,
            parts,
            input,
            oracle,
            &mut observation,
        );
        observation.finish(
            if automate_feedback {
                "feedback_automation_keeps_mix_and_time_moving"
            } else {
                "mix_automation_keeps_feedback_and_time_moving"
            },
            rate,
            parts,
            rows,
        )
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
                    failures += usize::from(!exact_automation_ownership(
                        rate,
                        parts,
                        automate_feedback,
                        &mut rows,
                    ));
                }
            }
        }
        println!(
            "DELAY_FEEDBACK_RAMP_RECORD {}",
            json!({
                "execution_head": std::env::var("VIBEZ_VERIFICATION_HEAD").unwrap_or_else(|_| "UNLABELLED".into()),
                "oracle": "Independent chronological f64 writes x[n]+g[n]*wet[n], fractional past writes and mixed wet output; k/N gain, i/(N-1) fixed-tap Crossfade",
                "sample_tolerance": SAMPLE_TOLERANCE, "observed": rows,
            })
        );
        assert_eq!(
            failures, 0,
            "Feedback ramp/ownership failures; see all observations above"
        );
    }
}

mod routing {
    use super::*;

    #[derive(Clone, Copy)]
    struct Controls {
        feedback: f64,
        mix: f64,
        routing: f64,
        left_old_ms: f64,
        left_new_ms: f64,
        left_blend: f64,
    }

    fn stationary(routing: f64, feedback: f64, mix: f64) -> Controls {
        Controls {
            feedback,
            mix,
            routing,
            left_old_ms: 3.0,
            left_new_ms: 3.0,
            left_blend: 0.0,
        }
    }

    fn reference(
        rate: u32,
        frames: usize,
        input: impl Fn(usize, usize) -> f32,
        controls: impl Fn(usize) -> Controls,
    ) -> Vec<[f64; 2]> {
        // Chronological channel writes are independent of the production ring and routing implementation.
        let mut writes: Vec<[f64; 2]> = Vec::with_capacity(frames);
        let mut output = Vec::with_capacity(frames);
        for frame in 0..frames {
            let state = controls(frame);
            let read = |channel, milliseconds: f64| {
                let distance = milliseconds * f64::from(rate) / 1000.0;
                let whole = distance.floor() as usize;
                let fraction = distance.fract();
                let sample = |delay| {
                    frame
                        .checked_sub(delay)
                        .map(|at| writes[at][channel])
                        .unwrap_or(0.0)
                };
                sample(whole) * (1.0 - fraction) + sample(whole + 1) * fraction
            };
            let wet = [
                read(0, state.left_old_ms) * (1.0 - state.left_blend)
                    + read(0, state.left_new_ms) * state.left_blend,
                read(1, 5.0),
            ];
            let dry = [f64::from(input(frame, 0)), f64::from(input(frame, 1))];
            writes.push([
                dry[0] + state.feedback * ((1.0 - state.routing) * wet[0] + state.routing * wet[1]),
                dry[1] + state.feedback * ((1.0 - state.routing) * wet[1] + state.routing * wet[0]),
            ]);
            output.push([
                dry[0] * (1.0 - state.mix) + wet[0] * state.mix,
                dry[1] * (1.0 - state.mix) + wet[1] * state.mix,
            ]);
        }
        output
    }

    fn prepare(
        rate: u32,
        requested_routing: f32,
        effective_routing: f32,
        feedback: f32,
        mix: f32,
    ) -> DelayEffect {
        let mut effect = prepared(rate, 3.0, mix);
        for (index, value) in [
            (param::FEEDBACK, feedback),
            (param::LINK, 0.0),
            (param::RIGHT_TIME_MS, 5.0),
            (param::ROUTING, requested_routing),
        ] {
            assert!(effect.set_param(index, value));
        }
        assert_eq!(
            effect.get_param(param::ROUTING).to_bits(),
            effective_routing.to_bits()
        );
        effect
    }

    fn finish(
        observation: Observation,
        scenario: &str,
        rate: u32,
        parts: &[usize],
        rows: &mut Vec<Value>,
    ) -> bool {
        let matches = observation.heap == [0; 3]
            && observation.mismatches == 0
            && observation.rejected_expected_edits == 0
            && observation.target_mismatches == 0
            && observation.exact_endpoint_mismatches == 0;
        rows.push(json!({
            "scenario": scenario, "rate": rate, "partitions": parts,
            "routing_frames": (rate as usize).div_ceil(50), "matches": matches,
            "heap_alloc_realloc_dealloc": observation.heap,
            "sample_mismatches": observation.mismatches, "maximum_error": observation.maximum_error,
            "first_mismatch": observation.first_mismatch,
            "unexpected_rejected_edits": observation.rejected_expected_edits,
            "target_bit_mismatches": observation.target_mismatches,
            "exact_routing_endpoint_mismatches": observation.exact_endpoint_mismatches,
        }));
        matches
    }

    fn mono_prefix(
        effect: &mut DelayEffect,
        frames: usize,
        parts: &[usize],
        input: impl Fn(usize, usize) -> f32,
        expected: &[[f64; 2]],
        observation: &mut Observation,
    ) {
        let mut scratch = [0.0; 512];
        let mut frame = 0;
        let mut part = 0;
        while frame < frames {
            let end = (frame + parts[part % parts.len()]).min(frames);
            let block = &mut scratch[..end - frame];
            for (index, sample) in block.iter_mut().enumerate() {
                *sample = input(frame + index, 0);
            }
            measured(|| effect.process(block, 1), &mut observation.heap);
            for (index, &actual) in block.iter().enumerate() {
                observation.sample(frame + index, 0, actual, expected[frame + index][0]);
            }
            frame = end;
            part += 1;
        }
    }

    struct StaticCase {
        name: &'static str,
        requested_routing: f32,
        routing: f32,
        right_impulse: f32,
        mono: bool,
    }

    fn static_arrivals(rate: u32, parts: &[usize], rows: &mut Vec<Value>) -> usize {
        let cases = [
            StaticCase {
                name: "ping_pong_left3_right8_left11",
                requested_routing: 42.0,
                routing: 1.0,
                right_impulse: 0.0,
                mono: false,
            },
            StaticCase {
                name: "ping_pong_preserves_distinct_stereo_inputs",
                requested_routing: 1.0,
                routing: 1.0,
                right_impulse: -0.0625,
                mono: false,
            },
            StaticCase {
                name: "stereo_feedback_preserves_independent_lines",
                requested_routing: -42.0,
                routing: 0.0,
                right_impulse: -0.0625,
                mono: false,
            },
            StaticCase {
                name: "hidden_mono_right_feedback_returns_after_stereo_resume",
                requested_routing: 1.0,
                routing: 1.0,
                right_impulse: 0.0,
                mono: true,
            },
        ];
        let frames = (0.036 * f64::from(rate)).ceil() as usize + 3;
        let mut failures = 0;
        for case in cases {
            let input = |frame, channel| {
                if frame == 0 {
                    if channel == 0 {
                        0.125
                    } else {
                        case.right_impulse
                    }
                } else {
                    0.0
                }
            };
            let expected = reference(rate, frames, input, |_| {
                stationary(f64::from(case.routing), 0.5, 1.0)
            });
            if rate == 1000 && case.routing == 1.0 && case.right_impulse == 0.0 {
                assert_eq!(expected[3], [0.125, 0.0]);
                assert_eq!(expected[8], [0.0, 0.0625]);
                assert_eq!(expected[11], [0.03125, 0.0]);
            }
            let mut effect = prepare(rate, case.requested_routing, case.routing, 0.5, 1.0);
            let mut observation = Observation::default();
            let prefix = if case.mono {
                (0.009 * f64::from(rate)).ceil() as usize
            } else {
                0
            };
            mono_prefix(
                &mut effect,
                prefix,
                parts,
                input,
                &expected,
                &mut observation,
            );
            render(
                &mut effect,
                prefix..frames,
                parts,
                input,
                |frame, channel| expected[frame][channel],
                &mut observation,
            );
            failures += usize::from(!finish(observation, case.name, rate, parts, rows));
        }
        failures
    }

    fn small_input(frame: usize, channel: usize) -> f32 {
        rich(frame, channel) / 16.0
    }

    fn exact_nth_endpoint(rate: u32, parts: &[usize], observation: &mut Observation) {
        let n = (rate as usize).div_ceil(50);
        let left_delay = if rate == 1001 {
            1001
        } else {
            3 * rate as usize / 1000
        };
        let right_delay = if rate == 1001 {
            1001
        } else {
            5 * rate as usize / 1000
        };
        let start = left_delay.saturating_sub(n - 1);
        let arrival_left = start + n - 1;
        let input_at = arrival_left - left_delay;
        let arrival_right = arrival_left + right_delay;
        let input = |frame, channel| {
            if frame == input_at && channel == 0 {
                0.125
            } else {
                0.0
            }
        };
        let expected = |frame, channel| match (frame, channel) {
            (at, 0) if at == arrival_left => 0.125,
            (at, 1) if at == arrival_right => 0.0625,
            _ => 0.0,
        };
        let mut effect = prepare(rate, 0.0, 0.0, 0.5, 1.0);
        // Whole-second distances at 1001 Hz isolate the coefficient endpoint from fractional rounding.
        if rate == 1001 {
            assert!(effect.set_param(param::TIME_MS, 1000.0));
            assert!(effect.set_param(param::RIGHT_TIME_MS, 1000.0));
        }
        let mut silent = [0.0; 2];
        measured(|| effect.process(&mut silent, 2), &mut observation.heap);
        render(&mut effect, 0..start, parts, input, expected, observation);
        observation.edit(&mut effect, param::ROUTING, 1.0, false);
        render(
            &mut effect,
            start..arrival_right,
            parts,
            input,
            expected,
            observation,
        );
        let mut actual = [0.0; 2];
        measured(|| effect.process(&mut actual, 2), &mut observation.heap);
        for (channel, sample) in actual.into_iter().enumerate() {
            let expected = expected(arrival_right, channel);
            observation.sample(arrival_right, channel, sample, expected);
            observation.exact_endpoint_mismatches +=
                usize::from(sample.to_bits() != (expected as f32).to_bits());
        }
    }

    fn manual_transition(
        rate: u32,
        parts: &[usize],
        retarget: bool,
        rows: &mut Vec<Value>,
    ) -> bool {
        let n = (rate as usize).div_ceil(50);
        let warm = (rate as usize).div_ceil(50);
        let retarget_at = warm + n / 2;
        let repeated = if retarget {
            retarget_at + n / 3
        } else {
            warm + n / 3
        };
        let transition_start = if retarget { retarget_at } else { warm };
        let end = transition_start + n + (0.015 * f64::from(rate)).ceil() as usize + 3;
        let origin = (n / 2) as f64 / n as f64;
        let routing = |frame: usize| {
            if frame < warm {
                0.0
            } else if retarget && frame >= retarget_at {
                origin * (1.0 - ((frame - retarget_at + 1) as f64 / n as f64).min(1.0))
            } else {
                ((frame - warm + 1) as f64 / n as f64).min(1.0)
            }
        };
        let expected = reference(rate, end, small_input, |frame| {
            stationary(routing(frame), 0.5, 1.0)
        });
        let oracle = |frame: usize, channel| expected[frame][channel];
        let mut effect = prepare(rate, 0.0, 0.0, 0.5, 1.0);
        let mut observation = Observation::default();
        render(
            &mut effect,
            0..warm,
            parts,
            small_input,
            oracle,
            &mut observation,
        );
        observation.edit(&mut effect, param::ROUTING, 1.0, false);
        if retarget {
            render(
                &mut effect,
                warm..retarget_at,
                parts,
                small_input,
                oracle,
                &mut observation,
            );
            observation.edit(&mut effect, param::ROUTING, 0.0, false);
        }
        render(
            &mut effect,
            transition_start..repeated,
            parts,
            small_input,
            oracle,
            &mut observation,
        );
        let target = if retarget { 0.0 } else { 1.0 };
        observation.edit(&mut effect, param::ROUTING, target, false);
        for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            for automated in [false, true] {
                let accepted = measured(
                    || {
                        if automated {
                            effect.set_automated_param(param::ROUTING, value)
                        } else {
                            effect.set_param(param::ROUTING, value)
                        }
                    },
                    &mut observation.heap,
                );
                observation.rejected_expected_edits += usize::from(accepted);
                observation.target_mismatches +=
                    usize::from(effect.get_param(param::ROUTING).to_bits() != target.to_bits());
            }
        }
        render(
            &mut effect,
            repeated..end,
            parts,
            small_input,
            oracle,
            &mut observation,
        );
        for pre_audio in [false, true] {
            let target = if pre_audio { 1.0 } else { 1.0 - target };
            observation.edit(
                &mut effect,
                param::ROUTING,
                if pre_audio { 0.0 } else { target },
                false,
            );
            measured(|| effect.reset(), &mut observation.heap);
            if pre_audio {
                observation.edit(&mut effect, param::ROUTING, target, false);
            }
            let frames = (0.024 * f64::from(rate)).ceil() as usize + 3;
            let impulse = |frame, channel| {
                if frame == 0 {
                    [0.125, -0.0625][channel]
                } else {
                    0.0
                }
            };
            let expected = reference(rate, frames, impulse, |_| {
                stationary(f64::from(target), 0.5, 1.0)
            });
            render(
                &mut effect,
                0..frames,
                parts,
                impulse,
                |frame, channel| expected[frame][channel],
                &mut observation,
            );
        }
        if !retarget {
            exact_nth_endpoint(rate, parts, &mut observation);
        }
        finish(
            observation,
            if retarget {
                "routing_retargets_last_applied_then_reset_and_pre_audio_snap"
            } else {
                "routing_twenty_ms_same_rejected_then_reset_and_pre_audio_snap"
            },
            rate,
            parts,
            rows,
        )
    }

    fn automation_ownership(
        rate: u32,
        parts: &[usize],
        automate_routing: bool,
        rows: &mut Vec<Value>,
    ) -> bool {
        let scalar_frames = (rate as usize).div_ceil(100);
        let routing_frames = (rate as usize).div_ceil(50);
        let warm = routing_frames;
        let time_edit = warm + scalar_frames / 4;
        let automation = warm + scalar_frames / 2;
        let end = time_edit + routing_frames + (0.021 * f64::from(rate)).ceil() as usize + 3;
        let amount = |frame: usize, automated| {
            if frame < warm {
                0.25
            } else if automated && frame >= automation {
                0.75
            } else {
                0.25 + 0.5 * ((frame - warm + 1) as f64 / scalar_frames as f64).min(1.0)
            }
        };
        let expected = reference(rate, end, small_input, |frame| {
            let routing = if frame < warm {
                0.0
            } else if automate_routing && frame >= automation {
                1.0
            } else {
                ((frame - warm + 1) as f64 / routing_frames as f64).min(1.0)
            };
            Controls {
                routing,
                feedback: amount(frame, !automate_routing),
                mix: amount(frame, false),
                left_old_ms: 3.0,
                left_new_ms: if frame < time_edit { 3.0 } else { 7.0 },
                left_blend: if frame < time_edit {
                    0.0
                } else {
                    ((frame - time_edit) as f64 / (routing_frames - 1) as f64).min(1.0)
                },
            }
        });
        let oracle = |frame: usize, channel| expected[frame][channel];
        let mut effect = prepare(rate, 0.0, 0.0, 0.25, 0.25);
        let mut observation = Observation::default();
        render(
            &mut effect,
            0..warm,
            parts,
            small_input,
            oracle,
            &mut observation,
        );
        for (index, value) in [
            (param::ROUTING, 1.0),
            (param::FEEDBACK, 0.75),
            (param::MIX, 0.75),
        ] {
            observation.edit(&mut effect, index, value, false);
        }
        render(
            &mut effect,
            warm..time_edit,
            parts,
            small_input,
            oracle,
            &mut observation,
        );
        observation.edit(&mut effect, param::TIME_MS, 7.0, false);
        render(
            &mut effect,
            time_edit..automation,
            parts,
            small_input,
            oracle,
            &mut observation,
        );
        observation.edit(
            &mut effect,
            if automate_routing {
                param::ROUTING
            } else {
                param::FEEDBACK
            },
            if automate_routing { 1.0 } else { 0.75 },
            true,
        );
        render(
            &mut effect,
            automation..end,
            parts,
            small_input,
            oracle,
            &mut observation,
        );
        finish(
            observation,
            if automate_routing {
                "routing_automation_preserves_mix_feedback_and_time"
            } else {
                "feedback_automation_preserves_routing_mix_and_time"
            },
            rate,
            parts,
            rows,
        )
    }

    #[test]
    fn feedback_routing_uses_independent_lines_and_twenty_millisecond_own_field_edits() {
        let mut rows = Vec::new();
        let mut failures = 0;
        for rate in [1000, 1001, 48_000] {
            for parts in [&[1][..], &[7, 31, 64, 1, 128, 512][..]] {
                failures += static_arrivals(rate, parts, &mut rows);
                for retarget in [false, true] {
                    failures += usize::from(!manual_transition(rate, parts, retarget, &mut rows));
                }
                for automate_routing in [false, true] {
                    failures += usize::from(!automation_ownership(
                        rate,
                        parts,
                        automate_routing,
                        &mut rows,
                    ));
                }
            }
        }
        println!(
            "DELAY_ROUTING_RECORD {}",
            json!({
                "execution_head": std::env::var("VIBEZ_VERIFICATION_HEAD").unwrap_or_else(|_| "UNLABELLED".into()),
                "oracle": "Independent two-channel chronological feedback writes, identity/swap convex matrices, input and first wet unchanged; routing k/N, timing i/(N-1)",
                "sample_tolerance": SAMPLE_TOLERANCE, "observed": rows,
            })
        );
        assert_eq!(
            failures, 0,
            "Routing/ownership failures; see all observations above"
        );
    }
}

mod feedback_filters {
    use super::*;

    const RESPONSE_TOLERANCE: f64 = 5.0e-6;

    #[derive(Clone, Copy)]
    struct Mode {
        name: &'static str,
        hp: bool,
        lp: bool,
        hp_hz: f32,
        lp_hz: f32,
    }

    #[derive(Clone, Copy)]
    struct Switches {
        hp: bool,
        lp: bool,
        ping: bool,
    }

    #[derive(Clone, Copy)]
    struct DifferenceFilter {
        pole: f64,
        b0: f64,
        b1: f64,
        previous_input: f64,
        previous_output: f64,
    }

    impl DifferenceFilter {
        fn new(rate: u32, stored_hz: f32, high_pass: bool) -> Self {
            let cutoff = f64::from(stored_hz).min(0.45 * f64::from(rate));
            let (sine, cosine) = (std::f64::consts::PI * cutoff / f64::from(rate)).sin_cos();
            let denominator = cosine + sine;
            let b0 = (if high_pass { cosine } else { sine }) / denominator;
            Self {
                pole: (cosine - sine) / denominator,
                b0,
                b1: if high_pass { -b0 } else { b0 },
                previous_input: 0.0,
                previous_output: 0.0,
            }
        }

        fn step(&mut self, input: f32) -> f32 {
            // A previous-input/output recurrence is independent of the production TPT integrator state.
            let output = self.pole * self.previous_output
                + self.b0 * f64::from(input)
                + self.b1 * self.previous_input;
            self.previous_input = f64::from(input);
            self.previous_output = output;
            output as f32
        }

        fn response(self, omega: f64) -> (f64, f64) {
            let (sine, cosine) = omega.sin_cos();
            let (nr, ni) = (self.b0 + self.b1 * cosine, -self.b1 * sine);
            let (dr, di) = (1.0 - self.pole * cosine, self.pole * sine);
            let square = dr * dr + di * di;
            ((nr * dr + ni * di) / square, (ni * dr - nr * di) / square)
        }
    }

    fn reference(
        rate: u32,
        frames: usize,
        times_ms: [f32; 2],
        mode: Mode,
        input: impl Fn(usize, usize) -> f32,
        switches: impl Fn(usize) -> Switches,
    ) -> Vec<[f64; 2]> {
        let mut hp = [DifferenceFilter::new(rate, mode.hp_hz, true); 2];
        let mut lp = [DifferenceFilter::new(rate, mode.lp_hz, false); 2];
        let mut writes: Vec<[f64; 2]> = Vec::with_capacity(frames);
        let mut output = Vec::with_capacity(frames);
        for frame in 0..frames {
            let wet: [f32; 2] = std::array::from_fn(|channel| {
                let distance = f64::from(times_ms[channel]) * f64::from(rate) / 1000.0;
                let whole = distance.floor() as usize;
                let sample = |delay| {
                    frame
                        .checked_sub(delay)
                        .map(|at| writes[at][channel])
                        .unwrap_or(0.0)
                };
                (sample(whole) * (1.0 - distance.fract()) + sample(whole + 1) * distance.fract())
                    as f32
            });
            let state = switches(frame);
            let routed = if state.ping { [wet[1], wet[0]] } else { wet };
            let next = std::array::from_fn(|channel| {
                let high = hp[channel].step(routed[channel]);
                let after_hp = if state.hp { high } else { routed[channel] };
                let low = lp[channel].step(after_hp);
                let filtered = if state.lp { low } else { after_hp };
                f64::from(input(frame, channel)) + 0.5 * f64::from(filtered)
            });
            writes.push(next);
            output.push(wet.map(f64::from));
        }
        output
    }

    fn prepare(rate: u32, times_ms: [f32; 2], mode: Mode) -> DelayEffect {
        let mut effect = prepared(rate, times_ms[0], 1.0);
        for (index, value) in [
            (param::FEEDBACK, 0.5),
            (param::LINK, 0.0),
            (param::RIGHT_TIME_MS, times_ms[1]),
            (param::FEEDBACK_HP_HZ, mode.hp_hz),
            (param::FEEDBACK_LP_HZ, mode.lp_hz),
            (param::FEEDBACK_HP_ENABLED, f32::from(u8::from(mode.hp))),
            (param::FEEDBACK_LP_ENABLED, f32::from(u8::from(mode.lp))),
        ] {
            assert!(effect.set_param(index, value));
        }
        effect
    }

    fn impulse(frame: usize, channel: usize) -> f32 {
        if frame == 0 {
            [0.125, -0.0625][channel]
        } else {
            0.0
        }
    }

    fn capture(
        effect: &mut DelayEffect,
        input: &mut [f32],
        parts: &[usize],
        observation: &mut Observation,
    ) {
        let mut frame = 0;
        let mut part = 0;
        while frame < input.len() / 2 {
            let end = (frame + parts[part % parts.len()]).min(input.len() / 2);
            measured(
                || effect.process(&mut input[2 * frame..2 * end], 2),
                &mut observation.heap,
            );
            frame = end;
            part += 1;
        }
    }

    fn finish(
        observation: Observation,
        scenario: &str,
        rate: u32,
        parts: &[usize],
        response_error: f64,
        exact_mismatches: usize,
        rows: &mut Vec<Value>,
    ) -> bool {
        let matches = observation.heap == [0; 3]
            && observation.mismatches == 0
            && observation.rejected_expected_edits == 0
            && observation.target_mismatches == 0
            && response_error <= RESPONSE_TOLERANCE
            && exact_mismatches == 0;
        rows.push(json!({ "scenario": scenario, "rate": rate, "partitions": parts, "matches": matches,
            "heap_alloc_realloc_dealloc": observation.heap, "sample_mismatches": observation.mismatches,
            "maximum_error": observation.maximum_error, "first_mismatch": observation.first_mismatch,
            "unexpected_rejected_edits": observation.rejected_expected_edits,
            "target_bit_mismatches": observation.target_mismatches,
            "maximum_complex_transfer_error": response_error, "exact_mismatches": exact_mismatches }));
        matches
    }

    fn transfer_error(rate: u32, mode: Mode, delay: usize, actual: &[f32]) -> f64 {
        let hp = DifferenceFilter::new(rate, mode.hp_hz, true);
        let lp = DifferenceFilter::new(rate, mode.lp_hz, false);
        for filter in [hp, lp] {
            assert!(filter.pole.abs() < 1.0);
            for index in 0..=512 {
                let (re, im) = filter.response(std::f64::consts::PI * index as f64 / 512.0);
                assert!(re * re + im * im <= 1.0 + 2.0e-12);
            }
        }
        for (filter, cutoff) in [(hp, mode.hp_hz), (lp, mode.lp_hz)] {
            let omega = std::f64::consts::TAU * f64::from(cutoff).min(0.45 * f64::from(rate))
                / f64::from(rate);
            let (re, im) = filter.response(omega);
            assert!((re * re + im * im - 0.5).abs() < 2.0e-12);
        }
        let mut frequencies: Vec<_> = (0..=32)
            .map(|i| std::f64::consts::PI * i as f64 / 32.0)
            .collect();
        for cutoff in [mode.hp_hz, mode.lp_hz] {
            frequencies.push(
                std::f64::consts::TAU * f64::from(cutoff).min(0.45 * f64::from(rate))
                    / f64::from(rate),
            );
        }
        let mut maximum = 0.0_f64;
        for omega in frequencies {
            let high = if mode.hp {
                hp.response(omega)
            } else {
                (1.0, 0.0)
            };
            let low = if mode.lp {
                lp.response(omega)
            } else {
                (1.0, 0.0)
            };
            let expected = (
                high.0 * low.0 - high.1 * low.1,
                high.0 * low.1 + high.1 * low.0,
            );
            let mut observed = (0.0, 0.0);
            for frame in 0..delay {
                let value = f64::from(actual[2 * (2 * delay + frame)]) / (0.125 * 0.5);
                let (sine, cosine) = (omega * frame as f64).sin_cos();
                observed.0 += value * cosine;
                observed.1 -= value * sine;
            }
            maximum = maximum.max((observed.0 - expected.0).hypot(observed.1 - expected.1));
        }
        maximum
    }

    fn stationary_modes(rate: u32, parts: &[usize], rows: &mut Vec<Value>) -> usize {
        let modes = [
            Mode {
                name: "both_bypassed_identity_response",
                hp: false,
                lp: false,
                hp_hz: 1000.0,
                lp_hz: 1000.0,
            },
            Mode {
                name: "hp_feedback_only_first_repeat_unfiltered",
                hp: true,
                lp: false,
                hp_hz: 120.0,
                lp_hz: 8000.0,
            },
            Mode {
                name: "lp_feedback_only_first_repeat_unfiltered",
                hp: false,
                lp: true,
                hp_hz: 120.0,
                lp_hz: 1000.0,
            },
            Mode {
                name: "hp_then_lp_including_low_rate_effective_cutoff",
                hp: true,
                lp: true,
                hp_hz: 1000.0,
                lp_hz: 20000.0,
            },
        ];
        let time_ms = if rate == 1001 { 1000.0 } else { 100.0 };
        let delay = (f64::from(time_ms) * f64::from(rate) / 1000.0) as usize;
        let frames = 3 * delay + 2;
        let mut failures = 0;
        for mode in modes {
            let expected = reference(rate, frames, [time_ms; 2], mode, impulse, |_| Switches {
                hp: mode.hp,
                lp: mode.lp,
                ping: false,
            });
            let mut effect = prepare(rate, [time_ms; 2], mode);
            let mut actual: Vec<_> = (0..2 * frames).map(|i| impulse(i / 2, i % 2)).collect();
            let mut observation = Observation::default();
            capture(&mut effect, &mut actual, parts, &mut observation);
            for (i, &sample) in actual.iter().enumerate() {
                observation.sample(i / 2, i % 2, sample, expected[i / 2][i % 2]);
            }
            let exact = usize::from(actual[2 * delay].to_bits() != 0.125_f32.to_bits())
                + usize::from(actual[2 * delay + 1].to_bits() != (-0.0625_f32).to_bits());
            let error = transfer_error(rate, mode, delay, &actual);
            failures += usize::from(!finish(
                observation,
                mode.name,
                rate,
                parts,
                error,
                exact,
                rows,
            ));
        }
        failures
    }

    fn warm_input(frame: usize, channel: usize) -> f32 {
        if channel == 0 {
            0.03125 + rich(frame, channel) / 16.0
        } else {
            -0.015625 + rich(frame, channel) / 32.0
        }
    }

    fn warm_bypass_and_reset(rate: u32, parts: &[usize], rows: &mut Vec<Value>) -> bool {
        let mode = Mode {
            name: "routed_hp_and_post_hp_lp_stay_warm_through_bypass",
            hp: false,
            lp: false,
            hp_hz: 120.0,
            lp_hz: 1000.0,
        };
        let span = |milliseconds: f64| (milliseconds * f64::from(rate) / 1000.0).ceil() as usize;
        let hp_on = span(12.0);
        let lp_on = hp_on + span(9.0);
        let hp_off = lp_on + span(7.0);
        let lp_off = hp_off + span(8.0);
        let end = lp_off + span(10.0);
        let expected = reference(rate, end, [3.0, 5.0], mode, warm_input, |frame| Switches {
            hp: (hp_on..hp_off).contains(&frame),
            lp: (lp_on..lp_off).contains(&frame),
            ping: frame >= hp_on,
        });
        let oracle = |frame: usize, channel| expected[frame][channel];
        let mut effect = prepare(rate, [3.0, 5.0], mode);
        let mut observation = Observation::default();
        render(
            &mut effect,
            0..hp_on,
            parts,
            warm_input,
            oracle,
            &mut observation,
        );
        observation.edit(&mut effect, param::ROUTING, 1.0, true);
        observation.edit(&mut effect, param::FEEDBACK_HP_ENABLED, 1.0, true);
        render(
            &mut effect,
            hp_on..hp_on + span(2.0),
            parts,
            warm_input,
            oracle,
            &mut observation,
        );
        observation.edit(&mut effect, param::FEEDBACK_HP_ENABLED, 1.0, true);
        observation.edit(&mut effect, param::FEEDBACK_HP_HZ, 120.0, false);
        render(
            &mut effect,
            hp_on + span(2.0)..lp_on,
            parts,
            warm_input,
            oracle,
            &mut observation,
        );
        observation.edit(&mut effect, param::FEEDBACK_LP_ENABLED, 1.0, true);
        render(
            &mut effect,
            lp_on..lp_on + span(2.0),
            parts,
            warm_input,
            oracle,
            &mut observation,
        );
        observation.edit(&mut effect, param::FEEDBACK_LP_ENABLED, 1.0, true);
        for index in [param::FEEDBACK_HP_HZ, param::FEEDBACK_LP_HZ] {
            let before = effect.get_param(index).to_bits();
            for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
                for automated in [false, true] {
                    let accepted = measured(
                        || {
                            if automated {
                                effect.set_automated_param(index, value)
                            } else {
                                effect.set_param(index, value)
                            }
                        },
                        &mut observation.heap,
                    );
                    observation.rejected_expected_edits += usize::from(accepted);
                    observation.target_mismatches +=
                        usize::from(effect.get_param(index).to_bits() != before);
                }
            }
        }
        render(
            &mut effect,
            lp_on + span(2.0)..hp_off,
            parts,
            warm_input,
            oracle,
            &mut observation,
        );
        observation.edit(&mut effect, param::FEEDBACK_HP_ENABLED, 0.0, true);
        render(
            &mut effect,
            hp_off..lp_off,
            parts,
            warm_input,
            oracle,
            &mut observation,
        );
        observation.edit(&mut effect, param::FEEDBACK_LP_ENABLED, 0.0, true);
        render(
            &mut effect,
            lp_off..end,
            parts,
            warm_input,
            oracle,
            &mut observation,
        );
        for index in [param::FEEDBACK_HP_ENABLED, param::FEEDBACK_LP_ENABLED] {
            observation.edit(&mut effect, index, 1.0, true);
        }
        measured(|| effect.reset(), &mut observation.heap);
        let frames = span(32.0);
        let expected = reference(rate, frames, [3.0, 5.0], mode, impulse, |_| Switches {
            hp: true,
            lp: true,
            ping: true,
        });
        render(
            &mut effect,
            0..frames,
            parts,
            impulse,
            |frame, channel| expected[frame][channel],
            &mut observation,
        );
        finish(observation, mode.name, rate, parts, 0.0, 0, rows)
    }

    fn disabled_frequency_is_exactly_neutral(
        rate: u32,
        parts: &[usize],
        rows: &mut Vec<Value>,
    ) -> bool {
        let mode = Mode {
            name: "disabled_cutoff_values_do_not_change_any_audio_bit",
            hp: false,
            lp: false,
            hp_hz: 20.0,
            lp_hz: 20000.0,
        };
        let mut baseline = prepare(rate, [3.0, 5.0], mode);
        let mut changed = prepare(
            rate,
            [3.0, 5.0],
            Mode {
                hp_hz: 1000.0,
                lp_hz: 1000.0,
                ..mode
            },
        );
        for effect in [&mut baseline, &mut changed] {
            assert!(effect.set_param(param::ROUTING, 1.0));
            assert!(effect.set_param(param::MIX, 0.375));
        }
        let frames = (0.040 * f64::from(rate)).ceil() as usize;
        let mut first: Vec<_> = (0..2 * frames).map(|i| warm_input(i / 2, i % 2)).collect();
        let mut second = first.clone();
        let mut observation = Observation::default();
        capture(&mut baseline, &mut first, parts, &mut observation);
        capture(&mut changed, &mut second, parts, &mut observation);
        let exact = first
            .iter()
            .zip(&second)
            .filter(|(left, right)| left.to_bits() != right.to_bits())
            .count();
        for (i, (&actual, &expected)) in second.iter().zip(&first).enumerate() {
            observation.sample(i / 2, i % 2, actual, f64::from(expected));
        }
        finish(observation, mode.name, rate, parts, 0.0, exact, rows)
    }

    #[test]
    fn feedback_hp_lp_match_transfer_functions_and_preserve_warm_exact_bypass() {
        let mut rows = Vec::new();
        let mut failures = 0;
        for rate in [1000, 1001, 48_000] {
            for parts in [&[1][..], &[7, 31, 64, 1, 128, 512][..]] {
                failures += stationary_modes(rate, parts, &mut rows);
                failures += usize::from(!warm_bypass_and_reset(rate, parts, &mut rows));
                failures += usize::from(!disabled_frequency_is_exactly_neutral(
                    rate, parts, &mut rows,
                ));
            }
        }
        println!(
            "DELAY_FEEDBACK_FILTER_RECORD {}",
            json!({
                "execution_head": std::env::var("VIBEZ_VERIFICATION_HEAD").unwrap_or_else(|_| "UNLABELLED".into()),
                "oracle": "Independent BLT sin/cos transfer coefficients and previous-input/output difference equations; routed delayed -> warm HP -> warm LP, unfiltered wet output",
                "sample_tolerance": SAMPLE_TOLERANCE, "complex_transfer_tolerance": RESPONSE_TOLERANCE,
                "observed": rows, "scope": "Static network and exact/warm bypass; manual moving enable/Hz ramps and maximum-feedback stability remain later slices",
            })
        );
        assert_eq!(
            failures, 0,
            "Feedback filter network failures; see all observations above"
        );
    }

    mod edits {
        use super::*;

        const FIELDS: [usize; 4] = [
            param::FEEDBACK_HP_ENABLED,
            param::FEEDBACK_LP_ENABLED,
            param::FEEDBACK_HP_HZ,
            param::FEEDBACK_LP_HZ,
        ];
        const INITIAL: Mode = Mode {
            name: "manual_filter_edits",
            hp: false,
            lp: true,
            hp_hz: 20.0,
            lp_hz: 20000.0,
        };

        #[derive(Clone, Copy)]
        struct Controls {
            hp: f64,
            lp: f64,
            hp_hz: f64,
            lp_hz: f64,
            routing: f64,
        }

        fn effective(rate: u32, hertz: f32) -> f64 {
            f64::from(hertz).min(0.45 * f64::from(rate))
        }

        fn initial(rate: u32, mode: Mode) -> Controls {
            Controls {
                hp: f64::from(u8::from(mode.hp)),
                lp: f64::from(u8::from(mode.lp)),
                hp_hz: effective(rate, mode.hp_hz),
                lp_hz: effective(rate, mode.lp_hz),
                routing: 0.0,
            }
        }

        fn coefficient(rate: u32, hertz: f64) -> f64 {
            let (sine, cosine) = (std::f64::consts::PI * hertz / f64::from(rate)).sin_cos();
            sine / (sine + cosine)
        }

        #[derive(Clone, Copy)]
        struct PreviousLow {
            low: f64,
            input: f64,
            g: f64,
        }

        impl PreviousLow {
            fn step(&mut self, input: f32, g: f64) -> f64 {
                // Eliminating the integrator requires the previous G when coefficients move.
                let low = g * f64::from(input)
                    + (1.0 - g) / (1.0 - self.g)
                        * ((1.0 - 2.0 * self.g) * self.low + self.g * self.input);
                *self = Self {
                    low,
                    input: f64::from(input),
                    g,
                };
                low
            }
        }

        fn blend(raw: f32, filtered: f32, amount: f64) -> f32 {
            if amount == 0.0 {
                raw
            } else if amount == 1.0 {
                filtered
            } else {
                ((1.0 - amount) * f64::from(raw) + amount * f64::from(filtered)) as f32
            }
        }

        fn trajectory(frame: usize, start: usize, frames: usize, from: f64, to: f64) -> f64 {
            if frame < start {
                from
            } else if frame - start + 1 >= frames {
                to
            } else {
                from + (to - from) * (frame - start + 1) as f64 / frames as f64
            }
        }

        fn reference_edits(
            rate: u32,
            frames: usize,
            times: [f32; 2],
            mode: Mode,
            controls: impl Fn(usize) -> Controls,
            input: impl Fn(usize, usize) -> f32,
        ) -> Vec<[f64; 2]> {
            let mut hp = [PreviousLow {
                low: 0.0,
                input: 0.0,
                g: coefficient(rate, effective(rate, mode.hp_hz)),
            }; 2];
            let mut lp = [PreviousLow {
                low: 0.0,
                input: 0.0,
                g: coefficient(rate, effective(rate, mode.lp_hz)),
            }; 2];
            let mut writes: Vec<[f64; 2]> = Vec::with_capacity(frames);
            let mut output = Vec::with_capacity(frames);
            for frame in 0..frames {
                let wet: [f32; 2] = std::array::from_fn(|channel| {
                    let distance = f64::from(times[channel]) * f64::from(rate) / 1000.0;
                    let sample = |delay| {
                        frame
                            .checked_sub(delay)
                            .map(|at| writes[at][channel])
                            .unwrap_or(0.0)
                    };
                    (sample(distance.floor() as usize) * (1.0 - distance.fract())
                        + sample(distance.floor() as usize + 1) * distance.fract())
                        as f32
                });
                let c = controls(frame);
                let routed = [
                    blend(wet[0], wet[1], c.routing),
                    blend(wet[1], wet[0], c.routing),
                ];
                let next = std::array::from_fn(|channel| {
                    let low = hp[channel].step(routed[channel], coefficient(rate, c.hp_hz));
                    let high = (f64::from(routed[channel]) - low) as f32;
                    let after_hp = blend(routed[channel], high, c.hp);
                    let low = lp[channel].step(after_hp, coefficient(rate, c.lp_hz)) as f32;
                    f64::from(input(frame, channel)) + 0.5 * f64::from(blend(after_hp, low, c.lp))
                });
                writes.push(next);
                output.push(wet.map(f64::from));
            }
            output
        }

        fn render_changes(
            effect: &mut DelayEffect,
            expected: &[[f64; 2]],
            changes: &[(usize, usize, f32, bool)],
            parts: &[usize],
            input: impl Fn(usize, usize) -> f32,
            observation: &mut Observation,
        ) {
            let mut start = 0;
            for &(at, index, value, automated) in changes {
                assert!(at >= start && at <= expected.len());
                render(
                    effect,
                    start..at,
                    parts,
                    &input,
                    |frame, channel| expected[frame][channel],
                    observation,
                );
                if value.is_finite() {
                    observation.edit(effect, index, value, automated);
                } else {
                    let before = effect.get_param(index).to_bits();
                    let accepted = measured(
                        || {
                            if automated {
                                effect.set_automated_param(index, value)
                            } else {
                                effect.set_param(index, value)
                            }
                        },
                        &mut observation.heap,
                    );
                    observation.rejected_expected_edits += usize::from(accepted);
                    observation.target_mismatches +=
                        usize::from(effect.get_param(index).to_bits() != before);
                }
                start = at;
            }
            render(
                effect,
                start..expected.len(),
                parts,
                input,
                |frame, channel| expected[frame][channel],
                observation,
            );
        }

        fn manual_retarget_and_reset(rate: u32, parts: &[usize], rows: &mut Vec<Value>) -> bool {
            let n = (rate as usize).div_ceil(100);
            let start = 2 * n;
            let retarget = start + n / 2;
            let end = retarget + n / 3;
            let hz = |frame, from, target, next| {
                let first = trajectory(frame, start, n, from, target);
                if frame < retarget {
                    first
                } else {
                    trajectory(
                        frame,
                        retarget,
                        n,
                        trajectory(retarget - 1, start, n, from, target),
                        next,
                    )
                }
            };
            let expected = reference_edits(
                rate,
                end,
                [3.0, 5.0],
                INITIAL,
                |frame| Controls {
                    hp: f64::from(trajectory(frame, start, n, 0.0, 1.0) as f32),
                    lp: f64::from(trajectory(frame, start, n, 1.0, 0.0) as f32),
                    hp_hz: hz(
                        frame,
                        effective(rate, 20.0),
                        effective(rate, 1000.0),
                        effective(rate, 100.0),
                    ),
                    lp_hz: hz(
                        frame,
                        effective(rate, 20000.0),
                        effective(rate, 1000.0),
                        effective(rate, 5000.0),
                    ),
                    routing: 0.0,
                },
                warm_input,
            );
            let mut changes: Vec<_> = FIELDS
                .into_iter()
                .zip([1.0, 0.0, 1000.0, 1000.0])
                .map(|(field, value)| (start, field, value, false))
                .collect();
            changes.extend(
                FIELDS
                    .into_iter()
                    .zip([1.0, 0.0, 1000.0, 1000.0])
                    .map(|(field, value)| (start + n / 3, field, value, false)),
            );
            changes.extend([
                (retarget, param::FEEDBACK_HP_HZ, 100.0, false),
                (retarget, param::FEEDBACK_LP_HZ, 5000.0, false),
            ]);
            let mut effect = prepare(rate, [3.0, 5.0], INITIAL);
            let mut observation = Observation::default();
            render_changes(
                &mut effect,
                &expected,
                &changes,
                parts,
                warm_input,
                &mut observation,
            );
            // Reset occurs before any of the retargeted frequency trajectories has finished.
            measured(|| effect.reset(), &mut observation.heap);
            let reset_mode = Mode {
                hp: true,
                lp: false,
                hp_hz: 100.0,
                lp_hz: 5000.0,
                ..INITIAL
            };
            let expected = reference_edits(
                rate,
                3 * n,
                [3.0, 5.0],
                reset_mode,
                |_| initial(rate, reset_mode),
                impulse,
            );
            render_changes(
                &mut effect,
                &expected,
                &[],
                parts,
                impulse,
                &mut observation,
            );
            measured(|| effect.reset(), &mut observation.heap);
            for (field, value) in FIELDS.into_iter().zip([0.0, 1.0, 37.0, 10000.0]) {
                observation.edit(&mut effect, field, value, false);
            }
            let pre_audio = Mode {
                hp: false,
                lp: true,
                hp_hz: 37.0,
                lp_hz: 10000.0,
                ..INITIAL
            };
            let expected = reference_edits(
                rate,
                3 * n,
                [3.0, 5.0],
                pre_audio,
                |_| initial(rate, pre_audio),
                impulse,
            );
            render_changes(
                &mut effect,
                &expected,
                &[],
                parts,
                impulse,
                &mut observation,
            );
            finish(
                observation,
                "manual_retarget_noop_reset_and_pre_audio",
                rate,
                parts,
                0.0,
                0,
                rows,
            )
        }

        fn clamp_equivalent(
            rate: u32,
            parts: &[usize],
            automated: bool,
            rows: &mut Vec<Value>,
        ) -> bool {
            let mode = Mode {
                hp: true,
                lp: false,
                ..INITIAL
            };
            let n = (rate as usize).div_ceil(100);
            let start = 2 * n;
            let edit = start + n / 2;
            let cap = effective(rate, 1000.0);
            assert_eq!(cap, effective(rate, 500.0));
            let expected = reference_edits(
                rate,
                5 * n,
                [3.0, 5.0],
                mode,
                |frame| Controls {
                    hp_hz: if automated && frame >= edit {
                        cap
                    } else {
                        trajectory(frame, start, n, 20.0, cap)
                    },
                    ..initial(rate, mode)
                },
                warm_input,
            );
            let changes = [
                (start, param::FEEDBACK_HP_HZ, 1000.0, false),
                (start + n / 3, param::FEEDBACK_HP_HZ, 1000.0, false),
                (edit, param::FEEDBACK_HP_HZ, 500.0, automated),
                (edit, param::FEEDBACK_HP_HZ, f32::NAN, false),
                (edit, param::FEEDBACK_LP_HZ, f32::INFINITY, true),
                (edit + 1, param::FEEDBACK_HP_HZ, 500.0, automated),
            ];
            let mut effect = prepare(rate, [3.0, 5.0], mode);
            let mut observation = Observation::default();
            render_changes(
                &mut effect,
                &expected,
                &changes,
                parts,
                warm_input,
                &mut observation,
            );
            finish(
                observation,
                if automated {
                    "clamp_equivalent_automation_snaps_own_hz"
                } else {
                    "clamp_equivalent_manual_preserves_original_trajectory"
                },
                rate,
                parts,
                0.0,
                0,
                rows,
            )
        }

        fn own_automation(parts: &[usize], own: usize, rows: &mut Vec<Value>) -> bool {
            let rate = 48000;
            let n = (rate as usize).div_ceil(100);
            let start = 2 * n;
            let snap = start + n / 2;
            let targets = [1.0, 0.0, 1000.0, 1000.0];
            let expected = reference_edits(
                rate,
                6 * n,
                [3.0, 5.0],
                INITIAL,
                |frame| {
                    let value = |field, from, to| {
                        if FIELDS[field] == own && frame >= snap {
                            to
                        } else {
                            trajectory(frame, start, n, from, to)
                        }
                    };
                    Controls {
                        hp: f64::from(value(0, 0.0, 1.0) as f32),
                        lp: f64::from(value(1, 1.0, 0.0) as f32),
                        hp_hz: value(2, 20.0, 1000.0),
                        lp_hz: value(3, 20000.0, 1000.0),
                        routing: f64::from(trajectory(
                            frame,
                            start,
                            (rate as usize).div_ceil(50),
                            0.0,
                            1.0,
                        ) as f32),
                    }
                },
                warm_input,
            );
            let mut changes: Vec<_> = FIELDS
                .into_iter()
                .zip(targets)
                .map(|(field, value)| (start, field, value, false))
                .collect();
            changes.push((start, param::ROUTING, 1.0, false));
            let target = targets[FIELDS.iter().position(|&field| field == own).unwrap()];
            changes.extend([(snap, own, target, true), (snap + n / 3, own, target, true)]);
            let mut effect = prepare(rate, [3.0, 5.0], INITIAL);
            let mut observation = Observation::default();
            render_changes(
                &mut effect,
                &expected,
                &changes,
                parts,
                warm_input,
                &mut observation,
            );
            let scenario = match own {
                param::FEEDBACK_HP_ENABLED => "hp_enable_automation_owns_only_hp_enable",
                param::FEEDBACK_LP_ENABLED => "lp_enable_automation_owns_only_lp_enable",
                param::FEEDBACK_HP_HZ => "hp_hz_automation_owns_only_hp_hz",
                param::FEEDBACK_LP_HZ => "lp_hz_automation_owns_only_lp_hz",
                _ => unreachable!(),
            };
            finish(observation, scenario, rate, parts, 0.0, 0, rows)
        }

        fn exact_effective_endpoint(parts: &[usize], rows: &mut Vec<Value>) -> bool {
            let rate = 1001;
            let n = (rate as usize).div_ceil(100);
            let delay = rate as usize;
            let start = delay - (n - 1);
            let mode = Mode {
                hp: true,
                lp: false,
                ..INITIAL
            };
            let expected = reference_edits(
                rate,
                2 * delay + 1,
                [1000.0; 2],
                mode,
                |frame| Controls {
                    hp_hz: trajectory(frame, start, n, 20.0, effective(rate, 1000.0)),
                    ..initial(rate, mode)
                },
                impulse,
            );
            // Integer delay isolates the very first filtered return, without prior filter input.
            assert_eq!((expected[2 * delay][0] as f32).to_bits(), 0x3c0c029e);
            let changes = [
                (start, param::FEEDBACK_HP_HZ, 1000.0, false),
                (start + n / 3, param::FEEDBACK_HP_HZ, 500.0, false),
            ];
            let mut effect = prepare(rate, [1000.0; 2], mode);
            let mut observation = Observation::default();
            render_changes(
                &mut effect,
                &expected[..2 * delay],
                &changes,
                parts,
                impulse,
                &mut observation,
            );
            let mut last = [0.0; 2];
            measured(|| effect.process(&mut last, 2), &mut observation.heap);
            for channel in 0..2 {
                observation.sample(
                    2 * delay,
                    channel,
                    last[channel],
                    expected[2 * delay][channel],
                );
            }
            let exact = usize::from(last[0].to_bits() != 0x3c0c029e);
            finish(
                observation,
                "effective_f64_cutoff_exact_final_frame_no_restart",
                rate,
                parts,
                0.0,
                exact,
                rows,
            )
        }

        #[test]
        fn manual_filter_fields_ramp_for_ten_ms_and_automation_owns_only_its_field() {
            let mut rows = Vec::new();
            let mut failures = 0;
            for parts in [&[1][..], &[7, 31, 64, 1, 128, 512][..]] {
                for rate in [1000, 1001, 48000] {
                    failures += usize::from(!manual_retarget_and_reset(rate, parts, &mut rows));
                    if rate != 48000 {
                        for automated in [false, true] {
                            failures +=
                                usize::from(!clamp_equivalent(rate, parts, automated, &mut rows));
                        }
                    }
                }
                for field in FIELDS {
                    failures += usize::from(!own_automation(parts, field, &mut rows));
                }
                failures += usize::from(!exact_effective_endpoint(parts, &mut rows));
            }
            assert_eq!(rows.len(), 24);
            println!(
                "DELAY_FILTER_EDIT_RECORD {}",
                json!({
                    "execution_head": std::env::var("VIBEZ_VERIFICATION_HEAD").unwrap_or_else(|_| "UNLABELLED".into()),
                    "oracle": "Eliminated-integrator varying-TPT previous-low/input/G recurrence, independent sin/cos G; exact f64 effectiveHz and separate k/N field trajectories",
                    "sample_tolerance": SAMPLE_TOLERANCE, "observed": rows,
                    "scope": "Small-signal field trajectories and exact ownership; maximum-feedback moving stability is separate",
                })
            );
            assert_eq!(
                failures, 0,
                "Filter edit trajectory/ownership failures; see observations above"
            );
        }
    }
}

mod duck {
    use super::*;

    fn finite(sample: f32) -> f32 {
        if sample.is_finite() {
            sample
        } else {
            0.0
        }
    }

    fn plateau(prior: f64, peak: f64, frames: usize, rate: u32, tau: f64) -> f64 {
        peak + (prior - peak) * (-(frames as f64) / (tau * f64::from(rate))).exp()
    }

    fn gain(envelope: f64, amount: f32) -> f64 {
        if amount == 0.0 {
            1.0
        } else {
            10.0_f64.powf(-24.0 * f64::from(amount) * (envelope / 0.25).clamp(0.0, 1.0) / 20.0)
        }
    }

    fn unducked_wet(
        rate: u32,
        frames: usize,
        channels: usize,
        time_ms: f32,
        feedback: f32,
        input: impl Fn(usize, usize) -> f32,
    ) -> Vec<[f32; 2]> {
        let distance = f64::from(time_ms) * f64::from(rate) / 1000.0;
        let mut writes: Vec<[f32; 2]> = Vec::with_capacity(frames);
        let mut output = Vec::with_capacity(frames);
        for frame in 0..frames {
            let wet: [f32; 2] = std::array::from_fn(|channel| {
                let sample = |delay| {
                    frame
                        .checked_sub(delay)
                        .map(|at| f64::from(writes[at][channel]))
                        .unwrap_or(0.0)
                };
                (sample(distance.floor() as usize) * (1.0 - distance.fract())
                    + sample(distance.floor() as usize + 1) * distance.fract())
                    as f32
            });
            writes.push(std::array::from_fn(|channel| {
                let dry = if channel < channels {
                    finite(input(frame, channel))
                } else {
                    0.0
                };
                dry + feedback * wet[channel]
            }));
            output.push(wet);
        }
        output
    }

    fn output(dry: f32, wet: f32, mix: f32, amount: f32, envelope: f64) -> f64 {
        let ducked = (f64::from(wet) * gain(envelope, amount)) as f32;
        f64::from(finite(dry) * (1.0 - mix) + ducked * mix)
    }

    fn capture(
        effect: &mut DelayEffect,
        audio: &mut [f32],
        channels: usize,
        parts: &[usize],
        observation: &mut Observation,
    ) {
        let mut frame = 0;
        let mut part = 0;
        while frame < audio.len() / channels {
            let end = (frame + parts[part % parts.len()]).min(audio.len() / channels);
            measured(
                || effect.process(&mut audio[channels * frame..channels * end], channels),
                &mut observation.heap,
            );
            frame = end;
            part += 1;
        }
    }

    fn pulse(frame: usize, channel: usize, low_end: usize, high_end: usize) -> f32 {
        if frame < low_end {
            [0.0625, -0.125][channel]
        } else if frame < high_end {
            [0.5, -0.25][channel]
        } else if frame == high_end + 2 {
            [f32::NAN, f32::INFINITY][channel]
        } else if frame == high_end + 3 {
            [f32::NEG_INFINITY, f32::NAN][channel]
        } else {
            0.0
        }
    }

    fn analytic_envelope(
        frame: usize,
        channels: usize,
        low_end: usize,
        high_end: usize,
        rate: u32,
    ) -> f64 {
        let low = if channels == 1 { 0.0625 } else { 0.125 };
        let low_final = plateau(0.0, low, low_end, rate, 0.005);
        if frame < low_end {
            plateau(0.0, low, frame + 1, rate, 0.005)
        } else if frame < high_end {
            plateau(low_final, 0.5, frame - low_end + 1, rate, 0.005)
        } else {
            let high_final = plateau(low_final, 0.5, high_end - low_end, rate, 0.005);
            plateau(high_final, 0.0, frame - high_end + 1, rate, 0.150)
        }
    }

    fn analytic_attack_release(
        rate: u32,
        parts: &[usize],
        channels: usize,
        warm: bool,
        rows: &mut Vec<Value>,
    ) -> bool {
        let span = |seconds: f64| (seconds * f64::from(rate)).ceil() as usize;
        let low_end = span(0.040);
        let high_end = low_end + span(0.020);
        let duck_off = high_end + span(0.300);
        let end = duck_off + span(0.080);
        let duck_on = if warm { span(0.030) } else { 0 };
        let input = |frame, channel| pulse(frame, channel, low_end, high_end);
        let wet = unducked_wet(rate, end, channels, 20.0, 0.75, input);
        let mut actual: Vec<_> = (0..channels * end)
            .map(|i| input(i / channels, i % channels))
            .collect();
        let mut baseline = actual.clone();
        let mut effect = prepared(rate, 20.0, 0.375);
        let mut neutral = prepared(rate, 20.0, 0.375);
        for effect in [&mut effect, &mut neutral] {
            assert!(effect.set_param(param::FEEDBACK, 0.75));
        }
        let mut observation = Observation::default();
        observation.edit(
            &mut effect,
            param::DUCK,
            if warm { 0.0 } else { 0.75 },
            false,
        );
        capture(
            &mut neutral,
            &mut baseline,
            channels,
            parts,
            &mut observation,
        );
        capture(
            &mut effect,
            &mut actual[..channels * duck_on],
            channels,
            parts,
            &mut observation,
        );
        if warm {
            observation.edit(&mut effect, param::DUCK, 0.75, true);
        }
        capture(
            &mut effect,
            &mut actual[channels * duck_on..channels * duck_off],
            channels,
            parts,
            &mut observation,
        );
        observation.edit(&mut effect, param::DUCK, 0.0, true);
        capture(
            &mut effect,
            &mut actual[channels * duck_off..],
            channels,
            parts,
            &mut observation,
        );
        for (index, &sample) in actual.iter().enumerate() {
            let (frame, channel) = (index / channels, index % channels);
            let amount = if frame < duck_on || frame >= duck_off {
                0.0
            } else {
                0.75
            };
            let expected = output(
                input(frame, channel),
                wet[frame][channel],
                0.375,
                amount,
                analytic_envelope(frame, channels, low_end, high_end, rate),
            );
            observation.sample(frame, channel, sample, expected);
            // Equality after bypass also detects any attenuation incorrectly stored in feedback.
            if amount == 0.0 || wet[frame][channel] == 0.0 {
                observation.exact_endpoint_mismatches +=
                    usize::from(sample.to_bits() != baseline[index].to_bits());
            }
        }
        let scenario = if warm {
            "zero_amount_is_exact_and_detector_warm_before_enable"
        } else if channels == 1 {
            "mono_analytic_attack_release_and_unchanged_feedback"
        } else {
            "stereo_linked_peak_analytic_attack_release_and_unchanged_feedback"
        };
        observation.finish(scenario, rate, parts, rows)
    }

    fn trajectory(frame: usize, start: usize, frames: usize, from: f32, to: f32) -> f32 {
        if frame < start {
            from
        } else if frame - start + 1 >= frames {
            to
        } else {
            (f64::from(from)
                + (f64::from(to) - f64::from(from)) * (frame - start + 1) as f64 / frames as f64)
                as f32
        }
    }

    fn manual_amount_and_ownership(rate: u32, parts: &[usize], rows: &mut Vec<Value>) -> bool {
        let n = (rate as usize).div_ceil(100);
        let start = 2 * n;
        let retarget = start + n / 2;
        let snap_duck = retarget + n / 3;
        let second = start + 2 * n;
        let snap_mix = second + n / 2;
        let end = snap_mix + n / 3;
        let from = trajectory(retarget - 1, start, n, 0.0, 1.0);
        let amount = |frame| {
            if frame >= second {
                trajectory(frame, second, n, 0.25, 1.0)
            } else if frame >= snap_duck {
                0.25
            } else if frame >= retarget {
                trajectory(frame, retarget, n, from, 0.25)
            } else {
                trajectory(frame, start, n, 0.0, 1.0)
            }
        };
        let mix = |frame| {
            if frame >= snap_mix {
                0.25
            } else if frame >= second {
                trajectory(frame, second, n, 0.75, 0.25)
            } else {
                trajectory(frame, start, n, 0.25, 0.75)
            }
        };
        let wet = unducked_wet(rate, end, 2, 3.0, 0.5, dry);
        let expected = |frame: usize, channel| {
            output(
                dry(frame, channel),
                wet[frame][channel],
                mix(frame),
                amount(frame),
                plateau(0.0, 0.25, frame + 1, rate, 0.005),
            )
        };
        let changes = [
            (start, param::DUCK, 1.0, false),
            (start, param::MIX, 0.75, false),
            (start + n / 3, param::DUCK, 1.0, false),
            (retarget, param::DUCK, 0.25, false),
            (snap_duck, param::DUCK, 0.25, true),
            (snap_duck + 1, param::DUCK, 0.25, true),
            (second, param::DUCK, 1.0, false),
            (second, param::MIX, 0.25, false),
            (second + n / 3, param::DUCK, 1.0, false),
            (snap_mix, param::MIX, 0.25, true),
        ];
        let mut effect = prepared(rate, 3.0, 0.25);
        assert!(effect.set_param(param::FEEDBACK, 0.5));
        let mut observation = Observation::default();
        let mut previous = 0;
        for (at, index, target, automated) in changes {
            render(
                &mut effect,
                previous..at,
                parts,
                dry,
                expected,
                &mut observation,
            );
            observation.edit(&mut effect, index, target, automated);
            if at == retarget {
                for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
                    let accepted = measured(
                        || effect.set_param(param::DUCK, value),
                        &mut observation.heap,
                    );
                    observation.rejected_expected_edits += usize::from(accepted);
                    observation.target_mismatches +=
                        usize::from(effect.get_param(param::DUCK).to_bits() != target.to_bits());
                }
            }
            previous = at;
        }
        render(
            &mut effect,
            previous..end,
            parts,
            dry,
            expected,
            &mut observation,
        );
        // The second amount ramp is still moving when reset must snap its target.
        measured(|| effect.reset(), &mut observation.heap);
        let wet = unducked_wet(rate, 3 * n, 2, 3.0, 0.5, dry);
        render(
            &mut effect,
            0..3 * n,
            parts,
            dry,
            |frame, channel| {
                output(
                    dry(frame, channel),
                    wet[frame][channel],
                    0.25,
                    1.0,
                    plateau(0.0, 0.25, frame + 1, rate, 0.005),
                )
            },
            &mut observation,
        );
        measured(|| effect.reset(), &mut observation.heap);
        observation.edit(&mut effect, param::DUCK, 0.5, false);
        observation.edit(&mut effect, param::MIX, 0.5, false);
        render(
            &mut effect,
            0..3 * n,
            parts,
            dry,
            |frame, channel| {
                output(
                    dry(frame, channel),
                    wet[frame][channel],
                    0.5,
                    0.5,
                    plateau(0.0, 0.25, frame + 1, rate, 0.005),
                )
            },
            &mut observation,
        );
        observation.finish(
            "manual_duck_retarget_noop_exact_own_and_converse_automation_reset",
            rate,
            parts,
            rows,
        )
    }

    #[test]
    fn duck_uses_linked_input_peak_and_wet_only_gain_with_independent_amount_edits() {
        let mut rows = Vec::new();
        let mut failures = 0;
        for rate in [1000, 1001, 48000] {
            for parts in [&[1][..], &[7, 31, 64, 1, 128, 512][..]] {
                for channels in [1, 2] {
                    failures += usize::from(!analytic_attack_release(
                        rate, parts, channels, false, &mut rows,
                    ));
                }
                failures += usize::from(!analytic_attack_release(rate, parts, 2, true, &mut rows));
                failures += usize::from(!manual_amount_and_ownership(rate, parts, &mut rows));
            }
        }
        assert_eq!(rows.len(), 24);
        println!(
            "DELAY_DUCK_RECORD {}",
            json!({
                "execution_head": std::env::var("VIBEZ_VERIFICATION_HEAD").unwrap_or_else(|_| "UNLABELLED".into()),
                "oracle": "Analytic constant-step exp attack/release, chronological unducked feedback writes, wet-only24dB amount law; amount k/N and independent Mix ownership",
                "sample_tolerance": SAMPLE_TOLERANCE, "observed": rows,
            })
        );
        assert_eq!(
            failures, 0,
            "Duck envelope/output/ownership failures; see observations above"
        );
    }
}

#[path = "delay_quality/mod.rs"]
mod quality;
