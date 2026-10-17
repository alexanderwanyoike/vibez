//! The names a native Drive card gives its choices.

/// Index-aligned with `vibez_dsp::drive::character`.
pub const CHARACTER_NAMES: [&str; 4] = ["Classic", "Soft", "Warm", "Hard"];
/// Index-aligned with `vibez_dsp::drive::quality`.
pub const QUALITY_NAMES: [&str; 3] = ["1x", "2x", "4x"];

pub fn is_native_drive(effect: &crate::state::UiEffect) -> bool {
    effect.effect_type == vibez_core::effect::EffectType::Drive
        && effect.plugin_ref.is_none()
        && effect.plugin_name.is_none()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_cover_every_choice_the_dsp_offers() {
        assert_eq!(CHARACTER_NAMES.len(), vibez_dsp::drive::character::COUNT);
        assert_eq!(QUALITY_NAMES.len(), vibez_dsp::drive::quality::COUNT);
    }
}
