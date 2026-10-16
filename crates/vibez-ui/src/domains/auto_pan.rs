//! The names a native Auto-Pan card gives its choices, and where its output
//! sits between the speakers.

use vibez_engine::metering::MeterValues;

/// Index-aligned with `vibez_dsp::auto_pan::SYNC_BEATS`, after "Free".
pub const SYNC_NAMES: [&str; 8] = [
    "Free", "2 bars", "1 bar", "1/2", "1/4", "1/8", "1/16", "1/32",
];
/// Where Sync starts when it is turned on.
pub const DEFAULT_SYNC: usize = 5;
/// Index-aligned with `vibez_dsp::auto_pan::shape`.
pub const SHAPE_NAMES: [&str; 5] = ["Sine", "Square", "Triangle", "Saw Up", "Saw Down"];
pub const MODE_NAMES: [&str; 2] = ["Pan", "Tremolo"];
pub const RETRIGGER_NAMES: [&str; 2] = ["Free Run", "Song"];

/// Quieter than this, there is nothing to place.
const SILENT_DB: f32 = -60.0;

pub fn is_native_auto_pan(effect: &crate::state::UiEffect) -> bool {
    effect.effect_type == vibez_core::effect::EffectType::AutoPan
        && effect.plugin_ref.is_none()
        && effect.plugin_name.is_none()
}

/// The output's balance from -1 (all left) to 1 (all right), by power, or
/// `None` while it is silent.
pub fn stereo_position(output: MeterValues) -> Option<f32> {
    let silent = vibez_core::level::db_to_amplitude(SILENT_DB);
    if output.rms_l.max(output.rms_r) < silent {
        return None;
    }
    let (left, right) = (output.rms_l.powi(2), output.rms_r.powi(2));
    Some((right - left) / (right + left))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn level(left: f32, right: f32) -> MeterValues {
        MeterValues {
            peak_l: left,
            peak_r: right,
            rms_l: left,
            rms_r: right,
        }
    }

    #[test]
    fn names_cover_every_choice_the_dsp_offers() {
        assert_eq!(SYNC_NAMES.len(), vibez_dsp::auto_pan::SYNC_BEATS.len() + 1);
        assert_eq!(SYNC_NAMES[DEFAULT_SYNC], "1/8");
        assert_eq!(SHAPE_NAMES.len(), vibez_dsp::auto_pan::shape::SAW_DOWN + 1);
    }

    #[test]
    fn the_position_follows_the_louder_side() {
        assert_eq!(stereo_position(level(0.5, 0.5)), Some(0.0));
        assert_eq!(stereo_position(level(0.0, 0.5)), Some(1.0));
        assert_eq!(stereo_position(level(0.5, 0.0)), Some(-1.0));
        assert!(stereo_position(level(0.5, 0.25)).unwrap() < -0.5);
        assert_eq!(stereo_position(MeterValues::SILENT), None);
    }
}
