//! The names a native Delay card gives its choices.

/// Index-aligned with `vibez_dsp::delay::synced_beats`.
pub const DIVISION_NAMES: [&str; 7] = ["1/64", "1/32", "1/16", "1/8", "1/4", "1/2", "1/1"];
pub const MODIFIER_NAMES: [&str; 3] = ["Straight", "Dotted", "Triplet"];
/// The Advanced groups a narrow card pages between.
pub const STEREO_PAGE: usize = 0;
pub const FEEDBACK_PAGE: usize = 1;

pub fn is_native_delay(effect: &crate::state::UiEffect) -> bool {
    effect.effect_type == vibez_core::effect::EffectType::Delay
        && effect.plugin_ref.is_none()
        && effect.plugin_name.is_none()
}

#[cfg(test)]
mod tests {
    use super::*;
    use vibez_dsp::delay::{param, DELAY_PARAMS};

    #[test]
    fn names_cover_every_division_and_modifier_the_dsp_offers() {
        assert_eq!(
            DIVISION_NAMES.len(),
            DELAY_PARAMS[param::LEFT_DIVISION].max as usize + 1
        );
        assert_eq!(
            MODIFIER_NAMES.len(),
            DELAY_PARAMS[param::LEFT_MODIFIER].max as usize + 1
        );
    }
}
