//! Static Filter modulation models using configured depth and envelope time constants.

use vibez_dsp::filter::DEPTH_MAX_OCTAVES;

pub(crate) fn model_value(
    source: usize,
    position: f32,
    depth: f32,
    phase_degrees: f32,
    attack_ms: f32,
    release_ms: f32,
) -> f32 {
    let amount = depth / DEPTH_MAX_OCTAVES;
    match source {
        1 => (std::f32::consts::TAU * (position * 2.0 + phase_degrees / 360.0)).sin() * amount,
        2 => {
            let hold = attack_ms * 3.0;
            let time = position * (hold + release_ms * 5.0);
            if time <= hold {
                (1.0 - (-time / attack_ms).exp()) * amount
            } else {
                (1.0 - (-hold / attack_ms).exp()) * (-(time - hold) / release_ms).exp() * amount
            }
        }
        _ => 0.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shape_preview_uses_sine_depth_and_stereo_phase_without_claiming_live_phase() {
        let sample = |position, depth, phase| model_value(1, position, depth, phase, 10.0, 150.0);
        assert_eq!(sample(0.0, 6.0, 0.0), 0.0);
        assert!((sample(0.125, 6.0, 0.0) - 1.0).abs() < 1e-6);
        assert!((sample(0.125, 3.0, 0.0) - 0.5).abs() < 1e-6);
        assert!((sample(0.125, 6.0, 180.0) + 1.0).abs() < 1e-6);
        assert_eq!(sample(0.125, 0.0, 0.0), 0.0);
        assert_eq!(model_value(0, 0.125, 6.0, 0.0, 10.0, 150.0), 0.0);
    }
    #[test]
    fn envelope_preview_matches_the_attack_and_release_time_constants() {
        let (attack, release) = (10.0, 150.0);
        let duration = 3.0 * attack + 5.0 * release;
        let at_attack = model_value(2, attack / duration, 6.0, 0.0, attack, release);
        assert!((at_attack - (1.0 - (-1.0_f32).exp())).abs() < 1e-6);
        let after_release = model_value(
            2,
            (3.0 * attack + release) / duration,
            6.0,
            0.0,
            attack,
            release,
        );
        assert!((after_release - (1.0 - (-3.0_f32).exp()) * (-1.0_f32).exp()).abs() < 1e-6);
    }
}
