//! Retains only valid measured Gate windows and clears stale signal state.

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
