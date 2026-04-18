use vibez_core::effect::{EffectType, ParamDescriptor};

use crate::effect::AudioEffect;

static BITCRUSH_PARAMS: &[ParamDescriptor] = &[
    ParamDescriptor {
        name: "Bits",
        min: 2.0,
        max: 16.0,
        default: 10.0,
        unit: "bit",
    },
    ParamDescriptor {
        name: "SR Reduce",
        min: 1.0,
        max: 32.0,
        default: 1.0,
        unit: "x",
    },
    ParamDescriptor {
        name: "Mix",
        min: 0.0,
        max: 1.0,
        default: 1.0,
        unit: "",
    },
];

/// Lo-fi effect: reduces bit depth and sample rate (sample-and-hold).
pub struct BitcrushEffect {
    bits: f32,
    sr_divider: f32,
    mix: f32,
    /// Sample-and-hold state, per channel.
    hold: [f32; 2],
    /// Frame counter for SR reduce, per channel.
    counter: [u32; 2],
}

impl BitcrushEffect {
    pub fn new() -> Self {
        Self {
            bits: 10.0,
            sr_divider: 1.0,
            mix: 1.0,
            hold: [0.0; 2],
            counter: [0; 2],
        }
    }
}

impl Default for BitcrushEffect {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioEffect for BitcrushEffect {
    fn effect_type(&self) -> EffectType {
        EffectType::Bitcrush
    }

    fn param_descriptors(&self) -> &'static [ParamDescriptor] {
        BITCRUSH_PARAMS
    }

    fn set_param(&mut self, index: usize, value: f32) -> bool {
        match index {
            0 => {
                self.bits = value.clamp(2.0, 16.0);
                true
            }
            1 => {
                self.sr_divider = value.clamp(1.0, 32.0);
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
            0 => self.bits,
            1 => self.sr_divider,
            2 => self.mix,
            _ => 0.0,
        }
    }

    fn process(&mut self, buffer: &mut [f32], channels: usize) {
        let ch = channels.clamp(1, 2);
        let frames = buffer.len() / ch;
        let levels = 2u32.pow(self.bits as u32) as f32;
        let step = 2.0 / levels;
        let divider = self.sr_divider.round().max(1.0) as u32;

        for frame in 0..frames {
            for c in 0..ch {
                let idx = frame * ch + c;
                let dry = buffer[idx];

                if self.counter[c] == 0 {
                    // Quantize to `levels` steps in [-1, 1].
                    let q = (dry / step).round() * step;
                    self.hold[c] = q.clamp(-1.0, 1.0);
                }
                self.counter[c] = (self.counter[c] + 1) % divider;

                let wet = self.hold[c];
                buffer[idx] = dry * (1.0 - self.mix) + wet * self.mix;
            }
        }
    }

    fn reset(&mut self) {
        self.hold = [0.0; 2];
        self.counter = [0; 2];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passthrough_at_16_bit_and_no_sr_reduce() {
        let mut fx = BitcrushEffect::new();
        fx.set_param(0, 16.0);
        fx.set_param(1, 1.0);
        fx.set_param(2, 1.0);
        let orig: Vec<f32> = (0..32).map(|i| (i as f32 / 32.0) - 0.5).collect();
        let mut buf = orig.clone();
        fx.process(&mut buf, 2);
        for (a, b) in buf.iter().zip(orig.iter()) {
            assert!((a - b).abs() < 0.02, "quantized {a} vs {b}");
        }
    }

    #[test]
    fn heavy_bit_reduction_quantizes() {
        let mut fx = BitcrushEffect::new();
        fx.set_param(0, 2.0);
        fx.set_param(2, 1.0);
        let mut buf: Vec<f32> = (0..64).map(|i| i as f32 / 64.0 - 0.5).collect();
        let levels = 4.0_f32;
        let step = 2.0 / levels;
        fx.process(&mut buf, 2);
        // After 2-bit crush, all samples align to a 4-step grid in [-1, 1].
        for s in &buf {
            let q = (s / step).round() * step;
            assert!((s - q).abs() < 1e-4, "{s} not on grid {step}");
        }
    }
}
