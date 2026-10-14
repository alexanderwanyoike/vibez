//! Manual sound edits advance on source frames while automation keeps exact control timing.

use vibez_core::effect::EffectType;
use vibez_dsp::{effect::AudioEffect, factory::create_effect_with_params};

fn drive_case(channels: usize) -> Box<dyn AudioEffect> {
    let mut effect =
        create_effect_with_params(EffectType::Bitcrush, 48_000.0, &[16.0, 1.0, 1.0, 0.0]);
    effect.process(&mut vec![0.0437; 9 * channels], channels);
    assert!(effect.set_param(3, 12.0));
    assert_eq!(effect.get_param(3), 12.0);
    effect
}

#[test]
fn manual_drive_ramps_on_source_frames_and_automation_snaps_without_reviving_the_ramp() {
    let quantum = 1.0 / 32_768.0;
    for channels in [1, 2] {
        let mut whole = vec![0.0437; 640 * channels];
        drive_case(channels).process(&mut whole, channels);
        let mut split = vec![0.0437; 640 * channels];
        let mut effect = drive_case(channels);
        let mut position = 0;
        for frames in [1, 7, 31, 97, 3, 501] {
            let end = position + frames * channels;
            effect.process(&mut split[position..end], channels);
            position = end;
        }
        assert_eq!(whole, split, "channels={channels}");
        for (frame, samples) in whole.chunks_exact(channels).enumerate() {
            let drive_db = 12.0 * ((frame + 1).min(512) as f64 / 512.0);
            let driven = f64::from(0.0437_f32) * 10.0_f64.powf(drive_db / 20.0);
            let expected = (driven * 32_768.0).round() as f32 / 32_768.0;
            for &sample in samples {
                assert!(
                    (sample - expected).abs() <= quantum,
                    "frame={frame}, expected={expected}, actual={sample}"
                );
            }
        }
        assert_eq!(
            &whole[512 * channels..],
            &vec![whole[512 * channels]; 128 * channels]
        );

        let mut automated = drive_case(channels);
        automated.process(&mut vec![0.0437; 173 * channels], channels);
        assert!(automated.set_automated_param(3, 6.0));
        assert_eq!(automated.get_param(3), 6.0);
        let mut actual = vec![0.0437; 640 * channels];
        automated.process(&mut actual, channels);
        let mut hydrated =
            create_effect_with_params(EffectType::Bitcrush, 48_000.0, &[16.0, 1.0, 1.0, 6.0]);
        let mut expected = vec![0.0437; 640 * channels];
        hydrated.process(&mut expected, channels);
        assert_eq!(
            actual, expected,
            "automation must replace the active Drive ramp"
        );
    }
}


#[test]
fn mix_ramp_survives_unrelated_automation_and_unchanged_manual_targets() {
    for channels in [1, 2] {
        let build = || {
            let mut effect = create_effect_with_params(
                EffectType::Bitcrush, 48_000.0, &[2.0, 3.0, 0.0, 0.0]);
            effect.process(&mut vec![0.26; 9 * channels], channels);
            assert!(effect.set_param(2, 1.0));
            assert_eq!(effect.get_param(2), 1.0);
            effect
        };
        let mut whole = vec![0.26; 640 * channels];
        build().process(&mut whole, channels);
        let mut effect = build();
        let mut actual = vec![0.26; 640 * channels];
        let mut position = 0;
        for frames in [127, 46, 1, 7, 31, 428] {
            let end = position + frames * channels;
            effect.process(&mut actual[position..end], channels);
            position = end;
            if position == 127 * channels {
                assert!(effect.set_automated_param(3, 6.0));
            }
            if position == 173 * channels {
                assert!(effect.set_param(2, 1.0));
            }
        }
        assert_eq!(actual, whole, "channels={channels}");
        for (frame, samples) in actual.chunks_exact(channels).enumerate() {
            let mix = (frame + 1).min(512) as f32 / 512.0;
            let expected = 0.26 * (1.0 - mix) + 0.5 * mix;
            for &sample in samples {
                assert_eq!(sample, expected, "frame={frame}, channels={channels}");
            }
        }
    }
}
