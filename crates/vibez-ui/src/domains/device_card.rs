//! What a native card with an Advanced switch remembers besides its
//! parameters: whether Advanced is open, which of its groups a narrow card
//! shows, and the levels the engine measured either side of the device.

use std::time::{Duration, Instant};

use vibez_core::id::EffectId;
use vibez_engine::metering::MeterValues;

#[derive(Debug, Clone)]
pub enum CardMsg {
    ToggleAdvanced(EffectId),
    /// Shows one of Advanced's groups on a card too narrow for all of them.
    ShowAdvancedPage(EffectId, usize),
}

impl CardMsg {
    pub fn effect_id(&self) -> EffectId {
        match self {
            Self::ToggleAdvanced(id) | Self::ShowAdvancedPage(id, _) => *id,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct CardPanelState {
    pub advanced: bool,
    pub advanced_page: usize,
    pub input: MeterValues,
    pub output: MeterValues,
    pub last_meter_at: Option<Instant>,
}

impl CardPanelState {
    pub fn update(&mut self, message: CardMsg) {
        match message {
            CardMsg::ToggleAdvanced(_) => self.advanced = !self.advanced,
            CardMsg::ShowAdvancedPage(_, page) => self.advanced_page = page,
        }
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
}
