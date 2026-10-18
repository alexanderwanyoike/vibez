//! The names a native Limiter card gives its choices.

/// Index-aligned with `vibez_dsp::limiter::style`.
pub const STYLE_NAMES: [&str; 3] = ["Transparent", "Punchy", "Loud"];

pub fn is_native_limiter(effect: &crate::state::UiEffect) -> bool {
    effect.effect_type == vibez_core::effect::EffectType::Limiter
        && effect.plugin_ref.is_none()
        && effect.plugin_name.is_none()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_cover_every_style_the_dsp_offers() {
        assert_eq!(STYLE_NAMES.len(), vibez_dsp::limiter::style::COUNT);
    }
}
