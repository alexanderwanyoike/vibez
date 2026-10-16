//! What a native Filter card shows besides its parameters: whether Advanced
//! is open, which page a medium card shows, and the measured output level.

use std::time::{Duration, Instant};

use vibez_core::id::EffectId;
use vibez_engine::metering::MeterValues;

pub const MODE_NAMES: [&str; 4] = ["LP", "HP", "BP", "Notch"];
pub const SOURCE_NAMES: [&str; 3] = ["Off", "LFO", "Envelope"];
/// Index-aligned with `vibez_dsp::filter::SYNC_BEATS`.
pub const SYNC_NAMES: [&str; 8] = [
    "Free", "1 bar", "1/2", "1/4", "1/8", "1/16", "1/4 T", "1/8 D",
];

pub fn is_native_filter(effect: &crate::state::UiEffect) -> bool {
    effect.effect_type == vibez_core::effect::EffectType::Filter
        && effect.plugin_ref.is_none()
        && effect.plugin_name.is_none()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Page {
    #[default]
    Filter,
    Modulation,
}

#[derive(Debug, Clone)]
pub enum FilterMsg {
    ToggleAdvanced(EffectId),
    ShowPage(EffectId, Page),
}

impl FilterMsg {
    pub fn effect_id(&self) -> EffectId {
        match self {
            Self::ToggleAdvanced(id) | Self::ShowPage(id, _) => *id,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct FilterPanelState {
    pub advanced: bool,
    pub page: Page,
    pub output: MeterValues,
    pub last_meter_at: Option<Instant>,
}

impl FilterPanelState {
    pub fn update(&mut self, message: FilterMsg) {
        match message {
            FilterMsg::ToggleAdvanced(_) => self.advanced = !self.advanced,
            FilterMsg::ShowPage(_, page) => self.page = page,
        }
    }
}

impl super::device_levels::MeteredPanel for FilterPanelState {
    fn accept_levels(&mut self, _input: MeterValues, output: MeterValues, now: Instant) {
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
        self.output = MeterValues::SILENT;
        self.last_meter_at = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sync_names_cover_every_division_the_dsp_offers() {
        assert_eq!(SYNC_NAMES.len(), vibez_dsp::filter::SYNC_BEATS.len());
    }

    #[test]
    fn a_panel_switches_pages_and_advanced() {
        let id = EffectId::new();
        let mut panel = FilterPanelState::default();
        panel.update(FilterMsg::ShowPage(id, Page::Modulation));
        panel.update(FilterMsg::ToggleAdvanced(id));
        assert_eq!(panel.page, Page::Modulation);
        assert!(panel.advanced);
    }
}
