//! Opt-in timing log of clip lane view builds and draws
//! (`VIBEZ_LANE_TIMING=1`), so drawing cost can be measured on a real
//! machine rather than guessed from the code.

use std::cell::RefCell;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy)]
pub(super) enum Phase {
    Build,
    Draw,
}

#[derive(Default)]
struct Totals {
    calls: u32,
    total: Duration,
    max: Duration,
}

impl Totals {
    fn add(&mut self, elapsed: Duration) {
        self.calls += 1;
        self.total += elapsed;
        self.max = self.max.max(elapsed);
    }

    fn summary(&self, window: Duration) -> String {
        let micros = |duration: Duration| duration.as_secs_f64() * 1e6;
        let mean = if self.calls == 0 {
            0.0
        } else {
            micros(self.total) / f64::from(self.calls)
        };
        format!(
            "{} calls, mean {mean:.0} us, max {:.0} us, {:.1} ms/s",
            self.calls,
            micros(self.max),
            micros(self.total) / 1000.0 / window.as_secs_f64()
        )
    }
}

struct Window {
    started: Instant,
    build: Totals,
    draw: Totals,
}

thread_local! {
    static WINDOW: RefCell<Option<Window>> = const { RefCell::new(None) };
}

const REPORT_EVERY: Duration = Duration::from_secs(1);

fn enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var_os("VIBEZ_LANE_TIMING").is_some())
}

/// When timing is on, the instant a measured phase began.
pub(super) fn start() -> Option<Instant> {
    enabled().then(Instant::now)
}

pub(super) fn finish(started: Option<Instant>, phase: Phase) {
    let Some(started) = started else {
        return;
    };
    let elapsed = started.elapsed();
    WINDOW.with(|window| {
        let mut window = window.borrow_mut();
        let current = window.get_or_insert_with(|| Window {
            started,
            build: Totals::default(),
            draw: Totals::default(),
        });
        match phase {
            Phase::Build => current.build.add(elapsed),
            Phase::Draw => current.draw.add(elapsed),
        }
        let length = current.started.elapsed();
        if length >= REPORT_EVERY {
            eprintln!(
                "vibez: lane timing: build {}; draw {}",
                current.build.summary(length),
                current.draw.summary(length)
            );
            *window = None;
        }
    });
}
