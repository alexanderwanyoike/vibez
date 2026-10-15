//! Requires bounded stereo Delay history preparation and recoverable real allocation failures.

use serde_json::json;
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
    process::Command,
};
use vibez_dsp::{delay::DelayEffect, effect::AudioEffect};

#[derive(Clone, Copy, Default)]
struct AllocationRecord {
    active: bool,
    fail_at: usize,
    attempts: usize,
    sizes: [usize; 4],
    alignments: [usize; 4],
    successes: usize,
    deallocations: usize,
    live_bytes: usize,
    peak_bytes: usize,
    reallocations: usize,
}

thread_local! {
    static RECORD: Cell<AllocationRecord> = const { Cell::new(AllocationRecord {
        active: false, fail_at: 0, attempts: 0, sizes: [0; 4], alignments: [0; 4],
        successes: 0, deallocations: 0, live_bytes: 0, peak_bytes: 0, reallocations: 0,
    }) };
}

fn allocation_attempt(size: usize, alignment: usize, reallocating: bool) -> bool {
    let mut fail = false;
    let _ = RECORD.try_with(|cell| {
        let mut record = cell.get();
        if record.active {
            let index = record.attempts;
            if index < record.sizes.len() {
                record.sizes[index] = size;
                record.alignments[index] = alignment;
            }
            record.attempts += 1;
            if reallocating {
                record.reallocations += 1;
            }
            fail = record.fail_at == record.attempts;
            cell.set(record);
        }
    });
    fail
}

fn allocated(size: usize, replaced: usize) {
    let _ = RECORD.try_with(|cell| {
        let mut record = cell.get();
        if record.active {
            record.successes += 1;
            record.live_bytes = record.live_bytes.saturating_sub(replaced) + size;
            record.peak_bytes = record.peak_bytes.max(record.live_bytes);
            cell.set(record);
        }
    });
}

struct ConstructorAllocator;

// Fixed thread-local Copy accounting cannot allocate or reenter this allocator.
unsafe impl GlobalAlloc for ConstructorAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if allocation_attempt(layout.size(), layout.align(), false) {
            return std::ptr::null_mut();
        }
        let pointer = unsafe { System.alloc(layout) };
        if !pointer.is_null() {
            allocated(layout.size(), 0);
        }
        pointer
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        if allocation_attempt(layout.size(), layout.align(), false) {
            return std::ptr::null_mut();
        }
        let pointer = unsafe { System.alloc_zeroed(layout) };
        if !pointer.is_null() {
            allocated(layout.size(), 0);
        }
        pointer
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        if allocation_attempt(size, layout.align(), true) {
            return std::ptr::null_mut();
        }
        let replacement = unsafe { System.realloc(pointer, layout, size) };
        if !replacement.is_null() {
            allocated(size, layout.size());
        }
        replacement
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        let _ = RECORD.try_with(|cell| {
            let mut record = cell.get();
            if record.active {
                record.deallocations += 1;
                record.live_bytes = record.live_bytes.saturating_sub(layout.size());
                cell.set(record);
            }
        });
        unsafe { System.dealloc(pointer, layout) };
    }
}

#[global_allocator]
static ALLOCATOR: ConstructorAllocator = ConstructorAllocator;

fn observe_constructor(rate: f32, fail_at: usize) -> (bool, Option<u32>, AllocationRecord, AllocationRecord) {
    RECORD.set(AllocationRecord { active: true, fail_at, ..AllocationRecord::default() });
    let effect = DelayEffect::new(rate);
    let constructed = RECORD.get();
    RECORD.set(AllocationRecord { active: false, ..constructed });
    let valid = effect.processing_configuration_valid();
    let identity = effect.activation_sample_rate();
    RECORD.set(constructed);
    drop(effect);
    let dropped = RECORD.get();
    RECORD.set(AllocationRecord::default());
    (valid, identity, constructed, dropped)
}

fn observation(record: AllocationRecord) -> serde_json::Value {
    json!({
        "attempts": record.attempts,
        "attempted_sizes": record.sizes,
        "attempted_alignments": record.alignments,
        "successful_allocations": record.successes,
        "deallocations": record.deallocations,
        "live_bytes": record.live_bytes,
        "peak_bytes": record.peak_bytes,
        "reallocations": record.reallocations,
    })
}

fn execution_head() -> String {
    std::env::var("VIBEZ_VERIFICATION_HEAD").unwrap_or_else(|_| "UNLABELLED".into())
}

#[test]
fn constructor_prepares_exactly_two_guarded_eighteen_second_histories() {
    let mut failures = Vec::new();
    let mut observations = Vec::new();
    for rate in [1000_u32, 1001, 44_100, 48_000, 88_200, 96_000, 176_400, 192_000, 352_800, 384_000, 466_033] {
        let line_bytes = (18 * u64::from(rate) + 2) * 4;
        let line_bytes = usize::try_from(line_bytes).unwrap();
        let (valid, identity, constructed, dropped) = observe_constructor(rate as f32, 0);
        let matches = valid
            && identity == Some(rate)
            && constructed.attempts == 2
            && constructed.sizes == [line_bytes, line_bytes, 0, 0]
            && constructed.alignments == [std::mem::align_of::<f32>(), std::mem::align_of::<f32>(), 0, 0]
            && constructed.successes == 2
            && constructed.deallocations == 0
            && constructed.live_bytes == 2 * line_bytes
            && constructed.peak_bytes == 2 * line_bytes
            && constructed.reallocations == 0
            && dropped.deallocations == 2
            && dropped.live_bytes == 0;
        if !matches {
            failures.push(format!("rate={rate}: exact two-history allocation contract mismatch"));
        }
        observations.push(json!({
            "rate": rate, "expected_line_bytes": line_bytes,
            "expected_total_bytes": 2 * line_bytes, "budget_bytes": 64 * 1024 * 1024,
            "valid": valid, "identity": identity, "matches": matches,
            "constructed": observation(constructed), "dropped": observation(dropped),
        }));
    }
    for (case, rate, expected_identity) in [
        ("over_budget", 466_034.0, Some(466_034)),
        ("zero", 0.0, Some(0)),
        ("negative", -1000.0, None),
        ("fractional", 1000.5, None),
        ("nan", f32::NAN, None),
        ("infinity", f32::INFINITY, None),
        ("unrepresentable", 4_294_967_296.0, None),
    ] {
        let (valid, identity, constructed, dropped) = observe_constructor(rate, 0);
        let matches = !valid && identity == expected_identity
            && constructed.attempts == 0 && constructed.live_bytes == 0
            && dropped.deallocations == 0;
        if !matches {
            failures.push(format!("rejected rate={case}: must allocate no history"));
        }
        observations.push(json!({
            "rate_case": case, "requested_rate_bits": rate.to_bits(),
            "expected_identity": expected_identity, "valid": valid, "identity": identity,
            "matches": matches, "constructed": observation(constructed), "dropped": observation(dropped),
        }));
    }
    println!("DELAY_HISTORY_RESOURCES_RECORD {}", json!({
        "case": "exact_bounded_history_preparation", "execution_head": execution_head(),
        "observed": observations,
    }));
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn allocation_failure_child() {
    let Ok(ordinal) = std::env::var("VIBEZ_DELAY_FAIL_ALLOCATION") else { return; };
    let ordinal: usize = ordinal.parse().unwrap();
    assert!((1..=2).contains(&ordinal));
    let (valid, identity, constructed, dropped) = observe_constructor(1000.0, ordinal);
    let line_bytes = (18 * 1000 + 2) * 4;
    let matches = !valid && identity == Some(1000)
        && constructed.attempts == ordinal && constructed.successes == ordinal - 1
        && constructed.sizes[..ordinal].iter().all(|size| *size == line_bytes)
        && constructed.live_bytes == 0 && dropped.live_bytes == 0
        && constructed.deallocations == ordinal - 1 && dropped.deallocations == ordinal - 1
        && constructed.reallocations == 0;
    println!("DELAY_HISTORY_FAILURE_CHILD_RECORD {}", json!({
        "execution_head": execution_head(), "failed_allocation_ordinal": ordinal,
        "expected_line_bytes": line_bytes, "valid": valid, "identity": identity,
        "matches": matches, "constructed": observation(constructed), "dropped": observation(dropped),
    }));
    assert!(matches, "failed reservation must return an invalid instance without retaining history");
}

#[test]
fn either_real_history_allocation_failure_returns_a_bounded_invalid_instance() {
    let executable = std::env::current_exe().unwrap();
    let mut failures = Vec::new();
    for ordinal in [1, 2] {
        #[cfg(unix)]
        let mut command = {
            let mut command = Command::new("sh");
            // Limit ordinary abort dumps; a piped system handler can still retain diagnostics.
            command.args(["-c", "ulimit -c 0 || exit 125; exec \"$@\"", "delay-allocation-child"]);
            command.arg(&executable);
            command
        };
        #[cfg(not(unix))]
        let mut command = Command::new(&executable);
        let output = command
            .args(["--exact", "allocation_failure_child", "--nocapture", "--test-threads=1"])
            .env("VIBEZ_DELAY_FAIL_ALLOCATION", ordinal.to_string())
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let child_record = stdout.lines().find_map(|line| {
            line.split_once("DELAY_HISTORY_FAILURE_CHILD_RECORD ")
                .and_then(|(_, record)| serde_json::from_str::<serde_json::Value>(record).ok())
        });
        let matches = output.status.success()
            && child_record.as_ref().is_some_and(|record| {
                record["matches"] == true
                    && record["failed_allocation_ordinal"] == ordinal
                    && record["execution_head"] == execution_head()
            });
        println!("DELAY_HISTORY_FAILURE_PARENT_RECORD {}", json!({
            "execution_head": execution_head(), "failed_allocation_ordinal": ordinal,
            "status": output.status.to_string(), "exit_code": output.status.code(),
            "success": output.status.success(), "stdout": stdout, "stderr": stderr,
            "child_record": child_record, "matches": matches,
        }));
        if !matches {
            failures.push(format!("allocation {ordinal}: child did not return a bounded invalid instance ({})", output.status));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
