//! Resolves retained Left/Right Free/Sync timing from the prepared rate and current finite project tempo.

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

    pub(super) fn distances(&self, targets: &[f32; param::COUNT], sample_rate: f32) -> [f64; 2] {
        let left = self.distance(
            targets,
            sample_rate,
            param::TIME_MS,
            param::LEFT_DIVISION,
            param::LEFT_MODIFIER,
        );
        let right = if targets[param::LINK] >= 0.5 {
            left
        } else {
            self.distance(
                targets,
                sample_rate,
                param::RIGHT_TIME_MS,
                param::RIGHT_DIVISION,
                param::RIGHT_MODIFIER,
            )
        };
        [left, right]
    }

    fn distance(
        &self,
        targets: &[f32; param::COUNT],
        sample_rate: f32,
        time: usize,
        division: usize,
        modifier: usize,
    ) -> f64 {
        if targets[param::SYNC] >= 0.5 {
            let denominator = [64.0, 32.0, 16.0, 8.0, 4.0, 2.0, 1.0]
                [targets[division] as usize];
            let modifier = [1.0, 1.5, 2.0 / 3.0][targets[modifier] as usize];
            (60.0 / self.bpm) * (4.0 / denominator) * modifier * f64::from(sample_rate)
        } else {
            f64::from(targets[time]) * f64::from(sample_rate) / 1000.0
        }
    }
}
