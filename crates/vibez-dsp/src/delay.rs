//! Admits Delay parameter edits and processes finite linear feedback echoes.

use vibez_core::effect::{EffectType, ParamDescriptor};

use crate::effect::AudioEffect;

#[path = "delay_history.rs"]
mod history;
#[path = "delay_time.rs"]
mod time;
use history::DelayHistory;
use time::DelayTime;

pub mod param {
    pub const TIME_MS: usize = 0;
    pub const FEEDBACK: usize = 1;
    pub const MIX: usize = 2;
    pub const SYNC: usize = 3;
    pub const LEFT_DIVISION: usize = 4;
    pub const LEFT_MODIFIER: usize = 5;
    pub const ROUTING: usize = 6;
    pub const LINK: usize = 7;
    pub const RIGHT_TIME_MS: usize = 8;
    pub const RIGHT_DIVISION: usize = 9;
    pub const RIGHT_MODIFIER: usize = 10;
    pub const FEEDBACK_HP_ENABLED: usize = 11;
    pub const FEEDBACK_HP_HZ: usize = 12;
    pub const FEEDBACK_LP_ENABLED: usize = 13;
    pub const FEEDBACK_LP_HZ: usize = 14;
    pub const DUCK: usize = 15;
    pub const TIME_CHANGE: usize = 16;
    pub const COUNT: usize = 17;
}

pub static DELAY_PARAMS: &[ParamDescriptor] = &[
    ParamDescriptor {
        name: "Time",
        min: 1.0,
        max: 2000.0,
        default: 500.0,
        unit: "ms",
    },
    ParamDescriptor {
        name: "Feedback",
        min: 0.0,
        max: 0.95,
        default: 0.5,
        unit: "",
    },
    ParamDescriptor {
        name: "Mix",
        min: 0.0,
        max: 1.0,
        default: 0.3,
        unit: "",
    },
    ParamDescriptor {
        name: "Sync",
        min: 0.0,
        max: 1.0,
        default: 0.0,
        unit: "",
    },
    ParamDescriptor {
        name: "Left Division",
        min: 0.0,
        max: 6.0,
        default: 4.0,
        unit: "",
    },
    ParamDescriptor {
        name: "Left Modifier",
        min: 0.0,
        max: 2.0,
        default: 0.0,
        unit: "",
    },
    ParamDescriptor {
        name: "Routing",
        min: 0.0,
        max: 1.0,
        default: 0.0,
        unit: "",
    },
    ParamDescriptor {
        name: "L/R Link",
        min: 0.0,
        max: 1.0,
        default: 1.0,
        unit: "",
    },
    ParamDescriptor {
        name: "Right Free Time",
        min: 1.0,
        max: 2000.0,
        default: 500.0,
        unit: "ms",
    },
    ParamDescriptor {
        name: "Right Division",
        min: 0.0,
        max: 6.0,
        default: 4.0,
        unit: "",
    },
    ParamDescriptor {
        name: "Right Modifier",
        min: 0.0,
        max: 2.0,
        default: 0.0,
        unit: "",
    },
    ParamDescriptor {
        name: "Feedback HP Enabled",
        min: 0.0,
        max: 1.0,
        default: 0.0,
        unit: "",
    },
    ParamDescriptor {
        name: "Feedback HP Frequency",
        min: 20.0,
        max: 1000.0,
        default: 120.0,
        unit: "Hz",
    },
    ParamDescriptor {
        name: "Feedback LP Enabled",
        min: 0.0,
        max: 1.0,
        default: 0.0,
        unit: "",
    },
    ParamDescriptor {
        name: "Feedback LP Frequency",
        min: 1000.0,
        max: 20000.0,
        default: 8000.0,
        unit: "Hz",
    },
    ParamDescriptor {
        name: "Duck",
        min: 0.0,
        max: 1.0,
        default: 0.0,
        unit: "",
    },
    ParamDescriptor {
        name: "Time Change",
        min: 0.0,
        max: 1.0,
        default: 0.0,
        unit: "",
    },
];

/// Stereo delay effect with feedback.
pub struct DelayEffect {
    targets: [f32; param::COUNT],
    history: DelayHistory,
    timing: DelayTime,
}

impl DelayEffect {
    pub fn new(sample_rate: f32) -> Self {
        let history = DelayHistory::new(sample_rate);
        Self {
            targets: std::array::from_fn(|index| DELAY_PARAMS[index].default),
            history,
            timing: DelayTime::new(),
        }
    }

    fn finite_or_zero(sample: f32) -> f32 {
        if sample.is_finite() {
            sample
        } else {
            0.0
        }
    }
}

impl AudioEffect for DelayEffect {
    fn set_audio_context(&mut self, context: vibez_core::audio_context::DeviceAudioContext) {
        self.timing.set_bpm(context.bpm);
    }

    fn activation_sample_rate(&self) -> Option<u32> {
        self.history.activation_sample_rate()
    }

    fn processing_configuration_valid(&self) -> bool {
        self.history.is_valid()
    }

    fn effect_type(&self) -> EffectType {
        EffectType::Delay
    }

    fn param_descriptors(&self) -> &'static [ParamDescriptor] {
        DELAY_PARAMS
    }

    fn set_param(&mut self, index: usize, value: f32) -> bool {
        if !value.is_finite() {
            return false;
        }
        let Some(descriptor) = DELAY_PARAMS.get(index) else {
            return false;
        };
        let target = match index {
            param::SYNC | param::LINK | param::FEEDBACK_HP_ENABLED | param::FEEDBACK_LP_ENABLED => {
                if value >= 0.5 {
                    1.0
                } else {
                    0.0
                }
            }
            param::LEFT_DIVISION
            | param::LEFT_MODIFIER
            | param::ROUTING
            | param::RIGHT_DIVISION
            | param::RIGHT_MODIFIER
            | param::TIME_CHANGE => {
                // Ordinals have one zero representation for stored/preset matching.
                value.round().clamp(descriptor.min, descriptor.max) as u32 as f32
            }
            _ => value.clamp(descriptor.min, descriptor.max),
        };
        self.targets[index] = target;
        true
    }

    fn get_param(&self, index: usize) -> f32 {
        self.targets.get(index).copied().unwrap_or(0.0)
    }

    fn process(&mut self, buffer: &mut [f32], channels: usize) {
        if channels == 0 {
            return;
        }
        if !self.history.is_valid() {
            for frame in buffer.chunks_exact_mut(channels) {
                for sample in frame.iter_mut().take(2) {
                    if !sample.is_finite() {
                        *sample = 0.0;
                    }
                }
            }
            return;
        }

        let ch = channels;
        let frames = buffer.len() / ch;
        let distances = self.timing.distances(&self.targets, self.history.sample_rate());
        let [left_tap, right_tap] = distances.map(|distance| self.history.tap(distance));
        let feedback = self.targets[param::FEEDBACK];
        let mix = self.targets[param::MIX];

        for frame in 0..frames {
            let (delayed_l, delayed_r) = self.history.read(left_tap, right_tap);

            // Left channel
            let dry_l = Self::finite_or_zero(buffer[frame * ch]);
            let write_l = Self::finite_or_zero(dry_l + delayed_l * feedback);
            buffer[frame * ch] = Self::finite_or_zero(dry_l * (1.0 - mix) + delayed_l * mix);

            // Missing mono input is zero, but its feedback continues while unobserved.
            let dry_r = if ch >= 2 {
                Self::finite_or_zero(buffer[frame * ch + 1])
            } else {
                0.0
            };
            let write_r = Self::finite_or_zero(dry_r + delayed_r * feedback);
            if ch >= 2 {
                buffer[frame * ch + 1] =
                    Self::finite_or_zero(dry_r * (1.0 - mix) + delayed_r * mix);
            }

            self.history.write_and_advance(write_l, write_r);
        }
    }

    fn reset(&mut self) {
        self.history.reset();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delay_dry_only_when_mix_zero() {
        let mut fx = DelayEffect::new(44100.0);
        fx.set_param(2, 0.0); // mix = 0
        let mut buf = vec![1.0, 0.5, 0.0, 0.0];
        let orig = buf.clone();
        fx.process(&mut buf, 2);
        for (a, b) in buf.iter().zip(orig.iter()) {
            assert!((a - b).abs() < 1e-6);
        }
    }

    #[test]
    fn delay_produces_echo() {
        let mut fx = DelayEffect::new(1000.0); // 1000 Hz sample rate for easy math
        fx.set_param(0, 10.0); // 10ms = 10 samples at 1kHz
        fx.set_param(1, 0.0); // no feedback
        fx.set_param(2, 1.0); // wet only

        // Send impulse then silence
        let mut buf = vec![0.0; 100];
        buf[0] = 1.0; // impulse on L
        buf[1] = 1.0; // impulse on R
        fx.process(&mut buf, 2);

        // At frame 10 (sample indices 20,21) we should see the echo
        assert!((buf[20] - 1.0).abs() < 1e-6);
        assert!((buf[21] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn delay_feedback_produces_repeats() {
        let mut fx = DelayEffect::new(1000.0);
        fx.set_param(0, 5.0); // 5ms = 5 samples
        fx.set_param(1, 0.5); // feedback
        fx.set_param(2, 1.0); // wet only

        let mut buf = vec![0.0; 60]; // 30 frames stereo
        buf[0] = 1.0;
        buf[1] = 1.0;
        fx.process(&mut buf, 2);

        // First echo at frame 5
        assert!((buf[10] - 1.0).abs() < 1e-6);
        // Second echo at frame 10 (with 0.5 feedback)
        assert!((buf[20] - 0.5).abs() < 1e-6);
    }

    #[test]
    fn delay_param_access() {
        let mut fx = DelayEffect::new(44100.0);
        assert!((fx.get_param(0) - 500.0).abs() < 1e-3);
        assert!((fx.get_param(1) - 0.5).abs() < 1e-3);
        assert!((fx.get_param(2) - 0.3).abs() < 1e-3);
        fx.set_param(0, 100.0);
        assert!((fx.get_param(0) - 100.0).abs() < 1e-3);
        // Appended indices 3..16 are valid; the original three values stay unchanged.
        assert!(!fx.set_param(17, 1.0));
    }

    #[test]
    fn delay_reset() {
        let mut fx = DelayEffect::new(44100.0);
        let mut buf = vec![1.0; 100];
        fx.process(&mut buf, 2);
        fx.reset();
        assert_eq!(fx.history.write_position(), 0);
        assert!(fx.history.left_samples().iter().all(|&s| s == 0.0));
    }
}
