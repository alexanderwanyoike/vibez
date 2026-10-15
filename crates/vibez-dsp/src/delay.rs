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

const MAX_HISTORY_BYTES: u64 = 64 * 1024 * 1024;
const MAX_SYNC_SECONDS: u64 = 18;
const INTERPOLATION_GUARDS: u64 = 2;

/// Stereo delay effect with feedback.
pub struct DelayEffect {
    time_ms: f32,
    feedback: f32,
    mix: f32,
    sample_rate: f32,
    configuration_valid: bool,
    buffer_l: Vec<f32>,
    buffer_r: Vec<f32>,
    write_pos: usize,
}

impl DelayEffect {
    pub fn new(sample_rate: f32) -> Self {
        let histories = Self::prepare_histories(sample_rate);
        let configuration_valid = histories.is_some();
        let (buffer_l, buffer_r) = histories.unwrap_or_default();
        Self {
            time_ms: 500.0,
            feedback: 0.5,
            mix: 0.3,
            sample_rate,
            configuration_valid,
            buffer_l,
            buffer_r,
            write_pos: 0,
        }
    }

    fn prepare_histories(sample_rate: f32) -> Option<(Vec<f32>, Vec<f32>)> {
        let frames = Self::requested_activation_rate(sample_rate)
            .filter(|rate| *rate >= 1000)
            .and_then(|rate| u64::from(rate).checked_mul(MAX_SYNC_SECONDS))
            .and_then(|frames| frames.checked_add(INTERPOLATION_GUARDS))?;
        let bytes = frames
            .checked_mul(2)?
            .checked_mul(std::mem::size_of::<f32>() as u64)?;
        if bytes > MAX_HISTORY_BYTES {
            return None;
        }
        let count = usize::try_from(frames).ok()?;
        let mut buffer_l = Vec::new();
        buffer_l.try_reserve_exact(count).ok()?;
        buffer_l.resize(count, 0.0);
        let mut buffer_r = Vec::new();
        buffer_r.try_reserve_exact(count).ok()?;
        buffer_r.resize(count, 0.0);
        Some((buffer_l, buffer_r))
    }

    fn requested_activation_rate(sample_rate: f32) -> Option<u32> {
        let rate = f64::from(sample_rate);
        if rate.is_finite() && rate.fract() == 0.0 && (0.0..=f64::from(u32::MAX)).contains(&rate) {
            Some(rate as u32)
        } else {
            None
        }
    }

    fn delay_samples(&self) -> f64 {
        let samples = f64::from(self.time_ms) * f64::from(self.sample_rate) / 1000.0;
        // Full-ring distance reads the prior lap before the current frame overwrites it.
        samples.max(1.0).min(self.buffer_l.len().max(1) as f64)
    }
}

impl AudioEffect for DelayEffect {
    fn activation_sample_rate(&self) -> Option<u32> {
        Self::requested_activation_rate(self.sample_rate)
    }

    fn processing_configuration_valid(&self) -> bool {
        self.configuration_valid
    }

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
        if channels == 0 {
            return;
        }
        if !self.configuration_valid {
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
        let delay = self.delay_samples();
        let whole = delay.floor() as usize;
        let fraction = delay.fract();
        let buf_len = self.buffer_l.len();
        let read = |history: &[f32], position: usize| {
            let recent = history[position];
            // Integer taps bypass blend arithmetic to retain the legacy sample algebra.
            if fraction == 0.0 {
                recent
            } else {
                let older = history[(position + buf_len - 1) % buf_len];
                (f64::from(recent) * (1.0 - fraction) + f64::from(older) * fraction) as f32
            }
        };

        for frame in 0..frames {
            let read_pos = (self.write_pos + buf_len - whole) % buf_len;
            let delayed_l = read(&self.buffer_l, read_pos);
            let delayed_r = read(&self.buffer_r, read_pos);

            // Left channel
            let dry_l = buffer[frame * ch];
            self.buffer_l[self.write_pos] = dry_l + delayed_l * self.feedback;
            buffer[frame * ch] = dry_l * (1.0 - self.mix) + delayed_l * self.mix;

            // Missing mono input is zero, but its feedback continues while unobserved.
            let dry_r = if ch >= 2 { buffer[frame * ch + 1] } else { 0.0 };
            self.buffer_r[self.write_pos] = dry_r + delayed_r * self.feedback;
            if ch >= 2 {
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
