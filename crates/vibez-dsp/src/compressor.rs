use vibez_core::effect::{EffectType, ParamDescriptor};

use crate::effect::AudioEffect;

static COMPRESSOR_PARAMS: &[ParamDescriptor] = &[
    ParamDescriptor {
        name: "Threshold",
        min: -60.0,
        max: 0.0,
        default: -18.0,
        unit: "dB",
    },
    ParamDescriptor {
        name: "Ratio",
        min: 1.0,
        max: 20.0,
        default: 4.0,
        unit: ":1",
    },
    ParamDescriptor {
        name: "Attack",
        min: 0.1,
        max: 100.0,
        default: 5.0,
        unit: "ms",
    },
    ParamDescriptor {
        name: "Release",
        min: 10.0,
        max: 500.0,
        default: 80.0,
        unit: "ms",
    },
    ParamDescriptor {
        name: "Makeup",
        min: 0.0,
        max: 24.0,
        default: 3.0,
        unit: "dB",
    },
    ParamDescriptor {
        name: "Mix",
        min: 0.0,
        max: 1.0,
        default: 1.0,
        unit: "",
    },
];

/// Single-channel-linked feedforward compressor with a log-domain envelope
/// detector. Designed for musical gluing — ratios and times are
/// forgiving, not surgical.
pub struct CompressorEffect {
    threshold_db: f32,
    ratio: f32,
    attack_ms: f32,
    release_ms: f32,
    makeup_db: f32,
    mix: f32,
    sample_rate: f32,
    env_db: f32,
}

impl CompressorEffect {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            threshold_db: -18.0,
            ratio: 4.0,
            attack_ms: 5.0,
            release_ms: 80.0,
            makeup_db: 3.0,
            mix: 1.0,
            sample_rate,
            env_db: -120.0,
        }
    }

    fn coef(time_ms: f32, sr: f32) -> f32 {
        let t = (time_ms * 0.001).max(1e-5);
        (-1.0 / (t * sr)).exp()
    }
}

impl AudioEffect for CompressorEffect {
    fn effect_type(&self) -> EffectType {
        EffectType::Compressor
    }

    fn param_descriptors(&self) -> &'static [ParamDescriptor] {
        COMPRESSOR_PARAMS
    }

    fn set_param(&mut self, index: usize, value: f32) -> bool {
        match index {
            0 => {
                self.threshold_db = value.clamp(-60.0, 0.0);
                true
            }
            1 => {
                self.ratio = value.clamp(1.0, 20.0);
                true
            }
            2 => {
                self.attack_ms = value.clamp(0.1, 100.0);
                true
            }
            3 => {
                self.release_ms = value.clamp(10.0, 500.0);
                true
            }
            4 => {
                self.makeup_db = value.clamp(0.0, 24.0);
                true
            }
            5 => {
                self.mix = value.clamp(0.0, 1.0);
                true
            }
            _ => false,
        }
    }

    fn get_param(&self, index: usize) -> f32 {
        match index {
            0 => self.threshold_db,
            1 => self.ratio,
            2 => self.attack_ms,
            3 => self.release_ms,
            4 => self.makeup_db,
            5 => self.mix,
            _ => 0.0,
        }
    }

    fn process(&mut self, buffer: &mut [f32], channels: usize) {
        let ch = channels.clamp(1, 2);
        let frames = buffer.len() / ch;
        let att = Self::coef(self.attack_ms, self.sample_rate);
        let rel = Self::coef(self.release_ms, self.sample_rate);
        let makeup_lin = 10.0_f32.powf(self.makeup_db / 20.0);
        let inv_ratio = 1.0 / self.ratio;

        for frame in 0..frames {
            // Stereo-linked peak detector.
            let mut peak = 0.0_f32;
            for c in 0..ch {
                peak = peak.max(buffer[frame * ch + c].abs());
            }
            let peak_db = 20.0 * (peak.max(1e-10)).log10();

            // One-pole envelope follower in dB.
            let coef = if peak_db > self.env_db { att } else { rel };
            self.env_db = peak_db + coef * (self.env_db - peak_db);

            // Gain computer.
            let over = (self.env_db - self.threshold_db).max(0.0);
            let gr_db = over * (inv_ratio - 1.0);
            let gain = 10.0_f32.powf(gr_db / 20.0) * makeup_lin;

            for c in 0..ch {
                let idx = frame * ch + c;
                let dry = buffer[idx];
                let wet = dry * gain;
                buffer[idx] = dry * (1.0 - self.mix) + wet * self.mix;
            }
        }
    }

    fn reset(&mut self) {
        self.env_db = -120.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reduces_loud_signal_gain() {
        let mut fx = CompressorEffect::new(44_100.0);
        fx.set_param(0, -20.0);
        fx.set_param(1, 10.0);
        fx.set_param(4, 0.0);
        fx.set_param(5, 1.0);

        let mut buf = vec![0.5_f32; 4_410];
        fx.process(&mut buf, 2);
        let peak_out = buf.iter().map(|s| s.abs()).fold(0.0_f32, f32::max);
        assert!(peak_out < 0.5, "expected compression, got peak {peak_out}");
    }

    #[test]
    fn leaves_quiet_signal_untouched() {
        let mut fx = CompressorEffect::new(44_100.0);
        fx.set_param(0, -12.0);
        fx.set_param(1, 8.0);
        fx.set_param(4, 0.0);
        fx.set_param(5, 1.0);

        let mut buf = vec![0.05_f32; 4_410];
        fx.process(&mut buf, 2);
        for s in &buf {
            assert!((s - 0.05).abs() < 0.01, "{s}");
        }
    }
}
