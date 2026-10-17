//! Gain control values and transient metering state, independent of rendering.

use std::time::{Duration, Instant};

use vibez_core::id::{EffectId, TrackId};
use vibez_engine::metering::MeterValues;

pub const MIN_GAIN_DB: f32 = -60.0;
pub const MAX_GAIN_DB: f32 = 6.020_6;
const _: () = assert!(
    vibez_dsp::gain::MAX_GAIN == 2.0,
    "MAX_GAIN_DB is 20 log10(MAX_GAIN)"
);

#[derive(Debug, Clone)]
pub enum GainMsg {
    ToggleAdvanced(EffectId),
    ClearClip(EffectId),
}

pub fn is_native_gain(effect: &crate::state::UiEffect) -> bool {
    effect.effect_type == vibez_core::effect::EffectType::Gain
        && effect.plugin_ref.is_none()
        && effect.plugin_name.is_none()
}

impl GainMsg {
    pub fn destination(&self, channel: Option<TrackId>) -> Option<(TrackId, EffectId)> {
        match self {
            Self::ToggleAdvanced(effect) | Self::ClearClip(effect) => {
                channel.map(|track| (track, *effect))
            }
        }
    }
}

/// How far back the output history reaches.
pub const HISTORY: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Copy)]
pub struct GainHistoryPoint {
    pub peak: f32,
    pub rms: f32,
}

#[derive(Debug, Clone, Default)]
pub struct GainPanelState {
    pub advanced: bool,
    pub input: MeterValues,
    pub output: MeterValues,
    pub clip_latched: bool,
    pub history: super::device_levels::ReadingHistory<GainHistoryPoint>,
    pub last_meter_at: Option<Instant>,
}

impl GainPanelState {
    pub fn update(&mut self, message: GainMsg) {
        match message {
            GainMsg::ToggleAdvanced(_) => self.advanced = !self.advanced,
            GainMsg::ClearClip(_) => self.clear_clip(),
        }
    }

    pub fn accept_meter(&mut self, input: MeterValues, output: MeterValues, now: Instant) {
        self.input = input;
        self.output = output;
        self.clip_latched |= output.peak_l >= 1.0 || output.peak_r >= 1.0;
        self.last_meter_at = Some(now);
        self.history.push(
            now,
            GainHistoryPoint {
                peak: output.peak_l.max(output.peak_r),
                rms: output.rms_l.max(output.rms_r),
            },
            HISTORY,
        );
    }

    pub fn clear_clip(&mut self) {
        self.clip_latched = false;
    }

    /// Drops readings that describe the replaced values, keeping what the
    /// user has opened.
    pub fn restart_metering(&mut self) {
        self.clear_readings();
        self.clip_latched = false;
    }

    pub fn reset_stale(&mut self, now: Instant) {
        if self
            .last_meter_at
            .is_some_and(|last| now.saturating_duration_since(last) >= Duration::from_millis(250))
        {
            self.clear_readings();
        }
    }

    fn clear_readings(&mut self) {
        self.input = MeterValues::SILENT;
        self.output = MeterValues::SILENT;
        self.history.clear();
        self.last_meter_at = None;
    }
}

/// Knob travel for a linear gain: unity sits at the centre, with the cut range
/// in the left half and the boost range in the right.
pub fn knob_position(amplitude: f32) -> f32 {
    let db = vibez_core::level::amplitude_to_db(amplitude).clamp(MIN_GAIN_DB, MAX_GAIN_DB);
    if db <= 0.0 {
        (db - MIN_GAIN_DB) / (-2.0 * MIN_GAIN_DB)
    } else {
        0.5 + db / (2.0 * MAX_GAIN_DB)
    }
}

/// Inverse of [`knob_position`]; the bottom of the travel is silence.
pub fn knob_amplitude(position: f32) -> f32 {
    let position = position.clamp(0.0, 1.0);
    if position == 0.0 {
        return 0.0;
    }
    let db = if position <= 0.5 {
        MIN_GAIN_DB * (1.0 - 2.0 * position)
    } else {
        MAX_GAIN_DB * (2.0 * position - 1.0)
    };
    vibez_core::level::db_to_amplitude(db)
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let (at, point) = panel.history.iter().next().unwrap();
        assert_eq!(*at, now);
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
