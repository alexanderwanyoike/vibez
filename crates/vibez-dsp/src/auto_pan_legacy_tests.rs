//! The Auto-Pan before its new controls, kept to prove three-value projects
//! still sound the same.

mod original {
    use vibez_core::effect::{EffectType, ParamDescriptor};

    use crate::effect::AudioEffect;

    static AUTO_PAN_PARAMS: &[ParamDescriptor] = &[
        ParamDescriptor {
            name: "Rate",
            min: 0.05,
            max: 16.0,
            default: 2.0,
            unit: "Hz",
        },
        ParamDescriptor {
            name: "Depth",
            min: 0.0,
            max: 1.0,
            default: 0.7,
            unit: "",
        },
        ParamDescriptor {
            name: "Shape",
            min: 0.0,
            max: 1.0,
            default: 0.0,
            unit: "",
        },
    ];

    /// Stereo auto-pan driven by a sine LFO. On mono signals it falls back to
    /// amplitude modulation (tremolo).
    pub struct AutoPanEffect {
        rate_hz: f32,
        depth: f32,
        /// 0 = sine, 1 = square-ish (hard-edged pan).
        shape: f32,
        sample_rate: f32,
        phase: f32,
    }

    impl AutoPanEffect {
        pub fn new(sample_rate: f32) -> Self {
            Self {
                rate_hz: 2.0,
                depth: 0.7,
                shape: 0.0,
                sample_rate,
                phase: 0.0,
            }
        }
    }

    impl AudioEffect for AutoPanEffect {
        fn activation_sample_rate(&self) -> Option<u32> {
            Some(self.sample_rate as u32)
        }

        fn effect_type(&self) -> EffectType {
            EffectType::AutoPan
        }

        fn param_descriptors(&self) -> &'static [ParamDescriptor] {
            AUTO_PAN_PARAMS
        }

        fn set_param(&mut self, index: usize, value: f32) -> bool {
            match index {
                0 => {
                    self.rate_hz = value.clamp(0.05, 16.0);
                    true
                }
                1 => {
                    self.depth = value.clamp(0.0, 1.0);
                    true
                }
                2 => {
                    self.shape = value.clamp(0.0, 1.0);
                    true
                }
                _ => false,
            }
        }

        fn get_param(&self, index: usize) -> f32 {
            match index {
                0 => self.rate_hz,
                1 => self.depth,
                2 => self.shape,
                _ => 0.0,
            }
        }

        fn process(&mut self, buffer: &mut [f32], channels: usize) {
            let ch = channels.clamp(1, 2);
            let frames = buffer.len() / ch;
            let phase_inc = self.rate_hz * std::f32::consts::TAU / self.sample_rate;

            for frame in 0..frames {
                let lfo_sine = self.phase.sin();
                let lfo_square = if self.phase.sin() >= 0.0 { 1.0 } else { -1.0 };
                let lfo = lfo_sine * (1.0 - self.shape) + lfo_square * self.shape;
                let lfo = lfo * self.depth;

                if ch >= 2 {
                    let l_gain = (1.0 - lfo).clamp(0.0, 2.0);
                    let r_gain = (1.0 + lfo).clamp(0.0, 2.0);
                    buffer[frame * ch] *= l_gain;
                    buffer[frame * ch + 1] *= r_gain;
                } else {
                    let gain = 1.0 - lfo.abs();
                    buffer[frame * ch] *= gain;
                }

                self.phase += phase_inc;
                if self.phase > std::f32::consts::TAU {
                    self.phase -= std::f32::consts::TAU;
                }
            }
        }

        fn reset(&mut self) {
            self.phase = 0.0;
        }
    }
}

use crate::effect::AudioEffect;

fn signal(frames: usize) -> Vec<f32> {
    let mut seed = 0x1234_5678_u32;
    (0..frames * 2)
        .map(|_| {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            ((seed >> 9) as f32 / (1u32 << 23) as f32 * 2.0 - 1.0) * 0.5
        })
        .collect()
}

fn compare(rate: f32, controls: [f32; 3], channels: usize) {
    let mut before = original::AutoPanEffect::new(rate);
    let mut after = super::AutoPanEffect::new(rate);
    for (index, value) in controls.into_iter().enumerate() {
        before.set_param(index, value);
        after.set_param(index, value);
    }
    let input = signal(20_000);
    let (mut old, mut new) = (input.clone(), input);
    for (old, new) in old.chunks_mut(256).zip(new.chunks_mut(256)) {
        before.process(old, channels);
        after.process(new, channels);
    }
    for (index, (a, b)) in old.iter().zip(&new).enumerate() {
        assert!(
            (a - b).abs() <= 1e-6,
            "{rate} {controls:?} {channels} ch sample {index}: {a} vs {b}"
        );
    }
}

#[test]
fn three_value_projects_sound_as_they_did() {
    for rate in [44_100.0, 48_000.0] {
        for controls in [[2.0, 0.7, 0.0], [8.0, 1.0, 1.0], [0.05, 0.3, 0.4]] {
            compare(rate, controls, 2);
            compare(rate, controls, 1);
        }
    }
}
