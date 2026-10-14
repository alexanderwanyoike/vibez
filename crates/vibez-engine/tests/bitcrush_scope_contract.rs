//! Native Bitcrush scope packets describe actual left-channel audio and acquisitions.

use vibez_core::effect::{EffectType, SampleHoldMeter};
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


fn assert_five_frame_window(meter: SampleHoldMeter, input: &[f32], output: &[f32], first: u64) {
    let count = input.len() / 2;
    assert_eq!(usize::from(meter.count), count);
    assert_eq!(meter.first_source_frame, first);
    assert_eq!(meter.sample_rate, 48_000.0);
    assert_eq!(meter.primary_interval_frames, 5);
    for point in 0..count {
        assert_eq!(meter.input[point], input[point * 2]);
        assert_eq!(meter.output[point], output[point * 2]);
    }
    for point in 0..128 {
        let observed = meter.primary_acquisitions[point / 64] & (1_u64 << (point % 64)) != 0;
        assert_eq!(observed, point < count && (first + point as u64) % 5 == 0,
            "point={point}, first={first}, count={count}");
    }
    assert_eq!(meter.secondary_acquisitions, [0; 2]);
    assert_eq!(meter.blending, [0; 2]);
}

#[test]
fn scope_windows_wrap_and_drain_without_advancing_audio_or_confusing_silence_with_absence() {
    let build = || create_effect_with_params(EffectType::Bitcrush, 48_000.0, &[4.0, 5.0, 0.37, 6.0]);
    let mut effect = build();
    let absent = effect.take_sample_hold_meter().unwrap();
    assert_eq!(absent.count, 0);
    assert_eq!(absent.first_source_frame, 0);
    let input: Vec<_> = (0..330).flat_map(|frame| {
        [frame as f32 / 321.0 - 0.5, 0.3 - frame as f32 / 231.0]
    }).collect();
    let mut actual = input.clone();
    let mut reference = input.clone();
    build().process(&mut reference, 2);
    effect.process(&mut actual[..300], 2);
    assert_five_frame_window(effect.take_sample_hold_meter().unwrap(), &input[44..300], &actual[44..300], 22);
    let mut untouched = [0.7, -0.4];
    effect.process(&mut untouched, 0);
    assert_eq!(untouched, [0.7, -0.4]);
    effect.process(&mut [], 2);
    for _ in 0..2 {
        let absent = effect.take_sample_hold_meter().unwrap();
        assert_eq!(absent.count, 0);
        assert_eq!(absent.first_source_frame, 150);
        assert_eq!(absent.primary_acquisitions, [0; 2]);
    }
    effect.process(&mut actual[300..302], 2);
    assert_five_frame_window(effect.take_sample_hold_meter().unwrap(), &input[300..302], &actual[300..302], 150);
    effect.process(&mut actual[302..], 2);
    assert_eq!(actual, reference, "taking observations must not affect processed audio");
    assert_five_frame_window(effect.take_sample_hold_meter().unwrap(), &input[404..], &actual[404..], 202);
    let mut silence = [0.0; 24];
    effect.process(&mut silence, 2);
    assert_eq!(silence, [0.0; 24]);
    assert_five_frame_window(effect.take_sample_hold_meter().unwrap(), &[0.0; 24], &silence, 330);
}
