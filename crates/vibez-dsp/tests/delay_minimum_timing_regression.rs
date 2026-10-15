//! Keeps applied musical timing observations consistent with the causal one-sample history tap.

use vibez_core::audio_context::DeviceAudioContext;
use vibez_dsp::{delay::{DelayEffect, param}, effect::AudioEffect};

#[test]
fn sub_sample_sync_targets_keep_their_divisions_but_report_the_actual_one_sample_delay() {
    for rate in [1000, 1001] {
        for linked in [false, true] {
            let mut effect = DelayEffect::new(rate as f32);
            let right_division = if linked { 6.0 } else { 0.0 };
            let right_modifier = if linked { 1.0 } else { 2.0 };
            let stored = [
                (param::SYNC, 1.0),
                (param::LEFT_DIVISION, 0.0),
                (param::LEFT_MODIFIER, 2.0),
                (param::LINK, f32::from(u8::from(linked))),
                (param::RIGHT_DIVISION, right_division),
                (param::RIGHT_MODIFIER, right_modifier),
                (param::FEEDBACK, 0.5),
                (param::MIX, 1.0),
            ];
            for (index, value) in stored { assert!(effect.set_param(index, value)); }
            effect.set_audio_context(DeviceAudioContext {
                musical_sample: 0, continuous_sample: 0, sample_rate: rate,
                bpm: 999.0, playing: true,
            });
            let requested = (60.0 / 999.0) * (4.0 / 64.0) * (2.0 / 3.0) * f64::from(rate);
            assert!(requested > 0.0 && requested < 1.0);
            for pass in 0..2 {
                if pass == 1 { effect.reset(); }
                let mut audio = [0.0; 16];
                audio[..2].copy_from_slice(&[0.25, -0.125]);
                effect.process(&mut audio, 2);
                for (frame, pair) in audio.chunks_exact(2).enumerate() {
                    for (channel, &actual) in pair.iter().enumerate() {
                        let expected = if frame == 0 { 0.0 } else {
                            [0.25, -0.125][channel] * 0.5_f32.powi(frame as i32 - 1)
                        };
                        assert_eq!(actual, expected,
                            "rate={rate}, linked={linked}, pass={pass}, frame={frame}, channel={channel}");
                    }
                }
                let snapshot = effect.delay_snapshot().unwrap();
                assert_eq!(snapshot.sample_rate, rate);
                assert_eq!(snapshot.bpm, 999.0);
                assert!(!snapshot.reference_moving);
                for (index, value) in stored {
                    assert_eq!(effect.get_param(index).to_bits(), value.to_bits());
                    assert_eq!(snapshot.targets[index].to_bits(), value.to_bits());
                }
                for (line, state) in snapshot.lines.iter().enumerate() {
                    assert!(!state.moving && !state.gliding);
                    assert_eq!(state.remaining_frames, 0);
                    assert_eq!(state.pending_samples, None);
                    assert_eq!((state.old_samples, state.destination_samples, state.target_samples),
                        (1.0, 1.0, 1.0),
                        "audio already repeats every sample but snapshot reports a fractional delay: rate={rate}, linked={linked}, pass={pass}, line={line}");
                }
            }
        }
    }
}
