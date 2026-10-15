//! Resolves retained Left Free/Sync timing from the prepared rate and current finite project tempo.

use vibez_core::constants::{DEFAULT_BPM, MAX_BPM, MIN_BPM};

use super::param;

pub(super) struct DelayTime {
    bpm: f64,
}

impl DelayTime {
    pub(super) fn new() -> Self {
        Self { bpm: DEFAULT_BPM }
    }

    pub(super) fn set_bpm(&mut self, bpm: f64) {
        if bpm.is_finite() {
            self.bpm = bpm.clamp(MIN_BPM, MAX_BPM);
        }
    }

    pub(super) fn distance(&self, targets: &[f32; param::COUNT], sample_rate: f32) -> f64 {
        if targets[param::SYNC] >= 0.5 {
            let denominator = [64.0, 32.0, 16.0, 8.0, 4.0, 2.0, 1.0]
                [targets[param::LEFT_DIVISION] as usize];
            let modifier = [1.0, 1.5, 2.0 / 3.0][targets[param::LEFT_MODIFIER] as usize];
            (60.0 / self.bpm) * (4.0 / denominator) * modifier * f64::from(sample_rate)
        } else {
            f64::from(targets[param::TIME_MS]) * f64::from(sample_rate) / 1000.0
        }
    }
}
