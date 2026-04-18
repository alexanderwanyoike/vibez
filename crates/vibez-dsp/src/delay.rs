use vibez_core::effect::{EffectType, ParamDescriptor};

use crate::effect::AudioEffect;

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
    sample_rate: f32,
    buffer_l: Vec<f32>,
    buffer_r: Vec<f32>,
    write_pos: usize,
}

impl DelayEffect {
    pub fn new(sample_rate: f32) -> Self {
        // Max delay = 2 seconds
        let max_samples = (sample_rate * 2.0) as usize;
        Self {
            time_ms: 500.0,
            feedback: 0.5,
            mix: 0.3,
            sample_rate,
            buffer_l: vec![0.0; max_samples],
            buffer_r: vec![0.0; max_samples],
            write_pos: 0,
        }
    }

    fn delay_samples(&self) -> usize {
        let samples = (self.time_ms * 0.001 * self.sample_rate) as usize;
        samples.min(self.buffer_l.len() - 1).max(1)
    }
}

impl AudioEffect for DelayEffect {
    fn effect_type(&self) -> EffectType {
        EffectType::Delay
    }

    fn param_descriptors(&self) -> &'static [ParamDescriptor] {
        DELAY_PARAMS
    }

    fn set_param(&mut self, index: usize, value: f32) -> bool {
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
        let ch = channels.clamp(1, 2);
        let frames = buffer.len() / ch;
        let delay = self.delay_samples();
        let buf_len = self.buffer_l.len();

        for frame in 0..frames {
            let read_pos = (self.write_pos + buf_len - delay) % buf_len;

            // Left channel
            let dry_l = buffer[frame * ch];
            let delayed_l = self.buffer_l[read_pos];
            self.buffer_l[self.write_pos] = dry_l + delayed_l * self.feedback;
            buffer[frame * ch] = dry_l * (1.0 - self.mix) + delayed_l * self.mix;

            // Right channel (or duplicate mono)
            if ch >= 2 {
                let dry_r = buffer[frame * ch + 1];
                let delayed_r = self.buffer_r[read_pos];
                self.buffer_r[self.write_pos] = dry_r + delayed_r * self.feedback;
                buffer[frame * ch + 1] = dry_r * (1.0 - self.mix) + delayed_r * self.mix;
            }

            self.write_pos = (self.write_pos + 1) % buf_len;
        }
    }

    fn reset(&mut self) {
        self.buffer_l.fill(0.0);
        self.buffer_r.fill(0.0);
        self.write_pos = 0;
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
        assert_eq!(fx.write_pos, 0);
        assert!(fx.buffer_l.iter().all(|&s| s == 0.0));
    }
}
