//! What a native card with an Advanced switch remembers besides its
//! parameters: whether Advanced is open, which of its groups and plots a
//! narrow card shows, the levels the engine measured either side of the
//! device, and for a dynamics device what it took off and when.

use std::time::{Duration, Instant};

use vibez_core::effect::DynamicsMeter;
use vibez_core::id::EffectId;
use vibez_engine::metering::MeterValues;

use super::device_levels::ReadingHistory;

/// How far back a dynamics device's readings reach.
pub const DYNAMICS_HISTORY: Duration = Duration::from_secs(12);

/// One reading from a dynamics device, in decibels: the loudest input since
/// the last, and the deepest reduction.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DynamicsReading {
    pub input_db: f32,
    pub reduction_db: f32,
}

#[derive(Debug, Clone)]
pub enum CardMsg {
    ToggleAdvanced(EffectId),
    /// Shows one of Advanced's groups on a card too narrow for all of them.
    ShowAdvancedPage(EffectId, usize),
    /// Shows one of the plots on a card too narrow for all of them.
    ShowPlot(EffectId, usize),
}

impl CardMsg {
    pub fn effect_id(&self) -> EffectId {
        match self {
            Self::ToggleAdvanced(id) | Self::ShowAdvancedPage(id, _) | Self::ShowPlot(id, _) => *id,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct CardPanelState {
    pub advanced: bool,
    pub advanced_page: usize,
    pub plot: usize,
    pub input: MeterValues,
    pub output: MeterValues,
    /// The deepest reduction in the latest reading, of either channel.
    pub reduction_db: f32,
    pub dynamics: ReadingHistory<DynamicsReading>,
    pub last_meter_at: Option<Instant>,
}

impl CardPanelState {
    pub fn update(&mut self, message: CardMsg) {
        match message {
            CardMsg::ToggleAdvanced(_) => self.advanced = !self.advanced,
            CardMsg::ShowAdvancedPage(_, page) => self.advanced_page = page,
            CardMsg::ShowPlot(_, plot) => self.plot = plot,
        }
    }

    pub fn accept_dynamics(&mut self, meter: DynamicsMeter, now: Instant) {
        let reduction_db = meter.peak_reduction_db[0]
            .max(meter.peak_reduction_db[1])
            .max(0.0);
        let input = meter.detector_peak[0].max(meter.detector_peak[1]);
        self.reduction_db = reduction_db;
        self.last_meter_at = Some(now);
        self.dynamics.push(
            now,
            DynamicsReading {
                input_db: vibez_core::level::amplitude_to_db(input),
                reduction_db,
            },
            DYNAMICS_HISTORY,
        );
    }
}

impl super::device_levels::MeteredPanel for CardPanelState {
    fn accept_levels(&mut self, input: MeterValues, output: MeterValues, now: Instant) {
        self.input = input;
        self.output = output;
        self.last_meter_at = Some(now);
    }

    fn reset_stale(&mut self, now: Instant) {
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
        self.reduction_db = 0.0;
        self.dynamics.clear();
        self.last_meter_at = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domains::device_levels::MeteredPanel;

    #[test]
    fn stale_levels_clear_and_advanced_stays_open() {
        let now = Instant::now();
        let mut panel = CardPanelState::default();
        panel.update(CardMsg::ToggleAdvanced(EffectId::new()));
        let level = MeterValues {
            peak_l: 0.5,
            peak_r: 0.5,
            rms_l: 0.3,
            rms_r: 0.3,
        };
        panel.accept_levels(level, level, now);
        panel.reset_stale(now + Duration::from_millis(250));
        assert_eq!(panel.input, MeterValues::SILENT);
        assert!(panel.advanced);
    }

    #[test]
    fn dynamics_keep_the_deeper_channel_and_clear_with_the_levels() {
        let now = Instant::now();
        let mut panel = CardPanelState::default();
        panel.accept_dynamics(
            DynamicsMeter {
                peak_reduction_db: [2.0, 3.5],
                detector_peak: [0.5, 0.25],
                ..Default::default()
            },
            now,
        );
        assert_eq!(panel.reduction_db, 3.5);
        let (_, reading) = panel.dynamics.iter().next().copied().unwrap();
        assert!((reading.input_db - -6.02).abs() < 0.01, "{reading:?}");
        panel.reset_stale(now + Duration::from_millis(250));
        assert!(panel.dynamics.is_empty() && panel.reduction_db == 0.0);
    }
}
