//! The names a native Reverb card gives its choices.

/// Index-aligned with `vibez_dsp::reverb::PREDELAY_SYNC_BEATS`, after "Free".
pub const PREDELAY_SYNC_NAMES: [&str; 8] = [
    "Free", "1/64", "1/32", "1/16", "1/16 D", "1/8", "1/8 D", "1/4",
];
/// Where Sync starts when it is turned on.
pub const DEFAULT_SYNC: usize = 5;

pub fn is_native_reverb(effect: &crate::state::UiEffect) -> bool {
    effect.effect_type == vibez_core::effect::EffectType::Reverb
        && effect.plugin_ref.is_none()
        && effect.plugin_name.is_none()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_cover_every_synced_pre_delay_the_dsp_offers() {
        assert_eq!(
            PREDELAY_SYNC_NAMES.len(),
            vibez_dsp::reverb::PREDELAY_SYNC_BEATS.len() + 1
        );
        assert_eq!(PREDELAY_SYNC_NAMES[DEFAULT_SYNC], "1/8");
    }
}
