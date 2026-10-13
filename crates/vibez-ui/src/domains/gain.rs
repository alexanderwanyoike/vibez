//! Gain control values and transient metering state, independent of rendering.

use std::collections::VecDeque;
use std::time::Instant;

use vibez_core::id::{EffectId, TrackId};
use vibez_engine::metering::MeterValues;

pub const MIN_GAIN_DB: f32 = -60.0;
pub const MAX_GAIN_DB: f32 = 6.020_6;

#[derive(Debug, Clone)]
pub enum GainMsg {
    SetInput(TrackId, EffectId, usize, String),
    SubmitInput(TrackId, EffectId, usize),
    ToggleAdvanced(EffectId),
    ClearClip(EffectId),
}

#[derive(Debug, Clone, Copy)]
pub struct GainHistoryPoint {
    pub at: Instant,
    pub peak: f32,
    pub rms: f32,
}

#[derive(Debug, Clone, Default)]
pub struct GainPanelState {
    pub advanced: bool,
    pub input_text: [Option<String>; 3],
    pub input_error: [bool; 3],
    pub input: MeterValues,
    pub output: MeterValues,
    pub clip_latched: bool,
    pub history: VecDeque<GainHistoryPoint>,
    pub last_meter_at: Option<Instant>,
}

impl GainPanelState {
    pub fn accept_meter(&mut self, _input: MeterValues, _output: MeterValues, _now: Instant) {}

    pub fn clear_clip(&mut self) {}

    pub fn reset_stale(&mut self, _now: Instant) {}
}

pub fn gain_to_db(gain: f32) -> f32 {
    if gain > 0.0 {
        20.0 * gain.log10()
    } else {
        f32::NEG_INFINITY
    }
}

pub fn db_to_gain(db: f32) -> f32 {
    10.0_f32.powf(db / 20.0)
}

pub fn parse_input(index: usize, text: &str) -> Option<f32> {
    let text = text.trim();
    if index == 0 && (text.eq_ignore_ascii_case("-inf") || text == "-∞") {
        return Some(0.0);
    }
    let db = text.parse::<f32>().ok()?;
    if !db.is_finite() {
        return None;
    }
    match index {
        0 => (MIN_GAIN_DB..=MAX_GAIN_DB)
            .contains(&db)
            .then(|| db_to_gain(db)),
        1 | 2 => (-24.0..=24.0).contains(&db).then_some(db),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gain_decibel_entry_preserves_native_amplitude_and_rejects_invalid_values() {
        assert_eq!(parse_input(0, "0"), Some(1.0));
        assert!((parse_input(0, " -6.0 ").unwrap() - 0.501_187_2).abs() < 1e-6);
        assert_eq!(parse_input(0, "-inf"), Some(0.0));
        assert_eq!(parse_input(0, "-∞"), Some(0.0));
        assert!((parse_input(0, "-60").unwrap() - 0.001).abs() < 1e-6);
        assert!((parse_input(0, "6.0205999").unwrap() - 2.0).abs() < 1e-6);

        for invalid in ["", "NaN", "inf", "+inf", "100", "-61", "oops"] {
            assert_eq!(parse_input(0, invalid), None, "accepted {invalid}");
        }
    }

    #[test]
    fn trim_entry_preserves_decibels_and_rejects_values_outside_its_range() {
        for index in [1, 2] {
            for db in [-24.0, -6.0, 0.0, 6.0, 24.0] {
                assert_eq!(parse_input(index, &db.to_string()), Some(db));
            }
            for invalid in ["-inf", "-∞", "NaN", "inf", "-24.1", "24.1", "oops"] {
                assert_eq!(parse_input(index, invalid), None, "accepted {invalid}");
            }
        }
        assert_eq!(parse_input(3, "0"), None);
    }

    #[test]
    fn gain_meter_history_is_bounded_and_stale_readings_preserve_the_clip_latch() {
        use std::time::Duration;

        let now = Instant::now();
        let mut panel = GainPanelState::default();
        let input = MeterValues {
            peak_l: 0.5,
            peak_r: 0.25,
            rms_l: 0.3,
            rms_r: 0.2,
        };
        let output = MeterValues {
            peak_l: 1.1,
            peak_r: 0.7,
            rms_l: 0.8,
            rms_r: 0.5,
        };
        panel.accept_meter(input, output, now);
        assert_eq!(panel.input, input);
        assert_eq!(panel.output, output);
        assert!(panel.clip_latched);
        assert_eq!(panel.last_meter_at, Some(now));
        assert_eq!(panel.history.len(), 1);
        let point = panel.history.front().unwrap();
        assert_eq!(point.at, now);
        assert_eq!(point.peak, output.peak_l);
        assert_eq!(point.rms, output.rms_l);

        for millisecond in 1..=601 {
            panel.accept_meter(
                MeterValues::SILENT,
                MeterValues::SILENT,
                now + Duration::from_millis(millisecond),
            );
        }
        assert_eq!(panel.history.len(), 600);
        assert!(panel.clip_latched);

        let recent = now + Duration::from_secs(11);
        panel.accept_meter(input, output, recent);
        assert_eq!(panel.history.len(), 1);
        panel.reset_stale(recent + Duration::from_millis(249));
        assert_eq!(panel.output, output);
        panel.reset_stale(recent + Duration::from_millis(250));
        assert_eq!(panel.input, MeterValues::SILENT);
        assert_eq!(panel.output, MeterValues::SILENT);
        assert!(panel.history.is_empty());
        assert_eq!(panel.last_meter_at, None);
        assert!(panel.clip_latched);
        panel.clear_clip();
        assert!(!panel.clip_latched);
    }
}
