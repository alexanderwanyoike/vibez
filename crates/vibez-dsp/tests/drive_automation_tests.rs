//! Control changes follow source audio through the declared FIR latency.

use vibez_core::effect::EffectType;
use vibez_dsp::factory::create_effect_with_params;

#[test]
fn modern_tone_and_post_filter_automation_wait_for_their_source_audio() {
    let mut tone = create_effect_with_params(EffectType::Drive, 48_000.0, &[1.0, 0.5, 1.0, 1.0, 1.0]);
    let mut warm = [0.02; 4096];
    tone.process(&mut warm, 1);
    let neutral = warm[4095];
    assert!(tone.set_automated_param(1, 1.0));
    let mut output = [0.02; 128];
    tone.process(&mut output, 1);
    for (frame, actual) in output.into_iter().enumerate() {
        let expected = neutral * if frame < 64 { 1.0 } else { 10.0_f32.powf(-6.0 / 20.0) };
        assert!((actual - expected).abs() < 2e-6, "Tone frame={frame}: {actual} != {expected}");
    }
    let mut filter = create_effect_with_params(EffectType::Drive, 48_000.0, &[1.0, 0.5, 1.0, 1.0, 0.0, 2.0, 0.0, 80.0, 0.0, 1000.0]);
    let mut warm: Vec<_> = (0..4096).map(|n| if n % 2 == 0 { 0.02 } else { -0.02 }).collect();
    filter.process(&mut warm, 1);
    assert!(filter.set_automated_param(8, 1.0));
    let mut output: Vec<_> = (0..128).map(|n| if n % 2 == 0 { 0.02 } else { -0.02 }).collect();
    filter.process(&mut output, 1);
    for (frame, actual) in output.into_iter().enumerate() {
        let input = if frame % 2 == 0 { 0.6_f32 } else { -0.6 };
        let expected = if frame < 64 { input.tanh() } else { 0.0 };
        assert!((actual - expected).abs() < 2e-6, "Post LP frame={frame}: {actual} != {expected}");
    }
}
