use vibez_core::effect::{EffectType, ParamDescriptor};

use crate::effect::AudioEffect;

static DRIVE_PARAMS: &[ParamDescriptor] = &[
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
}

impl DriveEffect {
    pub fn new() -> Self {
        Self {
            amount: 0.35,
            tone: 0.5,
            output: 1.0,
            mix: 1.0,
            z: [0.0; 2],
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
            _ => false,
        }
    }

    fn get_param(&self, index: usize) -> f32 {
        match index {
            0 => self.amount,
            1 => self.tone,
            2 => self.output,
            3 => self.mix,
            _ => 0.0,
        }
    }

    fn process(&mut self, buffer: &mut [f32], channels: usize) {
        let ch = channels.clamp(1, 2);
        let frames = buffer.len() / ch;
        let drive = self.drive_gain();
        let norm = drive.tanh();
        let tilt = self.tone * 2.0 - 1.0; // -1 darker, +1 brighter

        for frame in 0..frames {
            for c in 0..ch {
                let idx = frame * ch + c;
                let dry = buffer[idx];
                let sat = (dry * drive).tanh() / norm;
                // One-pole tilt: mixes current with previous sample.
                // tilt > 0 emphasises difference (bright); < 0 smooths.
                let shelved = sat + tilt * (sat - self.z[c]);
                self.z[c] = sat;

                buffer[idx] = (dry * (1.0 - self.mix) + shelved * self.mix) * self.output;
            }
        }
    }

    fn reset(&mut self) {
        self.z = [0.0; 2];
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
        let mut buf = vec![0.5_f32; 64];
        fx.process(&mut buf, 2);
        for s in &buf {
            assert!((s - 0.5).abs() < 0.01);
        }
    }

    #[test]
    fn high_drive_clamps_output() {
        let mut fx = DriveEffect::new();
        fx.set_param(0, 1.0);
        fx.set_param(2, 1.0);
        fx.set_param(3, 1.0);
        let mut buf = vec![5.0_f32; 32];
        fx.process(&mut buf, 2);
        let peak = buf.iter().map(|s| s.abs()).fold(0.0_f32, f32::max);
        assert!(peak <= 1.05, "peak {peak}");
    }

    #[test]
    fn mix_zero_is_dry() {
        let mut fx = DriveEffect::new();
        fx.set_param(0, 1.0);
        fx.set_param(3, 0.0);
        let orig = vec![0.3_f32; 16];
        let mut buf = orig.clone();
        fx.process(&mut buf, 2);
        for (a, b) in buf.iter().zip(orig.iter()) {
            assert!((a - b).abs() < 1e-5);
        }
    }
}
