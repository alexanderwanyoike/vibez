//! The names a native Phaser card gives its choices.

/// Index-aligned with `vibez_dsp::phaser::SYNC_BEATS`, after "Free".
pub const SYNC_NAMES: [&str; 7] = ["Free", "2 bars", "1 bar", "1/2", "1/4", "1/8", "1/16"];
/// Where Sync starts when it is turned on.
pub const DEFAULT_SYNC: usize = 4;
/// Index-aligned with `vibez_dsp::phaser::STAGE_COUNTS`.
pub const STAGE_NAMES: [(usize, &str); 5] = [(0, "2"), (1, "4"), (2, "6"), (3, "8"), (4, "12")];
pub const POLARITY_NAMES: [&str; 2] = ["Positive", "Negative"];
pub const SHAPE_NAMES: [&str; 2] = ["Sine", "Triangle"];

pub fn is_native_phaser(effect: &crate::state::UiEffect) -> bool {
    effect.effect_type == vibez_core::effect::EffectType::Phaser
        && effect.plugin_ref.is_none()
        && effect.plugin_name.is_none()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_cover_every_sync_the_dsp_offers() {
        assert_eq!(SYNC_NAMES.len(), vibez_dsp::phaser::SYNC_BEATS.len() + 1);
        assert_eq!(SYNC_NAMES[DEFAULT_SYNC], "1/4");
        for ((index, name), count) in STAGE_NAMES.iter().zip(vibez_dsp::phaser::STAGE_COUNTS) {
            assert_eq!(STAGE_NAMES[*index].0, *index);
            assert_eq!(*name, count.to_string());
        }
        assert_eq!(STAGE_NAMES.len(), vibez_dsp::phaser::STAGE_COUNTS.len());
    }
}
