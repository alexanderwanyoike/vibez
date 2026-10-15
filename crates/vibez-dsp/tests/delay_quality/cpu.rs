//! Records explicitly requested optimized Delay CPU observations and machine-local budgets.

use super::*;
use std::time::Instant;

#[derive(Clone, Copy)]
enum Profile {
    Neutral,
    CutsDuck,
    Moving,
    NormalTailReference,
    Subnormal,
}

impl Profile {
    fn name(self) -> &'static str {
        match self {
            Self::Neutral => "neutral_settled",
            Self::CutsDuck => "cuts_duck_settled",
            Self::Moving => "moving_controls",
            Self::NormalTailReference => "normal_tail_reference",
            Self::Subnormal => "subnormal_tail",
        }
    }
    fn tail(self) -> Option<f32> {
        match self {
            Self::NormalTailReference => Some(1e-4),
            Self::Subnormal => Some(1e-35),
            _ => None,
        }
    }
    fn input(self, frame: usize, channel: usize, rate: u32) -> f32 {
        if let Some(amplitude) = self.tail() {
            if frame == 0 {
                if channel == 0 {
                    amplitude
                } else {
                    -amplitude
                }
            } else {
                0.0
            }
        } else {
            Signal::Sine.sample(frame, channel, rate)
        }
    }
}

struct Pass {
    raw_ns: Vec<u64>,
    real_time_factor: f64,
    window_ns_per_frame: f64,
    subnormal_samples: usize,
    valid: bool,
}

fn pass(profile: Profile, rate: u32, block_size: usize, pass_index: usize) -> Pass {
    let active = matches!(profile, Profile::CutsDuck | Profile::Moving);
    let time = if profile.tail().is_some() { 1.0 } else { 17.0 };
    let mut effect = prepare(
        rate,
        [time; 2],
        0.0,
        active.then_some([120.0, 8000.0]),
        if active { 0.5 } else { 0.0 },
    );
    let mut scratch = [0.0; 1024];
    let mut warm_frame = 0;
    let warm_frames = span(rate, 0.1);
    while warm_frame < warm_frames {
        let count = block_size.min(warm_frames - warm_frame);
        for (i, sample) in scratch[..2 * count].iter_mut().enumerate() {
            *sample = profile.input(warm_frame + i / 2, i % 2, rate);
        }
        effect.process(&mut scratch[..2 * count], 2);
        warm_frame += count;
    }
    // Reset makes the tiny impulse and its declared observation window independent of warmup.
    effect.reset();
    let frames = 2 * rate as usize;
    let mut raw_ns = Vec::with_capacity(frames.div_ceil(block_size));
    let mut heap = [0; 3];
    let (
        mut frame,
        mut block,
        mut window_ns,
        mut window_frames,
        mut subnormal_samples,
        mut nonfinite,
        mut rejected,
    ) = (0, 0, 0_u64, 0, 0, 0, 0);
    while frame < frames {
        let count = block_size.min(frames - frame);
        for (i, sample) in scratch[..2 * count].iter_mut().enumerate() {
            *sample = profile.input(frame + i / 2, i % 2, rate);
        }
        let targets = [
            (param::TIME_MS, [3.0, 17.5, 41.0, 5.25][block % 4]),
            (param::RIGHT_TIME_MS, [5.0, 29.25, 43.0, 7.0][block % 4]),
            (
                param::FEEDBACK_HP_HZ,
                if block % 2 == 0 { 20.0 } else { 1000.0 },
            ),
            (
                param::FEEDBACK_LP_HZ,
                if block % 2 == 0 { 20000.0 } else { 1000.0 },
            ),
            (param::ROUTING, (block % 2) as f32),
            (param::DUCK, (block % 2) as f32),
        ];
        let (elapsed, refused) = measured(
            || {
                let begin = Instant::now();
                let mut refused = 0;
                if matches!(profile, Profile::Moving) {
                    for (index, value) in targets {
                        refused += usize::from(!effect.set_param(index, value));
                    }
                }
                effect.process(&mut scratch[..2 * count], 2);
                (begin.elapsed(), refused)
            },
            &mut heap,
        );
        let nanos = u64::try_from(elapsed.as_nanos()).expect("block duration fits nanoseconds");
        raw_ns.push(nanos);
        rejected += refused;
        nonfinite += scratch[..2 * count]
            .iter()
            .filter(|sample| !sample.is_finite())
            .count();
        if frame >= span(rate, 0.2) && frame + count <= span(rate, 0.5) {
            window_ns += nanos;
            window_frames += count;
            subnormal_samples += scratch[..2 * count]
                .iter()
                .filter(|sample| sample.is_subnormal())
                .count();
        }
        frame += count;
        block += 1;
    }
    let processing_ns = raw_ns.iter().sum::<u64>();
    let real_time_factor = processing_ns as f64 / 2e9;
    let window_ns_per_frame = window_ns as f64 / window_frames as f64;
    let valid = heap == [0; 3] && nonfinite == 0 && rejected == 0;
    println!(
        "DELAY_CPU_PASS {}",
        json!({ "profile": profile.name(), "rate": rate, "block_frames": block_size,
        "pass": pass_index, "audio_frames": frames, "warmup_frames": warm_frames, "reset_after_warmup": true,
        "last_block_frames": frames - (raw_ns.len() - 1) * block_size,
        "raw_block_nanoseconds": raw_ns, "processing_ns": processing_ns, "real_time_factor": real_time_factor,
        "ns_per_frame": processing_ns as f64 / frames as f64, "tail_window_complete_frames": window_frames,
        "tail_window_ns_per_frame": window_ns_per_frame, "tail_window_nonzero_subnormal_samples": subnormal_samples,
        "heap_alloc_realloc_dealloc": heap, "nonfinite_outputs": nonfinite, "rejected_setters": rejected, "valid": valid })
    );
    Pass {
        raw_ns,
        real_time_factor,
        window_ns_per_frame,
        subnormal_samples,
        valid,
    }
}

fn median(mut values: [f64; 5]) -> f64 {
    values.sort_by(f64::total_cmp);
    values[2]
}

#[test]
#[ignore = "Explicit optimized machine-local CPU observation; requires source/binary/machine labels"]
#[allow(
    clippy::assertions_on_constants,
    reason = "This ignored observation must compile in debug and refuse execution outside release."
)]
fn optimized_delay_cpu_observation() {
    assert!(
        !cfg!(debug_assertions),
        "Run only the optimized release test binary"
    );
    let label =
        |name| std::env::var(name).unwrap_or_else(|_| panic!("Required observation label: {name}"));
    println!(
        "DELAY_CPU_MACHINE {}",
        json!({ "source_head": label("VIBEZ_VERIFICATION_HEAD"),
        "binary_sha256": label("VIBEZ_VERIFICATION_BINARY_SHA256"), "cpu": label("VIBEZ_VERIFICATION_CPU"),
        "load": label("VIBEZ_VERIFICATION_LOAD"), "build_flags": label("VIBEZ_VERIFICATION_BUILD_FLAGS"),
        "binary_path": std::env::current_exe().expect("test binary path"), "profile": "release",
        "os": std::env::consts::OS, "architecture": std::env::consts::ARCH, "ftz_daz": "unknown",
        "measurement": "Actual setters plus process only; input generation, reset, construction, reporting and allocator-counter access excluded",
        "limitation": "Wall-clock observations on this machine, not a portable realtime guarantee; raw scheduling outliers retained" })
    );
    let mut failures = 0;
    for rate in [48000, 192000] {
        for block_size in [128, 512] {
            let mut reference_window = None;
            for profile in [
                Profile::Neutral,
                Profile::CutsDuck,
                Profile::Moving,
                Profile::NormalTailReference,
                Profile::Subnormal,
            ] {
                let passes: [Pass; 5] =
                    std::array::from_fn(|index| pass(profile, rate, block_size, index));
                let median_factor = median(std::array::from_fn(|i| passes[i].real_time_factor));
                let window_cost = median(std::array::from_fn(|i| passes[i].window_ns_per_frame));
                let mut deadline_ratios: Vec<_> = passes
                    .iter()
                    .flat_map(|pass| {
                        pass.raw_ns.iter().enumerate().map(|(index, nanos)| {
                            let frames = block_size.min(2 * rate as usize - index * block_size);
                            *nanos as f64 * f64::from(rate) / (frames as f64 * 1e9)
                        })
                    })
                    .collect();
                deadline_ratios.sort_by(f64::total_cmp);
                let p95 = deadline_ratios[(deadline_ratios.len() * 95).div_ceil(100) - 1];
                let maximum = deadline_ratios.last().copied().unwrap();
                let limit = if matches!(profile, Profile::Moving) {
                    0.25
                } else {
                    0.10
                };
                let subnormals = passes
                    .iter()
                    .map(|pass| pass.subnormal_samples)
                    .sum::<usize>();
                if matches!(profile, Profile::NormalTailReference) {
                    reference_window = Some(window_cost);
                }
                let ratio = if matches!(profile, Profile::Subnormal) && subnormals > 0 {
                    Some(
                        window_cost
                            / reference_window.expect("paired normal tail precedes tiny tail"),
                    )
                } else {
                    None
                };
                let passed = passes.iter().all(|pass| pass.valid)
                    && median_factor <= limit
                    && p95 <= 0.5
                    && ratio.is_none_or(|ratio| ratio <= 2.0);
                failures += usize::from(!passed);
                println!(
                    "DELAY_CPU_CASE {}",
                    json!({ "profile": profile.name(), "rate": rate,
                    "block_frames": block_size, "passes": 5, "seconds_per_pass": 2,
                    "median_real_time_factor": median_factor, "real_time_factor_limit": limit,
                    "p95_block_deadline_ratio": p95, "p95_limit": 0.5, "maximum_block_deadline_ratio": maximum,
                    "median_tail_window_ns_per_frame": window_cost, "subnormal_samples_observed": subnormals,
                    "subnormal_normal_cost_ratio": ratio, "conditional_subnormal_ratio_limit": 2.0,
                    "ratio_applicability": if matches!(profile, Profile::Subnormal) && subnormals == 0 {
                        "Not established: no nonzero subnormal output survived" } else { "See profile and observed sample count" },
                    "passed": passed })
                );
            }
        }
    }
    assert_eq!(
        failures, 0,
        "Machine-local CPU budgets failed; retain raw observations and investigate a clean rerun"
    );
}
