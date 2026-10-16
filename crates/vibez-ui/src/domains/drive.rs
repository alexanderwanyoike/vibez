//! What a native Drive card shows besides its parameters: whether Advanced
//! is open, and the levels the engine measured either side of it.

use std::time::{Duration, Instant};

use vibez_core::id::EffectId;
use vibez_engine::metering::MeterValues;

/// Index-aligned with `vibez_dsp::drive::character`.
pub const CHARACTER_NAMES: [&str; 4] = ["Classic", "Soft", "Warm", "Hard"];
/// Index-aligned with `vibez_dsp::drive::quality`.
pub const QUALITY_NAMES: [&str; 3] = ["1x", "2x", "4x"];

pub fn is_native_drive(effect: &crate::state::UiEffect) -> bool {
    effect.effect_type == vibez_core::effect::EffectType::Drive
        && effect.plugin_ref.is_none()
        && effect.plugin_name.is_none()
}

#[derive(Debug, Clone)]
pub enum DriveMsg {
    ToggleAdvanced(EffectId),
}

impl DriveMsg {
    pub fn effect_id(&self) -> EffectId {
        match self {
            Self::ToggleAdvanced(id) => *id,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct DrivePanelState {
    pub advanced: bool,
    pub input: MeterValues,
    pub output: MeterValues,
    pub last_meter_at: Option<Instant>,
}

impl DrivePanelState {
    pub fn update(&mut self, message: DriveMsg) {
        match message {
            DriveMsg::ToggleAdvanced(_) => self.advanced = !self.advanced,
        }
    }
}

impl super::device_levels::MeteredPanel for DrivePanelState {
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
    fn names_cover_every_choice_the_dsp_offers() {
        assert_eq!(CHARACTER_NAMES.len(), vibez_dsp::drive::character::COUNT);
        assert_eq!(QUALITY_NAMES.len(), vibez_dsp::drive::quality::COUNT);
    }

    #[test]
    fn stale_levels_clear_and_advanced_stays_open() {
        let now = Instant::now();
        let mut panel = DrivePanelState::default();
        panel.update(DriveMsg::ToggleAdvanced(EffectId::new()));
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
