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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn depth_zero_is_identity() {
        let mut fx = AutoPanEffect::new(44_100.0);
        fx.set_param(1, 0.0);
        let orig = vec![0.5_f32; 64];
        let mut buf = orig.clone();
        fx.process(&mut buf, 2);
        for (a, b) in buf.iter().zip(orig.iter()) {
            assert!((a - b).abs() < 1e-5);
        }
    }

    #[test]
    fn creates_stereo_motion() {
        let mut fx = AutoPanEffect::new(44_100.0);
        fx.set_param(0, 1.0);
        fx.set_param(1, 1.0);
        let mut buf = vec![0.5_f32; 44_100 * 2];
        fx.process(&mut buf, 2);
        let mut any_diff = false;
        for frame in 0..44_100 {
            if (buf[frame * 2] - buf[frame * 2 + 1]).abs() > 0.01 {
                any_diff = true;
                break;
            }
        }
        assert!(any_diff);
    }
}
