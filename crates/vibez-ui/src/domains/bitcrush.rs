//! The names a native Bitcrush card gives its choices.

/// Index-aligned with the DSP's rounding modes.
pub const ROUNDING_NAMES: [&str; 3] = ["Round", "Floor", "Truncate"];

pub fn is_native_bitcrush(effect: &crate::state::UiEffect) -> bool {
    effect.effect_type == vibez_core::effect::EffectType::Bitcrush
        && effect.plugin_ref.is_none()
        && effect.plugin_name.is_none()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_cover_every_rounding_the_dsp_offers() {
        let rounding = &vibez_dsp::bitcrush::BITCRUSH_PARAMS[vibez_dsp::bitcrush::param::ROUNDING];
        assert_eq!(ROUNDING_NAMES.len(), rounding.max as usize + 1);
    }
}
