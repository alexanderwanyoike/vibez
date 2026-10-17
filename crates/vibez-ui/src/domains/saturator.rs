//! The names a native Saturator card gives its choices, its plot and
//! Advanced pages, and what dragging a node on its curve changes.

use vibez_dsp::saturator::{curve, param, shape, CUSTOM_INPUT_DB, SATURATOR_PARAMS};

/// Index-aligned with `vibez_dsp::saturator::shape`.
pub const SHAPE_NAMES: [&str; 4] = ["Soft", "Tube", "Fold", "Custom"];
/// Index-aligned with `PRE_FILTER_HZ`.
pub const PRE_FILTER_NAMES: [&str; 5] =
    ["Off", "20 Hz HPF", "40 Hz HPF", "80 Hz HPF", "160 Hz HPF"];
/// Index-aligned with `POST_FILTER_HZ`.
pub const POST_FILTER_NAMES: [&str; 6] = [
    "Off",
    "20 kHz LPF",
    "16 kHz LPF",
    "12 kHz LPF",
    "8 kHz LPF",
    "4 kHz LPF",
];
pub use crate::domains::drive::QUALITY_NAMES;

pub const CURVE_PLOT: usize = 0;
pub const SPECTRUM_PLOT: usize = 1;

pub const SHAPE_PAGE: usize = 0;
pub const FILTERS_PAGE: usize = 1;
pub const OUTPUT_PAGE: usize = 2;

/// Custom's three points, in the order its nodes sit on the curve.
pub const NODES: [usize; 3] = [param::CUSTOM_LOW, param::CUSTOM_MID, param::CUSTOM_HIGH];

pub fn is_native_saturator(effect: &crate::state::UiEffect) -> bool {
    effect.effect_type == vibez_core::effect::EffectType::Saturator
        && effect.plugin_ref.is_none()
        && effect.plugin_name.is_none()
}

fn value(params: &[f32], index: usize) -> f32 {
    params
        .get(index)
        .copied()
        .unwrap_or(SATURATOR_PARAMS[index].default)
}

fn clamped(index: usize, db: f32) -> f32 {
    let descriptor = &SATURATOR_PARAMS[index];
    db.clamp(descriptor.min, descriptor.max)
}

/// Custom's points as the current curve passes them, so switching to
/// Custom starts from what was heard.
pub fn points_of(params: &[f32]) -> [f32; 3] {
    let shape = value(params, param::SHAPE).round() as usize;
    let amount = value(params, param::CURVE);
    let points = NODES.map(|index| value(params, index));
    std::array::from_fn(|node| {
        let input = 10f32.powf(CUSTOM_INPUT_DB[node] / 20.0);
        let out = curve(shape, amount, points, input).abs().max(1e-6);
        clamped(NODES[node], 20.0 * out.log10())
    })
}

/// The edits that move `node` to `db`. A node dragged on another shape
/// makes the curve Custom, keeping the other two points where they were.
pub fn drag_node(params: &[f32], node: usize, db: f32) -> Vec<(usize, f32)> {
    let index = NODES[node];
    if value(params, param::SHAPE).round() as usize == shape::CUSTOM {
        return vec![(index, clamped(index, db))];
    }
    let mut points = points_of(params);
    points[node] = clamped(index, db);
    let mut edits = vec![(param::SHAPE, shape::CUSTOM as f32)];
    edits.extend(NODES.into_iter().zip(points));
    edits
}

#[cfg(test)]
mod tests {
    use super::*;
    use vibez_dsp::saturator::{POST_FILTER_HZ, PRE_FILTER_HZ};

    fn defaults() -> Vec<f32> {
        SATURATOR_PARAMS.iter().map(|d| d.default).collect()
    }

    #[test]
    fn names_cover_every_choice_the_dsp_offers() {
        assert_eq!(SHAPE_NAMES.len(), shape::CUSTOM + 1);
        assert_eq!(PRE_FILTER_NAMES.len(), PRE_FILTER_HZ.len());
        assert_eq!(POST_FILTER_NAMES.len(), POST_FILTER_HZ.len());
        let max = |index: usize| SATURATOR_PARAMS[index].max as usize;
        assert_eq!(SHAPE_NAMES.len(), max(param::SHAPE) + 1);
        assert_eq!(PRE_FILTER_NAMES.len(), max(param::PRE_FILTER) + 1);
        assert_eq!(POST_FILTER_NAMES.len(), max(param::POST_FILTER) + 1);
        assert_eq!(QUALITY_NAMES.len(), max(param::QUALITY) + 1);
    }

    #[test]
    fn dragging_a_soft_node_makes_a_custom_curve_that_sounds_the_same_elsewhere() {
        let params = defaults();
        let edits = drag_node(&params, 2, 3.0);
        assert_eq!(edits[0], (param::SHAPE, shape::CUSTOM as f32));
        let mut custom = params.clone();
        for (index, value) in &edits {
            custom[*index] = *value;
        }
        assert_eq!(custom[param::CUSTOM_HIGH], 3.0);
        // The undragged points follow Soft.
        let soft = |db: f32| curve(shape::SOFT, 0.75, [0.0; 3], 10f32.powf(db / 20.0));
        for node in [0, 1] {
            let heard = 20.0 * soft(CUSTOM_INPUT_DB[node]).log10();
            assert!((custom[NODES[node]] - heard).abs() < 1e-3, "{node}");
        }
    }

    #[test]
    fn dragging_a_custom_node_moves_only_that_point_within_its_range() {
        let mut params = defaults();
        params[param::SHAPE] = shape::CUSTOM as f32;
        assert_eq!(drag_node(&params, 1, 3.0), vec![(param::CUSTOM_MID, 3.0)]);
        assert_eq!(drag_node(&params, 0, 9.0), vec![(param::CUSTOM_LOW, 0.0)]);
    }
}
