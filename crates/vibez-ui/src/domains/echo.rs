//! The names a native Echo card gives its choices, and the Advanced pages a
//! narrow card shows one at a time.

/// Index-aligned with `vibez_dsp::echo::GRIDS`: shortest first, "D" dotted
/// and "T" triplet.
pub const GRID_NAMES: [&str; 21] = [
    "1/64 T", "1/64", "1/32 T", "1/64 D", "1/32", "1/16 T", "1/32 D", "1/16", "1/8 T", "1/16 D",
    "1/8", "1/4 T", "1/8 D", "1/4", "1/2 T", "1/4 D", "1/2", "1/1 T", "1/2 D", "1/1", "1/1 D",
];

/// Index-aligned with `vibez_dsp::echo::style`.
pub const STYLE_NAMES: [&str; 3] = ["Clean", "Tape", "Diffuse"];

pub const CHARACTER_PAGE: usize = 0;
pub const MOTION_PAGE: usize = 1;
pub const EQ_PAGE: usize = 2;

/// A grid's name as the Grid menu spells it out, "1/4 Dotted".
pub fn grid_long_name(index: usize) -> String {
    let short = GRID_NAMES[index.min(GRID_NAMES.len() - 1)];
    match short.split_once(' ') {
        Some((division, "D")) => format!("{division} Dotted"),
        Some((division, "T")) => format!("{division} Triplet"),
        _ => short.to_string(),
    }
}

pub fn is_native_echo(effect: &crate::state::UiEffect) -> bool {
    effect.effect_type == vibez_core::effect::EffectType::Echo
        && effect.plugin_ref.is_none()
        && effect.plugin_name.is_none()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domains::delay::{DIVISION_NAMES, MODIFIER_NAMES};
    use vibez_dsp::echo::GRIDS;

    #[test]
    fn grid_names_follow_the_dsps_grids() {
        for (index, &(division, modifier)) in GRIDS.iter().enumerate() {
            let suffix = ["", " D", " T"][modifier];
            assert_eq!(
                GRID_NAMES[index],
                format!("{}{suffix}", DIVISION_NAMES[division]),
                "{index}"
            );
            let long = grid_long_name(index);
            assert!(long.starts_with(DIVISION_NAMES[division]), "{long}");
            if modifier != 0 {
                assert!(long.ends_with(MODIFIER_NAMES[modifier]), "{long}");
            }
        }
    }
}
