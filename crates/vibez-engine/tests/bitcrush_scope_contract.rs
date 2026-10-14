//! Native Bitcrush scope packets describe actual left-channel audio and acquisitions.

use vibez_core::effect::EffectType;
use vibez_dsp::factory::create_effect_with_params;
use vibez_engine::events::EngineEvent;

#[test]
fn sample_hold_snapshot_contains_actual_left_audio_and_the_acquisition_schedule() {
    println!("EngineEvent size={} align={}",
        std::mem::size_of::<EngineEvent>(), std::mem::align_of::<EngineEvent>());
    let input = [-0.8, -0.6, -0.3, -0.2, 0.0, 0.2, 0.3, 0.6, 0.8, 1.2, 1.5, -1.25];
    let right = [0.91, -0.8, 0.6, -0.4, 0.2, 0.0, -0.2, 0.4, -0.6, 0.8, -1.0, 1.5];
    let held = [-1.0, -1.0, -1.0, -0.5, -0.5, -0.5, 0.5, 0.5, 0.5, 1.0, 1.0, 1.0];
    let expected: Vec<_> = input.iter().zip(held).map(|(&dry, wet)| dry * 0.75 + wet * 0.25).collect();
    let mut audio: Vec<_> = input.into_iter().zip(right).flat_map(|(left, right)| [left, right]).collect();
    let mut effect = create_effect_with_params(EffectType::Bitcrush, 48_000.0, &[2.0, 3.0, 0.25, 6.0]);
    effect.process(&mut audio, 2);
    let actual_left: Vec<_> = audio.chunks_exact(2).map(|frame| frame[0]).collect();
    assert_eq!(actual_left, expected);
    let meter = effect.take_sample_hold_meter().expect("native Bitcrush scope");
    assert_eq!(meter.count, 12);
    assert_eq!(meter.sample_rate, 48_000.0);
    assert_eq!(meter.first_source_frame, 0);
    assert_eq!(meter.latency_samples, 0);
    assert_eq!(meter.primary_interval_frames, 3);
    assert_eq!(meter.secondary_interval_frames, 0);
    assert!(!meter.transition_active);
    assert_eq!(&meter.input[..12], &input);
    assert_eq!(&meter.output[..12], actual_left.as_slice());
    assert_eq!(meter.primary_acquisitions, [0x249, 0]);
    assert_eq!(meter.secondary_acquisitions, [0; 2]);
    assert_eq!(meter.blending, [0; 2]);
}
