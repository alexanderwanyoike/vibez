//! Bitcrush quantization and source-frame sample-and-hold processing.

use vibez_core::effect::{EffectType, ParamDescriptor};

use crate::effect::AudioEffect;

pub mod param {
    pub const BITS: usize = 0;
    pub const SR_REDUCE: usize = 1;
    pub const MIX: usize = 2;
    pub const DRIVE_DB: usize = 3;
    pub const ROUNDING: usize = 4;
    pub const DITHER: usize = 5;
    pub const JITTER: usize = 6;
    pub const POST_LP_ENABLED: usize = 7;
    pub const POST_LP_HZ: usize = 8;
    pub const COUNT: usize = 9;
}

pub static BITCRUSH_PARAMS: &[ParamDescriptor] = &[
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
    ParamDescriptor {
        name: "Drive",
        min: 0.0,
        max: 24.0,
        default: 0.0,
        unit: "dB",
    },
    ParamDescriptor {
        name: "Rounding",
        min: 0.0,
        max: 2.0,
        default: 0.0,
        unit: "",
    },
    ParamDescriptor {
        name: "Dither",
        min: 0.0,
        max: 1.0,
        default: 0.0,
        unit: "",
    },
    ParamDescriptor {
        name: "Jitter",
        min: 0.0,
        max: 1.0,
        default: 0.0,
        unit: "",
    },
    ParamDescriptor {
        name: "Post LP Enabled",
        min: 0.0,
        max: 1.0,
        default: 0.0,
        unit: "",
    },
    ParamDescriptor {
        name: "Post LP Frequency",
        min: 20.0,
        max: 20000.0,
        default: 12000.0,
        unit: "Hz",
    },
];

/// Lo-fi effect: reduces bit depth and sample rate (sample-and-hold).
pub struct BitcrushEffect {
    params: [f32; param::COUNT],
    sample_rate: f32,
    /// Sample-and-hold state, per channel.
    hold: [f32; 2],
    /// Frame counter for SR reduce, per channel.
    counter: [u32; 2],
}

impl BitcrushEffect {
    pub fn new() -> Self {
        Self::with_sample_rate(44_100.0)
    }

    pub fn with_sample_rate(sample_rate: f32) -> Self {
        let sample_rate = if sample_rate.is_finite() && sample_rate >= 2.0 {
            sample_rate
        } else {
            48_000.0
        };
        Self {
            params: std::array::from_fn(|index| BITCRUSH_PARAMS[index].default),
            sample_rate,
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
    fn activation_sample_rate(&self) -> Option<u32> {
        Some(self.sample_rate as u32)
    }

    fn effect_type(&self) -> EffectType {
        EffectType::Bitcrush
    }

    fn param_descriptors(&self) -> &'static [ParamDescriptor] {
        BITCRUSH_PARAMS
    }

    fn set_param(&mut self, index: usize, value: f32) -> bool {
        if !value.is_finite() {
            return false;
        }
        let Some(target) = self.params.get_mut(index) else {
            return false;
        };
        let descriptor = &BITCRUSH_PARAMS[index];
        let value = value.clamp(descriptor.min, descriptor.max);
        *target = if matches!(
            index,
            param::ROUNDING | param::DITHER | param::POST_LP_ENABLED
        ) {
            value.round()
        } else {
            value
        };
        true
    }

    fn get_param(&self, index: usize) -> f32 {
        self.params.get(index).copied().unwrap_or(0.0)
    }

    fn process(&mut self, buffer: &mut [f32], channels: usize) {
        if channels == 0 {
            return;
        }
        let ch = channels.min(2);
        let frames = buffer.len() / channels;
        let levels = 2u32.pow(self.params[param::BITS] as u32) as f32;
        let step = 2.0 / levels;
        let divider = self.params[param::SR_REDUCE].round().max(1.0) as u32;
        let drive_db = self.params[param::DRIVE_DB];
        let drive_gain = 10.0_f64.powf(f64::from(drive_db) / 20.0);

        for frame in 0..frames {
            for c in 0..ch {
                let idx = frame * channels + c;
                let dry = if buffer[idx].is_finite() {
                    buffer[idx]
                } else {
                    0.0
                };

                if self.counter[c] == 0 {
                    // Zero drive retains the original f32 quantizer division for saved projects.
                    let scaled = if drive_db == 0.0 {
                        f64::from(dry / step)
                    } else {
                        f64::from(dry) * drive_gain / f64::from(step)
                    };
                    let quantized = match self.params[param::ROUNDING] as u32 {
                        1 => scaled.floor(),
                        2 => scaled.trunc(),
                        _ => scaled.round(),
                    };
                    let q = quantized * f64::from(step);
                    self.hold[c] = q.clamp(-1.0, 1.0) as f32;
                }
                self.counter[c] = (self.counter[c] + 1) % divider;

                let wet = self.hold[c];
                buffer[idx] = dry * (1.0 - self.params[param::MIX]) + wet * self.params[param::MIX];
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
