//! Compressor sound, timing, sidechain and compatibility contracts.

use super::*;
use vibez_core::routing::{ExternalInputBlock, ExternalInputId};

fn configured(rate: f32, edits: &[(usize, f32)]) -> CompressorEffect {
    let mut effect = CompressorEffect::new(rate);
    for &(index, value) in edits {
        assert!(effect.set_param(index, value), "index {index}");
    }
    effect
}

fn db(value: f32) -> f32 {
    20.0 * value.max(1e-10).log10()
}

fn sidechain<'a>(samples: &'a [f32], channels: usize, connected: bool) -> ExternalInputBlock<'a> {
    ExternalInputBlock {
        id: ExternalInputId(0),
        channels,
        samples,
        connected,
    }
}

#[test]
fn legacy_descriptors_and_output_timing_are_preserved() {
    let expected = [
        ("Threshold", -60.0, 0.0, -18.0, "dB"),
        ("Ratio", 1.0, 20.0, 4.0, ":1"),
        ("Attack", 0.1, 100.0, 5.0, "ms"),
        ("Release", 10.0, 500.0, 80.0, "ms"),
        ("Makeup", 0.0, 24.0, 3.0, "dB"),
        ("Mix", 0.0, 1.0, 1.0, ""),
    ];
    for rate in [44100.0, 48000.0, 96000.0] {
        let mut effect = CompressorEffect::new(rate);
        for (actual, expected) in effect.param_descriptors().iter().zip(expected) {
            assert_eq!(
                (
                    actual.name,
                    actual.min,
                    actual.max,
                    actual.default,
                    actual.unit
                ),
                expected
            );
        }
        let mut audio = [0.5; 2048];
        effect.process(&mut audio, 2);
        let mut envelope = -120.0_f32;
        let detector = db(0.5);
        let coefficient = (-1.0 / (0.005 * rate)).exp();
        for frame in audio.as_chunks::<2>().0 {
            envelope = detector + coefficient * (envelope - detector);
            let reduction = (envelope + 18.0).max(0.0) * 0.75;
            let expected = 0.5 * 10.0_f32.powf(-reduction / 20.0) * 10.0_f32.powf(3.0 / 20.0);
            assert!(
                (frame[0] - expected).abs() < 2e-6,
                "rate {rate}: {} vs {expected}",
                frame[0]
            );
            assert_eq!(frame[0], frame[1]);
        }
    }
}

#[test]
fn extension_defaults_are_neutral_and_metering_is_supported() {
    let effect = CompressorEffect::new(48000.0);
    assert_eq!(effect.param_descriptors().len(), 17);
    for (index, default) in [
        (6, 0.0),
        (7, 0.0),
        (8, 120.0),
        (9, 0.0),
        (10, 12000.0),
        (11, 0.0),
        (12, 0.0),
        (13, 1.0),
        (14, 0.0),
        (15, 0.0),
        (16, 60.0),
    ] {
        assert_eq!(effect.get_param(index), default);
    }
    assert!(effect.supports_level_meter());
    assert_eq!(effect.external_inputs()[0].id, ExternalInputId(0));
    assert_eq!(effect.latency_samples(), 0);
}

#[test]
fn zero_and_multichannel_layouts_preserve_audio_ownership() {
    let mut effect = configured(48000.0, &[(4, 6.0), (1, 1.0)]);
    let mut audio = [0.25, -0.25, 0.7, -0.8].repeat(1024);
    effect.process(&mut audio, 4);
    for frame in audio.as_chunks::<4>().0 {
        assert!((frame[0] - 0.25 * 10.0_f32.powf(6.0 / 20.0)).abs() < 1e-6);
        assert_eq!(frame[2..], [0.7, -0.8]);
    }
    let before = audio.clone();
    effect.process(&mut audio, 0);
    assert_eq!(before, audio);
}

#[test]
fn invalid_parameters_and_rates_cannot_poison_the_processor() {
    for rate in [f32::NAN, f32::INFINITY, 0.0, -48000.0, 2.0] {
        let mut effect = CompressorEffect::new(rate);
        for index in 0..effect.param_descriptors().len() {
            let before = effect.get_param(index);
            for invalid in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
                assert!(!effect.set_param(index, invalid));
                assert!(!effect.set_automated_param(index, invalid));
                assert_eq!(effect.get_param(index), before);
            }
        }
        let mut audio = [0.5; 1024];
        effect.process(&mut audio, 2);
        assert!(audio.iter().all(|sample| sample.is_finite()));
    }
}

#[test]
fn external_detector_missing_zero_channel_and_short_blocks_have_defined_fallbacks() {
    for (connected, channels, samples, compressed) in [
        (false, 2, &[0.0, 0.0][..], true),
        (true, 0, &[][..], true),
        (true, 2, &[0.0, 0.0][..], false),
        (true, 1, &[][..], false),
    ] {
        let mut effect = configured(48000.0, &[(0, -30.0), (1, 10.0), (2, 0.1), (4, 0.0)]);
        let mut audio = [0.5; 9600];
        effect.process_with_inputs(&mut audio, 2, &[sidechain(samples, channels, connected)]);
        assert_eq!(
            audio[9598] < 0.2,
            compressed,
            "connected {connected}, channels {channels}"
        );
    }
    let mut effect = configured(48000.0, &[(0, -30.0), (2, 0.1), (4, 0.0)]);
    let detector = vec![0.8; 4800];
    let mut audio = [0.1; 9600];
    effect.process_with_inputs(&mut audio, 2, &[sidechain(&detector, 1, true)]);
    assert!(audio[9598] < 0.03);
    assert_eq!(audio[9598], audio[9599]);
}

#[test]
fn range_limits_wet_reduction_and_dry_mix_keeps_the_original_audio() {
    let mut effect = configured(
        48000.0,
        &[
            (0, -60.0),
            (1, 20.0),
            (2, 0.1),
            (4, 0.0),
            (15, 1.0),
            (16, 6.0),
        ],
    );
    let mut audio = [0.5; 9600];
    effect.process(&mut audio, 2);
    assert!((audio[9598] - 0.5 * 10.0_f32.powf(-6.0 / 20.0)).abs() < 1e-6);
    let mut dry = configured(48000.0, &[(0, -60.0), (4, 24.0), (5, 0.0)]);
    let mut audio = [0.5, -0.25].repeat(4800);
    let before = audio.clone();
    dry.process(&mut audio, 2);
    assert_eq!(audio, before);
}

#[test]
fn manual_makeup_ramps_and_automation_snaps_without_changing_other_ramps() {
    let mut effect = configured(48000.0, &[(1, 1.0), (4, 0.0)]);
    effect.process(&mut [0.25; 128], 2);
    effect.set_param(4, 12.0);
    effect.set_param(0, -50.0);
    let mut first = [0.25; 2];
    effect.process(&mut first, 2);
    assert!((first[0] - 0.25).abs() < 0.002);
    effect.set_automated_param(4, 12.0);
    let mut immediate = [0.25; 2];
    effect.process(&mut immediate, 2);
    assert!((immediate[0] - 0.25 * 10.0_f32.powf(12.0 / 20.0)).abs() < 1e-6);
    assert_eq!(effect.get_param(0), -50.0);
}

#[test]
fn unlinked_stereo_keeps_the_quiet_channel_uncompressed() {
    let mut effect = configured(
        48000.0,
        &[(0, -18.0), (1, 10.0), (2, 0.1), (4, 0.0), (13, 0.0)],
    );
    let mut audio = [0.8, 0.02].repeat(4800);
    effect.process(&mut audio, 2);
    assert!(audio[9598] < 0.2);
    assert!((audio[9599] - 0.02).abs() < 1e-6);
}

#[test]
fn detector_listen_is_the_filtered_detector_and_filters_reject_out_of_band_energy() {
    for (enabled, frequency_index, cutoff, tone) in [(7, 8, 1000.0, 60.0), (9, 10, 2000.0, 16000.0)]
    {
        let mut effect = configured(
            48000.0,
            &[
                (enabled, 1.0),
                (frequency_index, cutoff),
                (14, 1.0),
                (4, 24.0),
                (5, 0.0),
            ],
        );
        let detector: Vec<f32> = (0..48000)
            .map(|n| (std::f64::consts::TAU * tone * n as f64 / 48000.0).cos() as f32 * 0.5)
            .collect();
        let mut audio = vec![0.1; 96000];
        effect.process_with_inputs(&mut audio, 2, &[sidechain(&detector, 1, true)]);
        let rms = (audio[48000..]
            .iter()
            .map(|sample| sample * sample)
            .sum::<f32>()
            / 48000.0)
            .sqrt();
        assert!(rms < 0.01, "filtered detector RMS {rms}");
        assert!(
            rms > 0.00001,
            "listen must contain actual filtered detector"
        );
    }
}

#[test]
fn reset_replays_silence_impulse_and_sweep_and_blocks_do_not_change_the_result() {
    let edits = [
        (6, 12.0),
        (7, 1.0),
        (8, 150.0),
        (11, 1.0),
        (12, 1.0),
        (13, 0.0),
    ];
    for rate in [44100.0, 48000.0, 96000.0] {
        let mut whole = configured(rate, &edits);
        let mut split = configured(rate, &edits);
        let mut input = vec![0.0; 16384];
        input[0] = 0.8;
        for (n, value) in input[1024..].iter_mut().enumerate() {
            *value = ((n * n) as f64 * 0.0001).sin() as f32 * 0.2;
        }
        let mut a = input.clone();
        let mut b = input.clone();
        whole.process(&mut a, 2);
        for chunk in b.chunks_mut(62) {
            split.process(chunk, 2);
        }
        assert_eq!(a, b);
        assert!(a.iter().all(|sample| sample.is_finite()));
        whole.reset();
        whole.process(&mut input, 2);
        assert_eq!(a, input);
        whole.reset();
        let mut silence = [0.0; 1024];
        whole.process(&mut silence, 2);
        assert_eq!(silence, [0.0; 1024]);
    }
}

#[test]
fn static_soft_knee_is_continuous_and_matches_settled_audio_with_range_and_parallel_mix() {
    let mut p: Vec<f32> = COMPRESSOR_PARAMS
        .iter()
        .map(|descriptor| descriptor.default)
        .collect();
    p[0] = -24.0;
    p[1] = 4.0;
    p[2] = 0.1;
    p[4] = 6.0;
    p[6] = 12.0;
    for input_db in [-40.0_f32, -30.0, -27.0, -24.0, -21.0, -18.0, -6.0] {
        let over = input_db + 24.0;
        let reduction = if over <= -6.0 {
            0.0
        } else if over >= 6.0 {
            over * 0.75
        } else {
            0.75 * (over + 6.0).powi(2) / 24.0
        };
        assert!((static_transfer_db(input_db, &p) - (input_db - reduction + 6.0)).abs() < 1e-5);
    }
    for edge in [-30.0_f32, -18.0] {
        let left = static_transfer_db(edge - 0.001, &p);
        let center = static_transfer_db(edge, &p);
        let right = static_transfer_db(edge + 0.001, &p);
        assert!((center - left - (right - center)).abs() < 1e-5);
    }
    for (mix, range_enabled, range) in [
        (0.0, 0.0, 60.0),
        (0.5, 1.0, 3.0),
        (1.0, 1.0, 6.0),
        (1.0, 0.0, 60.0),
    ] {
        p[5] = mix;
        p[15] = range_enabled;
        p[16] = range;
        let mut effect = CompressorEffect::from_params(48000.0, &p);
        let level = 10.0_f32.powf(-12.0 / 20.0);
        let mut audio = vec![level; 48000];
        effect.process(&mut audio, 1);
        assert!((db(audio[47999]) - static_transfer_db(-12.0, &p)).abs() < 0.002);
    }
    p[1] = 1.0;
    assert!((static_transfer_db(-12.0, &p) + 6.0).abs() < 1e-5);
}

#[test]
fn rms_uses_twenty_millisecond_mean_square_and_legacy_log_envelope_timing() {
    for rate in [44100.0_f32, 48000.0, 96000.0] {
        let mut effect = configured(rate, &[(0, -24.0), (1, 4.0), (2, 0.1), (4, 0.0), (11, 1.0)]);
        let mut audio = [0.5; 4096];
        effect.process(&mut audio, 1);
        let rms_memory = (-1.0 / (0.020 * rate as f64)).exp();
        let attack = (-1.0 / (0.0001 * rate as f64)).exp();
        let mut square = 0.0_f64;
        let mut envelope = -120.0;
        for (frame, actual) in audio.into_iter().enumerate() {
            square = 0.25 + rms_memory * (square - 0.25);
            let level = 20.0 * square.sqrt().max(1e-10).log10();
            envelope = level + attack * (envelope - level);
            let reduction = (envelope + 24.0).max(0.0) * 0.75;
            let expected = 0.5 * 10.0_f64.powf(-reduction / 20.0);
            assert!(
                (actual as f64 - expected).abs() < 0.00004,
                "rate {rate}, frame {frame}: actual {actual} expected {expected}"
            );
        }
    }
}

#[test]
fn adaptive_release_matches_the_documented_activity_rule_for_brief_and_sustained_input() {
    let rate = 48000.0_f64;
    let mut reductions = Vec::new();
    for loud_frames in [960, 48000] {
        let mut effect = configured(
            rate as f32,
            &[
                (0, -18.0),
                (1, 4.0),
                (2, 5.0),
                (3, 80.0),
                (4, 0.0),
                (12, 1.0),
            ],
        );
        let mut audio = vec![0.5; loud_frames];
        audio.extend(vec![0.01; 4800]);
        effect.process(&mut audio, 1);
        let mut envelope = -120.0_f64;
        let mut activity = 0.0_f64;
        for (frame, actual) in audio.iter().copied().enumerate() {
            let input = if frame < loud_frames { 0.5_f64 } else { 0.01 };
            let level = 20.0 * input.log10();
            let desired = (level + 18.0).max(0.0) * 0.75;
            let target = (desired / 12.0).clamp(0.0, 1.0);
            activity = target + (-1.0 / (0.2 * rate)).exp() * (activity - target);
            let release = (80.0 * (0.5 + 2.5 * activity)).clamp(5.0, 2000.0);
            let time = if level > envelope { 5.0 } else { release };
            envelope = level + (-1.0 / (time * 0.001 * rate)).exp() * (envelope - level);
            let expected = input * 10.0_f64.powf(-(envelope + 18.0).max(0.0) * 0.75 / 20.0);
            assert!(
                (actual as f64 - expected).abs() < 0.00015,
                "burst {loud_frames}, frame {frame}: {actual} expected {expected}"
            );
        }
        reductions.push(-db(audio[loud_frames + 960] / 0.01));
    }
    assert!(
        reductions[1] > reductions[0] + 2.0,
        "short/sustained reductions {reductions:?}"
    );
}

#[test]
fn dynamics_snapshot_reports_applied_wet_gain_independently_of_makeup_and_mix() {
    let mut wet = configured(48000.0, &[(0, -24.0), (1, 4.0), (2, 0.1), (4, 0.0)]);
    let mut dry = configured(
        48000.0,
        &[(0, -24.0), (1, 4.0), (2, 0.1), (4, 24.0), (5, 0.0)],
    );
    let mut a = [0.5, 0.125].repeat(4800);
    let mut b = a.clone();
    wet.process(&mut a, 2);
    dry.process(&mut b, 2);
    let wet_meter = wet.take_dynamics_meter().unwrap();
    let dry_meter = dry.take_dynamics_meter().unwrap();
    assert_eq!(wet_meter, dry_meter);
    for (channel, input) in [0.5_f32, 0.125].into_iter().enumerate() {
        let measured = -db(a[a.len() - 2 + channel] / input);
        assert!((wet_meter.current_reduction_db[channel] - measured).abs() < 1e-5);
        assert!(wet_meter.peak_reduction_db[channel] >= measured - 1e-5);
    }
    assert_eq!(wet_meter.detector_peak, [0.5, 0.125]);
    let drained = wet.take_dynamics_meter().unwrap();
    assert_eq!(drained.peak_reduction_db, [0.0; 2]);
    assert_eq!(drained.detector_peak, [0.0; 2]);
    assert_eq!(drained.current_reduction_db, wet_meter.current_reduction_db);
    assert_eq!(&b[b.len() - 2..], [0.5, 0.125]);
}

#[test]
fn mode_link_listen_range_and_filter_switches_crossfade_while_playing() {
    for (index, target) in [
        (7, 1.0),
        (9, 1.0),
        (11, 1.0),
        (12, 1.0),
        (13, 0.0),
        (14, 1.0),
        (15, 1.0),
    ] {
        let edits = [(0, -30.0), (1, 10.0), (2, 0.1), (4, 0.0), (16, 3.0)];
        let mut effect = configured(48000.0, &edits);
        let mut unchanged = configured(48000.0, &edits);
        let mut warmup = [0.8, 0.02].repeat(4800);
        let mut control = warmup.clone();
        effect.process(&mut warmup, 2);
        unchanged.process(&mut control, 2);
        effect.set_param(index, target);
        let mut a = [0.8, 0.02];
        let mut b = a;
        effect.process(&mut a, 2);
        unchanged.process(&mut b, 2);
        for channel in 0..2 {
            assert!(
                (a[channel] - b[channel]).abs() < 0.002,
                "index {index} channel {channel}: {} vs {}",
                a[channel],
                b[channel]
            );
        }
        let mut remainder = [0.8, 0.02].repeat(1024);
        effect.process(&mut remainder, 2);
        assert!(remainder.iter().all(|sample| sample.is_finite()));
    }
}
