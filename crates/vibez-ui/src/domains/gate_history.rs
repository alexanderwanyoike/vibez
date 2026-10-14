//! Retains only valid measured Gate windows and clears stale signal state.

use std::time::{Duration, Instant};
use vibez_core::effect::{DynamicsMeter, GateMeter};

pub const HISTORY_WINDOWS: usize = 200;

#[derive(Clone, Copy, Debug, Default)]
pub struct GateWindow {
    pub gate: GateMeter,
    pub reduction_db: f32,
}

pub struct GateHistory {
    windows: [GateWindow; HISTORY_WINDOWS],
    next: usize,
    count: usize,
    meter: Option<DynamicsMeter>,
    last_meter_at: Option<Instant>,
}

impl Default for GateHistory {
    fn default() -> Self {
        Self {
            windows: [GateWindow::default(); HISTORY_WINDOWS],
            next: 0,
            count: 0,
            meter: None,
            last_meter_at: None,
        }
    }
}

impl GateHistory {
    pub fn accept(&mut self, meter: DynamicsMeter, now: Instant) -> bool {
        let Some(gate) = meter.gate else {
            return false;
        };
        if meter
            .current_reduction_db
            .iter()
            .chain(&meter.peak_reduction_db)
            .chain(&meter.detector_peak)
            .any(|value| !value.is_finite() || *value < 0.0)
            || [
                gate.envelope_db,
                gate.peak_envelope_db,
                gate.open_threshold_db,
                gate.close_threshold_db,
                gate.hold_remaining_ms,
            ]
            .iter()
            .any(|value| !value.is_finite())
            || gate.hold_remaining_ms < 0.0
            || gate.phases_seen & !7 != 0
        {
            return false;
        }
        self.meter = Some(meter);
        self.last_meter_at = Some(now);
        if gate.phases_seen != 0 {
            self.windows[self.next] = GateWindow {
                gate,
                reduction_db: meter.peak_reduction_db[0].max(meter.peak_reduction_db[1]),
            };
            self.next = (self.next + 1) % HISTORY_WINDOWS;
            self.count = (self.count + 1).min(HISTORY_WINDOWS);
        }
        true
    }
    pub fn meter(&self) -> Option<DynamicsMeter> {
        self.meter
    }
    pub fn windows(&self) -> impl Iterator<Item = GateWindow> + '_ {
        (0..self.count).map(|index| {
            self.windows[(self.next + HISTORY_WINDOWS - self.count + index) % HISTORY_WINDOWS]
        })
    }
    pub fn reset(&mut self) {
        *self = Self::default();
    }
    pub fn reset_stale(&mut self, now: Instant) {
        if self
            .last_meter_at
            .is_some_and(|last| now.saturating_duration_since(last) >= Duration::from_millis(250))
        {
            self.reset();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};
    use vibez_core::effect::{DynamicsMeter, GateMeter, GatePhase};

    #[test]
    fn bounded_gate_history_preserves_measured_phase_and_rejects_invalid_or_stale_windows() {
        let now = Instant::now();
        let mut history = GateHistory::default();
        for index in 0..205 {
            let value = -100.0 + index as f32 * 0.2;
            let meter = DynamicsMeter {
                gate: Some(GateMeter {
                    phase: GatePhase::Hold,
                    envelope_db: value,
                    peak_envelope_db: value + 1.0,
                    open_threshold_db: -40.0,
                    close_threshold_db: -46.0,
                    hold_remaining_ms: 12.0,
                    phases_seen: 6,
                }),
                current_reduction_db: [2.0, 3.0],
                peak_reduction_db: [4.0, 5.0],
                detector_peak: [0.1, 0.1],
            };
            assert!(history.accept(meter, now));
        }
        let windows = history.windows().collect::<Vec<_>>();
        assert_eq!(windows.len(), 200);
        assert_eq!(windows[0].gate.envelope_db, -99.0);
        assert_eq!(windows[199].gate.phases_seen, 6);
        assert_eq!(windows[199].gate.phase, GatePhase::Hold);
        assert_eq!(windows[199].reduction_db, 5.0);
        let valid = history.meter().unwrap();
        let mut invalid = valid;
        invalid.gate.as_mut().unwrap().envelope_db = f32::NAN;
        assert!(!history.accept(invalid, now + Duration::from_millis(200)));
        assert_eq!(history.meter(), Some(valid));
        history.reset_stale(now + Duration::from_millis(249));
        assert_eq!(history.windows().count(), 200);
        history.reset_stale(now + Duration::from_millis(250));
        assert!(history.meter().is_none());
        assert_eq!(history.windows().count(), 0);
    }
}
