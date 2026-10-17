//! What a native EQ card shows besides its parameters: the band being
//! edited, which side of the EQ the analyser listens to, and whether the
//! analyser is frozen.

use std::time::{Duration, Instant};

use vibez_core::id::EffectId;
use vibez_dsp::eq::param;
use vibez_engine::metering::MeterValues;

pub const BAND_NAMES: [&str; 4] = ["LF", "LMF", "HMF", "HF"];

/// The two shelves switch between shelf and bell; the mids have a Q.
pub fn has_q(band: usize) -> bool {
    matches!(band, 1 | 2)
}

/// The audition parameter stores the soloed band plus one, zero for none.
pub fn auditioned_band(params: &[f32]) -> Option<usize> {
    let value = params.get(param::AUDITION).copied().unwrap_or(0.0).round();
    (1.0..=4.0).contains(&value).then(|| value as usize - 1)
}

pub fn is_native_eq(effect: &crate::state::UiEffect) -> bool {
    effect.effect_type == vibez_core::effect::EffectType::Eq
        && effect.plugin_ref.is_none()
        && effect.plugin_name.is_none()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SpectrumSide {
    /// What reaches the EQ.
    Pre,
    /// What the EQ hands on.
    #[default]
    Post,
}

#[derive(Debug, Clone)]
pub enum EqMsg {
    SelectBand(EffectId, usize),
    SetSpectrumSide(EffectId, SpectrumSide),
    ToggleFreeze(EffectId),
    ToggleAdvanced(EffectId),
}

impl EqMsg {
    pub fn effect_id(&self) -> EffectId {
        match self {
            Self::SelectBand(id, _)
            | Self::SetSpectrumSide(id, _)
            | Self::ToggleFreeze(id)
            | Self::ToggleAdvanced(id) => *id,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct EqPanelState {
    pub band: usize,
    pub side: SpectrumSide,
    pub frozen: bool,
    pub advanced: bool,
    pub input: MeterValues,
    pub output: MeterValues,
    pub last_meter_at: Option<Instant>,
}

impl EqPanelState {
    pub fn update(&mut self, message: EqMsg) {
        match message {
            EqMsg::SelectBand(_, band) if band < BAND_NAMES.len() => self.band = band,
            EqMsg::SelectBand(..) => {}
            EqMsg::SetSpectrumSide(_, side) => self.side = side,
            EqMsg::ToggleFreeze(_) => self.frozen = !self.frozen,
            EqMsg::ToggleAdvanced(_) => self.advanced = !self.advanced,
        }
    }
}

impl super::device_levels::MeteredPanel for EqPanelState {
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

    #[test]
    fn a_panel_selects_bands_listens_after_the_eq_and_toggles_freeze() {
        let id = EffectId::new();
        let mut panel = EqPanelState::default();
        assert_eq!(panel.side, SpectrumSide::Post);
        panel.update(EqMsg::SelectBand(id, 2));
        panel.update(EqMsg::SelectBand(id, 9));
        assert_eq!(panel.band, 2, "an unknown band is ignored");
        panel.update(EqMsg::SetSpectrumSide(id, SpectrumSide::Pre));
        panel.update(EqMsg::ToggleFreeze(id));
        assert_eq!(panel.side, SpectrumSide::Pre);
        assert!(panel.frozen);
    }

    #[test]
    fn audition_reads_the_soloed_band() {
        let mut params = vec![0.0; param::COUNT];
        assert_eq!(auditioned_band(&params), None);
        params[param::AUDITION] = 3.0;
        assert_eq!(auditioned_band(&params), Some(2));
    }
}
