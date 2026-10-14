//! Legacy-compatible drive with a stable native character and quality parameter contract.

use vibez_core::effect::{EffectType, ParamDescriptor};

use crate::effect::AudioEffect;

pub mod param {
    pub const AMOUNT: usize = 0;
    pub const TONE: usize = 1;
    pub const OUTPUT: usize = 2;
    pub const MIX: usize = 3;
    pub const CHARACTER: usize = 4;
    pub const QUALITY: usize = 5;
    pub const PRE_HP_ENABLED: usize = 6;
    pub const PRE_HP_FREQUENCY: usize = 7;
    pub const POST_LP_ENABLED: usize = 8;
    pub const POST_LP_FREQUENCY: usize = 9;
    pub const COUNT: usize = 10;
}

pub const LATENCY_SAMPLES: u32 = 64;
pub const RAMP_FRAMES: u32 = 512;

pub static DRIVE_PARAMS: &[ParamDescriptor] = &[
    ParamDescriptor {
        name: "Amount",
        min: 0.0,
        max: 1.0,
        default: 0.35,
        unit: "",
    },
    ParamDescriptor {
        name: "Tone",
        min: 0.0,
        max: 1.0,
        default: 0.5,
        unit: "",
    },
    ParamDescriptor {
        name: "Output",
        min: 0.0,
        max: 2.0,
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
    ParamDescriptor { name: "Character", min: 0.0, max: 3.0, default: 0.0, unit: "" },
    ParamDescriptor { name: "Quality", min: 0.0, max: 2.0, default: 2.0, unit: "" },
    ParamDescriptor { name: "Pre HP Enabled", min: 0.0, max: 1.0, default: 0.0, unit: "" },
    ParamDescriptor { name: "Pre HP Frequency", min: 20.0, max: 1000.0, default: 80.0, unit: "Hz" },
    ParamDescriptor { name: "Post LP Enabled", min: 0.0, max: 1.0, default: 0.0, unit: "" },
    ParamDescriptor { name: "Post LP Frequency", min: 1000.0, max: 20000.0, default: 12000.0, unit: "Hz" },
];

/// Musical soft-saturation drive.
///
/// `Amount` maps to drive gain via an exponential curve so the knob is
/// useful across its whole range. `Tone` is a one-pole tilt filter applied
/// after the nonlinearity: low values favor low end, high values brighten.
pub struct DriveEffect {
    amount: f32,
    tone: f32,
    output: f32,
    mix: f32,
    /// One-pole high-shelf-ish state, per channel.
    z: [f32; 2],
    extra: [f32; 6],
    delay: [[f32; 2]; 64],
    delay_position: usize,
}

impl DriveEffect {
    pub fn new() -> Self {
        Self {
            amount: 0.35,
            tone: 0.5,
            output: 1.0,
            mix: 1.0,
            z: [0.0; 2],
            extra: [0.0, 2.0, 0.0, 80.0, 0.0, 12000.0],
            delay: [[0.0; 2]; 64],
            delay_position: 0,
        }
    }

    fn drive_gain(&self) -> f32 {
        // Amount 0 → 1x, Amount 1 → ~30x
        let a = self.amount.clamp(0.0, 1.0);
        1.0 + a * a * 29.0
    }
}

impl Default for DriveEffect {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioEffect for DriveEffect {
    fn latency_samples(&self) -> u32 {
        LATENCY_SAMPLES
    }

    fn effect_type(&self) -> EffectType {
        EffectType::Drive
    }

    fn param_descriptors(&self) -> &'static [ParamDescriptor] {
        DRIVE_PARAMS
    }

    fn set_param(&mut self, index: usize, value: f32) -> bool {
        match index {
            0 => {
                self.amount = value.clamp(0.0, 1.0);
                true
            }
            1 => {
                self.tone = value.clamp(0.0, 1.0);
                true
            }
            2 => {
                self.output = value.clamp(0.0, 2.0);
                true
            }
            3 => {
                self.mix = value.clamp(0.0, 1.0);
                true
            }
            4..=9 => {
                if !value.is_finite() { return false; }
                let descriptor = &DRIVE_PARAMS[index];
                let value = value.clamp(descriptor.min, descriptor.max);
                self.extra[index - 4] = match index {
                    param::CHARACTER | param::QUALITY => value.round(),
                    param::PRE_HP_ENABLED | param::POST_LP_ENABLED => f32::from(value >= 0.5),
                    _ => value,
                };
                true
            }
            _ => false,
        }
    }

    fn get_param(&self, index: usize) -> f32 {
        match index {
            0 => self.amount,
            1 => self.tone,
            2 => self.output,
            3 => self.mix,
            4..=9 => self.extra[index - 4],
            _ => 0.0,
        }
    }

    fn process(&mut self, buffer: &mut [f32], channels: usize) {
        if channels == 0 {
            return;
        }
        let ch = channels.min(2);
        let frames = buffer.len() / channels;
        let drive = self.drive_gain();
        let norm = drive.tanh();
        let amount = self.amount.clamp(0.0, 1.0);
        let tilt = self.tone * 2.0 - 1.0; // -1 darker, +1 brighter

        for frame in 0..frames {
            for c in 0..ch {
                let idx = frame * channels + c;
                let dry = buffer[idx];
                let sat = (dry * drive).tanh() / norm;
                // One-pole tilt: mixes current with previous sample.
                // tilt > 0 emphasises difference (bright); < 0 smooths.
                let shelved = sat + tilt * (sat - self.z[c]);
                self.z[c] = sat;

                // `Amount` blends between dry and saturated so amount=0 is
                // true passthrough; `Mix` then wet/dries the result.
                let processed = dry * (1.0 - amount) + shelved * amount;
                let processed = (dry * (1.0 - self.mix) + processed * self.mix) * self.output;
                buffer[idx] = self.delay[self.delay_position][c];
                self.delay[self.delay_position][c] = processed;
            }
            self.delay_position = (self.delay_position + 1) % 64;
        }
    }

    fn reset(&mut self) {
        self.z = [0.0; 2];
        self.delay = [[0.0; 2]; 64];
        self.delay_position = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_drive_preserves_peak_approximately() {
        let mut fx = DriveEffect::new();
        fx.set_param(0, 0.0);
        fx.set_param(3, 1.0);
        let mut buf = vec![0.5_f32; 256];
        fx.process(&mut buf, 2);
        for s in &buf[128..] {
            assert!((s - 0.5).abs() < 0.01);
        }
    }

    #[test]
    fn high_drive_clamps_output() {
        let mut fx = DriveEffect::new();
        fx.set_param(0, 1.0);
        fx.set_param(2, 1.0);
        fx.set_param(3, 1.0);
        let mut buf = vec![5.0_f32; 256];
        fx.process(&mut buf, 2);
        let peak = buf.iter().map(|s| s.abs()).fold(0.0_f32, f32::max);
        assert!(peak <= 1.05, "peak {peak}");
    }

    #[test]
    fn mix_zero_is_dry() {
        let mut fx = DriveEffect::new();
        fx.set_param(0, 1.0);
        fx.set_param(3, 0.0);
        let orig = vec![0.3_f32; 256];
        let mut buf = orig.clone();
        fx.process(&mut buf, 2);
        for (a, b) in buf[128..].iter().zip(orig.iter()) {
            assert!((a - b).abs() < 1e-5);
        }
    }
}
