//! Public Drive behavior stays aligned, controllable and observable without callback allocation.

use vibez_core::effect::EffectType;
use vibez_dsp::factory::create_effect_with_params;

#[test]
fn legacy_and_dry_mix_have_the_same_declared_exact_64_frame_delay() {
    for controls in [[0.0, 0.5, 1.0, 1.0], [1.0, 1.0, 0.7, 0.0]] {
        let mut effect = create_effect_with_params(EffectType::Drive, 48_000.0, &controls);
        assert_eq!(effect.latency_samples(), 64);
        let mut impulse = [0.0; 260];
        impulse[0] = 0.5;
        impulse[1] = -0.25;
        effect.process(&mut impulse, 2);
        for (frame, pair) in impulse.as_chunks::<2>().0.iter().enumerate() {
            let expected = if frame == 64 { [0.5 * controls[2], -0.25 * controls[2]] } else { [0.0; 2] };
            assert_eq!(*pair, expected, "controls={controls:?}, frame={frame}");
        }
    }
}
