//! Admits Delay parameter edits and processes finite linear feedback echoes.

use vibez_core::effect::{EffectType, ParamDescriptor};

use crate::effect::AudioEffect;

#[path = "delay_history.rs"]
mod history;
use history::DelayHistory;

static DELAY_PARAMS: &[ParamDescriptor] = &[
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
];

/// Stereo delay effect with feedback.
pub struct DelayEffect {
    time_ms: f32,
    feedback: f32,
    mix: f32,
    history: DelayHistory,
}

impl DelayEffect {
    pub fn new(sample_rate: f32) -> Self {
        let history = DelayHistory::new(sample_rate);
        Self {
            time_ms: 500.0,
            feedback: 0.5,
            mix: 0.3,
            history,
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
        match index {
            0 => {
                self.time_ms = value.clamp(1.0, 2000.0);
                true
            }
            1 => {
                self.feedback = value.clamp(0.0, 0.95);
                true
            }
            2 => {
                self.mix = value.clamp(0.0, 1.0);
                true
            }
            _ => false,
        }
    }

    fn get_param(&self, index: usize) -> f32 {
        match index {
            0 => self.time_ms,
            1 => self.feedback,
            2 => self.mix,
            _ => 0.0,
        }
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
        let tap = self.history.tap(self.time_ms);

        for frame in 0..frames {
            let (delayed_l, delayed_r) = self.history.read(tap);

            // Left channel
            let dry_l = Self::finite_or_zero(buffer[frame * ch]);
            let write_l = Self::finite_or_zero(dry_l + delayed_l * self.feedback);
            buffer[frame * ch] = Self::finite_or_zero(dry_l * (1.0 - self.mix) + delayed_l * self.mix);

            // Missing mono input is zero, but its feedback continues while unobserved.
            let dry_r = if ch >= 2 {
                Self::finite_or_zero(buffer[frame * ch + 1])
            } else {
                0.0
            };
            let write_r = Self::finite_or_zero(dry_r + delayed_r * self.feedback);
            if ch >= 2 {
                buffer[frame * ch + 1] =
                    Self::finite_or_zero(dry_r * (1.0 - self.mix) + delayed_r * self.mix);
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
        assert!(!fx.set_param(3, 1.0));
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
