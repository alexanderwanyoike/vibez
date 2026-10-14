//! Independent bilinear responses and stage ordering specify the common host-rate Post LP.

use vibez_core::effect::EffectType;
use vibez_dsp::{effect::AudioEffect, factory::create_effect_with_params};

#[derive(Default)]
struct ReferenceFilter {
    history: [[f64; 4]; 2],
}

impl ReferenceFilter {
    fn process(&mut self, input: f32, channel: usize, rate: f32, cutoff: f32) -> f32 {
        let cutoff = f64::from(cutoff.min(rate * 0.49));
        let k = (std::f64::consts::PI * cutoff / f64::from(rate)).tan();
        let divisor = 1.0 + std::f64::consts::SQRT_2 * k + k * k;
        let b = [k * k, 2.0 * k * k, k * k].map(|value| value / divisor);
        let a = [
            2.0 * (k * k - 1.0) / divisor,
            (1.0 - std::f64::consts::SQRT_2 * k + k * k) / divisor,
        ];
        let [x1, x2, y1, y2] = self.history[channel];
        let output = b[0] * f64::from(input) + b[1] * x1 + b[2] * x2 - a[0] * y1 - a[1] * y2;
        self.history[channel] = [f64::from(input), x1, output, y1];
        output as f32
    }
}

fn build(rate: f32, divider: f32, mix: f32, enabled: f32, cutoff: f32) -> Box<dyn AudioEffect> {
    create_effect_with_params(
        EffectType::Bitcrush,
        rate,
        &[6.0, divider, mix, 0.0, 0.0, 0.0, 0.0, enabled, cutoff],
    )
}

fn close(actual: f32, expected: f32, context: &str, frame: usize) {
    assert!(actual.is_finite());
    assert!((actual - expected).abs() < 3e-6,
        "{context}: frame={frame}, actual={actual}, expected={expected}");
}

#[test]
fn impulse_matches_butterworth_response_and_actual_cap_even_below_twenty_hertz() {
    for rate in [2.0, 10.0, 40.0, 8_000.0, 44_100.0, 48_000.0, 96_000.0] {
        for cutoff in [20.0, 700.0, 20_000.0] {
            let mut effect = build(rate, 1.0, 1.0, 1.0, cutoff);
            assert_eq!(effect.get_param(8), cutoff);
            assert_eq!(effect.activation_sample_rate(), Some(rate as u32));
            assert_eq!(effect.latency_samples(), 0);
            let mut actual = vec![0.0; 4096 * 2];
            actual[0] = 0.5;
            actual[1] = -0.25;
            effect.process(&mut actual, 2);
            let mut reference = ReferenceFilter::default();
            for frame in 0..4096 {
                for channel in 0..2 {
                    let input = if frame == 0 { [0.5, -0.25][channel] } else { 0.0 };
                    let expected = reference.process(input, channel, rate, cutoff);
                    close(actual[frame * 2 + channel], expected,
                        &format!("rate={rate}, stored_cutoff={cutoff}, channel={channel}"), frame);
                }
            }
            assert_eq!(effect.get_param(8), cutoff);
        }
    }
}

fn input(frame: usize, channel: usize) -> f32 {
    let left = ((frame % 97) as f32 - 48.0) / 32.0;
    if channel == 0 { left } else { 0.125 - left * 0.5 }
}

fn quantize(value: f32) -> f32 {
    (value * 32.0).round().clamp(-32.0, 32.0) / 32.0
}

#[test]
fn bypass_keeps_the_filter_warm_and_filtering_stays_before_the_untouched_dry_mix() {
    for rate in [2.0, 10.0, 40.0, 44_100.0, 96_000.0] {
        for mix in [0.0, 0.37, 1.0] {
            let mut effect = build(rate, 3.0, mix, 0.0, 700.0);
            let mut reference = ReferenceFilter::default();
            let mut held = [0.0; 2];
            for frame in 0..256 {
                if frame == 128 {
                    assert!(effect.set_automated_param(7, 1.0));
                }
                let mut actual = [input(frame, 0), input(frame, 1)];
                let dry = actual;
                effect.process(&mut actual, 2);
                for channel in 0..2 {
                    if frame % 3 == 0 {
                        held[channel] = quantize(dry[channel]);
                    }
                    let filtered = reference.process(held[channel], channel, rate, 700.0);
                    let wet = if frame < 128 { held[channel] } else { filtered };
                    let expected = dry[channel] * (1.0 - mix) + wet * mix;
                    close(actual[channel], expected,
                        &format!("warm bypass rate={rate}, mix={mix}, channel={channel}"), frame);
                    if mix == 0.0 {
                        assert_eq!(actual[channel], dry[channel]);
                    }
                }
            }
        }
    }
}

fn edits(effect: &mut dyn AudioEffect, frame: usize) {
    match frame {
        64 => {
            assert!(effect.set_param(8, 1_000.0));
            assert!(effect.set_param(7, 1.0));
        }
        192 => { assert!(effect.set_automated_param(2, 0.37)); }
        256 => {
            assert!(effect.set_param(8, 1_000.0));
            assert!(effect.set_param(7, 1.0));
        }
        320 => { assert!(effect.set_automated_param(8, 4_000.0)); }
        640 => {
            assert!(effect.set_param(8, 1_000.0));
            assert!(effect.set_param(7, 0.0));
        }
        768 => { assert!(effect.set_automated_param(7, 1.0)); }
        800 => { assert!(effect.set_param(8, 1_000.0)); }
        _ => {}
    }
}

fn controls(frame: usize) -> (f32, f32, f32) {
    let cutoff = match frame {
        0..64 => 20_000.0,
        64..320 => 20_000.0 - 19_000.0 * (frame - 63) as f32 / 512.0,
        320..640 => 4_000.0,
        _ => 4_000.0 - 3_000.0 * (frame - 639).min(512) as f32 / 512.0,
    };
    let enabled = match frame {
        0..64 => 0.0,
        64..640 => (frame - 63).min(512) as f32 / 512.0,
        640..768 => 1.0 - (frame - 639) as f32 / 512.0,
        _ => 1.0,
    };
    (cutoff, enabled, if frame < 192 { 0.65 } else { 0.37 })
}

#[test]
fn cutoff_and_enable_ramp_on_source_frames_with_independent_exact_automation_and_reset() {
    let boundaries = [64, 192, 256, 320, 640, 768, 800, 1280];
    for rate in [44_100.0, 96_000.0] {
        for channels in [1, 2] {
            let mut baseline = None;
            for partitions in [&[1280][..], &[1, 7, 31, 113, 5][..]] {
                let mut effect = build(rate, 3.0, 0.65, 0.0, 20_000.0);
                let mut actual: Vec<_> = (0..1280)
                    .flat_map(|frame| (0..channels).map(move |channel| input(frame, channel))).collect();
                let mut frame = 0;
                let mut callback = 0;
                while frame < 1280 {
                    edits(effect.as_mut(), frame);
                    let next_edit = *boundaries.iter().find(|&&next| next > frame).unwrap();
                    let end = (frame + partitions[callback % partitions.len()]).min(next_edit);
                    effect.process(&mut actual[frame * channels..end * channels], channels);
                    frame = end;
                    callback += 1;
                }
                let mut reference = ReferenceFilter::default();
                let mut held = [0.0; 2];
                for frame in 0..1280 {
                    let (cutoff, enabled, mix) = controls(frame);
                    for channel in 0..channels {
                        let dry = input(frame, channel);
                        if frame % 3 == 0 {
                            held[channel] = quantize(dry);
                        }
                        let filtered = reference.process(held[channel], channel, rate, cutoff);
                        let wet = held[channel] * (1.0 - enabled) + filtered * enabled;
                        let expected = dry * (1.0 - mix) + wet * mix;
                        close(actual[frame * channels + channel], expected,
                            &format!("ramps rate={rate}, channels={channels}"), frame);
                    }
                }
                if let Some(baseline) = &baseline {
                    assert_eq!(&actual, baseline, "callback partition invariance");
                } else {
                    baseline = Some(actual);
                }
                effect.reset();
                let mut reset: Vec<_> = (0..64)
                    .flat_map(|frame| (0..channels).map(move |channel| input(frame, channel))).collect();
                effect.process(&mut reset, channels);
                let mut reference = ReferenceFilter::default();
                let mut held = [0.0; 2];
                for frame in 0..64 {
                    for channel in 0..channels {
                        let dry = input(frame, channel);
                        if frame % 3 == 0 { held[channel] = quantize(dry); }
                        let wet = reference.process(held[channel], channel, rate, 1_000.0);
                        let expected = dry * (1.0 - 0.37) + wet * 0.37;
                        close(reset[frame * channels + channel], expected, "reset to current targets", frame);
                    }
                }
            }
        }
    }
}
