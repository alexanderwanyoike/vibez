//! What a native Compressor card shows besides its parameters: whether
//! Advanced is open, which plot a compact card shows, and the levels and
//! gain reduction the engine measured.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use vibez_core::effect::DynamicsMeter;
use vibez_core::id::EffectId;
use vibez_engine::metering::MeterValues;

/// How far back the reduction history reaches.
pub const HISTORY: Duration = Duration::from_secs(5);
const HISTORY_POINTS: usize = 600;

pub fn is_native_compressor(effect: &crate::state::UiEffect) -> bool {
    effect.effect_type == vibez_core::effect::EffectType::Compressor
        && effect.plugin_ref.is_none()
        && effect.plugin_name.is_none()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Page {
    #[default]
    Transfer,
    History,
}

#[derive(Debug, Clone)]
pub enum CompressorMsg {
    ToggleAdvanced(EffectId),
    ShowPage(EffectId, Page),
}

impl CompressorMsg {
    pub fn effect_id(&self) -> EffectId {
        match self {
            Self::ToggleAdvanced(id) | Self::ShowPage(id, _) => *id,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReductionPoint {
    pub at: Instant,
    /// Decibels of reduction, zero when the compressor is idle.
    pub db: f32,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct CompressorPanelState {
    pub advanced: bool,
    pub page: Page,
    pub input: MeterValues,
    pub output: MeterValues,
    /// Decibels of reduction now, the larger of the two channels.
    pub reduction_db: f32,
    pub history: VecDeque<ReductionPoint>,
    pub last_meter_at: Option<Instant>,
}

impl CompressorPanelState {
    pub fn update(&mut self, message: CompressorMsg) {
        match message {
            CompressorMsg::ToggleAdvanced(_) => self.advanced = !self.advanced,
            CompressorMsg::ShowPage(_, page) => self.page = page,
        }
    }

    pub fn accept_levels(&mut self, input: MeterValues, output: MeterValues, now: Instant) {
        self.input = input;
        self.output = output;
        self.last_meter_at = Some(now);
    }

    /// Records the deepest reduction since the last reading, so a short
    /// transient still shows in the history.
    pub fn accept_dynamics(&mut self, meter: DynamicsMeter, now: Instant) {
        let db = meter.peak_reduction_db[0]
            .max(meter.peak_reduction_db[1])
            .max(0.0);
        self.reduction_db = db;
        self.last_meter_at = Some(now);
        while self.history.front().is_some_and(|point| {
            self.history.len() >= HISTORY_POINTS
                || now.saturating_duration_since(point.at) > HISTORY
        }) {
            self.history.pop_front();
        }
        self.history.push_back(ReductionPoint { at: now, db });
    }

    /// Readings stop when audio stops reaching the compressor; a held value
    /// would claim reduction that is no longer happening.
    pub fn reset_stale(&mut self, now: Instant) {
        if self
            .last_meter_at
            .is_some_and(|last| now.saturating_duration_since(last) >= Duration::from_millis(250))
        {
            self.clear_readings();
        }
    }

    pub fn clear_readings(&mut self) {
        self.input = MeterValues::SILENT;
        self.output = MeterValues::SILENT;
        self.reduction_db = 0.0;
        self.history.clear();
        self.last_meter_at = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reduction(left: f32, right: f32) -> DynamicsMeter {
        DynamicsMeter {
            peak_reduction_db: [left, right],
            ..Default::default()
        }
    }

    #[test]
    fn reduction_keeps_the_deeper_channel_and_a_bounded_history() {
        let now = Instant::now();
        let mut panel = CompressorPanelState::default();
        panel.accept_dynamics(reduction(2.0, 3.5), now);
        assert_eq!(panel.reduction_db, 3.5);
        for step in 1..=800 {
            panel.accept_dynamics(reduction(1.0, 0.0), now + Duration::from_millis(step));
        }
        assert_eq!(panel.history.len(), HISTORY_POINTS);
        panel.accept_dynamics(reduction(0.0, 0.0), now + Duration::from_secs(10));
        assert_eq!(
            panel.history.len(),
            1,
            "older points fall out of the window"
        );
    }

    #[test]
    fn stale_readings_clear_and_panel_choices_stay() {
        let now = Instant::now();
        let id = EffectId::new();
        let mut panel = CompressorPanelState::default();
        panel.update(CompressorMsg::ToggleAdvanced(id));
        panel.update(CompressorMsg::ShowPage(id, Page::History));
        panel.accept_dynamics(reduction(4.0, 4.0), now);
        panel.reset_stale(now + Duration::from_millis(249));
        assert_eq!(panel.reduction_db, 4.0);
        panel.reset_stale(now + Duration::from_millis(250));
        assert_eq!(panel.reduction_db, 0.0);
        assert!(panel.history.is_empty());
        assert!(panel.advanced && panel.page == Page::History);
    }
}
