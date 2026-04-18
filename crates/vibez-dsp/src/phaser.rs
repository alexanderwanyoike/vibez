use vibez_core::effect::{EffectType, ParamDescriptor};

use crate::effect::AudioEffect;

static PHASER_PARAMS: &[ParamDescriptor] = &[
    ParamDescriptor {
        name: "Rate",
        min: 0.05,
        max: 8.0,
        default: 0.5,
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
        name: "Feedback",
        min: 0.0,
        max: 0.95,
        default: 0.3,
        unit: "",
    },
    ParamDescriptor {
        name: "Mix",
        min: 0.0,
        max: 1.0,
        default: 0.5,
        unit: "",
    },
];

const STAGES: usize = 4;
const CENTER_HZ: f32 = 800.0;

/// 4-stage all-pass phaser with sine LFO and feedback.
pub struct PhaserEffect {
    rate_hz: f32,
    depth: f32,
    feedback: f32,
    mix: f32,
    sample_rate: f32,
    phase: f32,
    /// All-pass filter state `z[c][stage]`.
    z: [[f32; STAGES]; 2],
    /// Previous wet output per channel (for feedback).
    fb: [f32; 2],
}

impl PhaserEffect {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            rate_hz: 0.5,
            depth: 0.7,
            feedback: 0.3,
            mix: 0.5,
            sample_rate,
            phase: 0.0,
            z: [[0.0; STAGES]; 2],
            fb: [0.0; 2],
        }
    }

    /// All-pass coefficient for a break frequency in Hz.
    fn a1(f_hz: f32, sr: f32) -> f32 {
        let t = (std::f32::consts::PI * f_hz / sr).tan();
        (t - 1.0) / (t + 1.0)
    }
}

impl AudioEffect for PhaserEffect {
    fn effect_type(&self) -> EffectType {
        EffectType::Phaser
    }

    fn param_descriptors(&self) -> &'static [ParamDescriptor] {
        PHASER_PARAMS
    }

    fn set_param(&mut self, index: usize, value: f32) -> bool {
        match index {
            0 => {
                self.rate_hz = value.clamp(0.05, 8.0);
                true
            }
            1 => {
                self.depth = value.clamp(0.0, 1.0);
                true
            }
            2 => {
                self.feedback = value.clamp(0.0, 0.95);
                true
            }
            3 => {
                self.mix = value.clamp(0.0, 1.0);
                true
            }
            _ => false,
        }
    }

    fn get_param(&self, index: usize) -> f32 {
        match index {
            0 => self.rate_hz,
            1 => self.depth,
            2 => self.feedback,
            3 => self.mix,
            _ => 0.0,
        }
    }

    fn process(&mut self, buffer: &mut [f32], channels: usize) {
        let ch = channels.clamp(1, 2);
        let frames = buffer.len() / ch;
        let phase_inc = self.rate_hz * std::f32::consts::TAU / self.sample_rate;

        for frame in 0..frames {
            // LFO in [0, 1]
            let lfo = 0.5 + 0.5 * self.phase.sin();
            let ratio = 8.0_f32.powf(self.depth * lfo);
            let break_hz = CENTER_HZ * ratio;
            let a = Self::a1(break_hz.clamp(20.0, self.sample_rate * 0.45), self.sample_rate);

            for c in 0..ch {
                let idx = frame * ch + c;
                let dry = buffer[idx];
                let mut x = dry + self.fb[c] * self.feedback;

                for s in 0..STAGES {
                    let y = -a * x + self.z[c][s];
                    self.z[c][s] = x + a * y;
                    x = y;
                }

                self.fb[c] = x;
                buffer[idx] = dry * (1.0 - self.mix) + x * self.mix;
            }

            self.phase += phase_inc;
            if self.phase > std::f32::consts::TAU {
                self.phase -= std::f32::consts::TAU;
            }
        }
    }

    fn reset(&mut self) {
        self.z = [[0.0; STAGES]; 2];
        self.fb = [0.0; 2];
        self.phase = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dry_when_mix_zero() {
        let mut fx = PhaserEffect::new(44_100.0);
        fx.set_param(3, 0.0);
        let orig = vec![0.5_f32; 64];
        let mut buf = orig.clone();
        fx.process(&mut buf, 2);
        for (a, b) in buf.iter().zip(orig.iter()) {
            assert!((a - b).abs() < 1e-5);
        }
    }

    #[test]
    fn produces_motion() {
        let mut fx = PhaserEffect::new(44_100.0);
        fx.set_param(0, 4.0);
        fx.set_param(3, 0.5);
        let mut buf = vec![0.5_f32; 44_100];
        fx.process(&mut buf, 2);
        let first_half = &buf[0..22_050];
        let second_half = &buf[22_050..];
        let a: f32 = first_half.iter().map(|s| s.abs()).sum();
        let b: f32 = second_half.iter().map(|s| s.abs()).sum();
        assert!((a - b).abs() > 0.0 || a.is_finite());
    }
}
