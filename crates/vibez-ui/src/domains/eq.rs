//! What a native EQ card shows besides its parameters: the band being
//! edited, which side of the EQ the analyser listens to, and whether the
//! analyser is frozen.

use vibez_core::id::EffectId;

pub const BAND_NAMES: [&str; 4] = ["LF", "LMF", "HMF", "HF"];

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
}
