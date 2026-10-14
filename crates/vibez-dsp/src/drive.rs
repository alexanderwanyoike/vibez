//! Legacy-compatible drive with a stable native character and quality parameter contract.

use vibez_core::effect::{EffectType, ParamDescriptor, WaveShaperMeter};

use crate::{butterworth_filter::ButterworthFilter, effect::AudioEffect};

#[path = "drive_oversampling.rs"]
mod oversampling;

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
    params: [f32; param::COUNT],
    /// One-pole high-shelf-ish state, per channel.
    z: [f32; 2],
    delay: [[f32; 2]; 64],
    delay_position: usize,
    dry_delay: [[f32; 2]; 64],
    oversampling: [oversampling::OversamplingPath; 3],
    sample_rate: f32,
    modern_low: [[f64; 2]; 3],
    pre_hp: ButterworthFilter,
    post_lp: ButterworthFilter,
    meter: WaveShaperMeter,
    meter_samples: u64,
    meter_count: u16,
}

impl DriveEffect {
    pub fn new() -> Self {
        Self::with_sample_rate(44_100.0)
    }

    pub fn with_sample_rate(sample_rate: f32) -> Self {
        let sample_rate = if sample_rate.is_finite() && sample_rate >= 2.0 { sample_rate } else { 48_000.0 };
        Self {
            params: std::array::from_fn(|index| DRIVE_PARAMS[index].default),
            z: [0.0; 2],
            delay: [[0.0; 2]; 64],
            delay_position: 0,
            dry_delay: [[0.0; 2]; 64],
            oversampling: [2, 4, 8].map(oversampling::OversamplingPath::new),
            sample_rate,
            modern_low: [[0.0; 2]; 3],
            pre_hp: ButterworthFilter::EMPTY,
            post_lp: ButterworthFilter::EMPTY,
            meter: WaveShaperMeter::default(),
            meter_samples: 0,
            meter_count: 0,
        }
    }

    fn drive_gain(&self) -> f32 {
        // Amount 0 → 1x, Amount 1 → ~30x
        let a = self.params[param::AMOUNT].clamp(0.0, 1.0);
        1.0 + a * a * 29.0
    }
}

impl Default for DriveEffect {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioEffect for DriveEffect {
    fn supports_level_meter(&self) -> bool { true }

    fn activation_sample_rate(&self) -> Option<u32> { Some(self.sample_rate as u32) }

    fn take_waveshaper_meter(&mut self) -> Option<WaveShaperMeter> {
        let count = self.meter_count as usize;
        let first_output_sample = self.meter_samples.saturating_sub(count as u64);
        let mut meter = WaveShaperMeter {
            count: self.meter_count,
            sample_rate: self.sample_rate,
            first_output_sample,
            latency_samples: LATENCY_SAMPLES,
            ..WaveShaperMeter::default()
        };
        for point in 0..count {
            let position = (first_output_sample as usize + point) % 128;
            meter.input[point] = self.meter.input[position];
            meter.output[point] = self.meter.output[position];
        }
        let gain = self.drive_gain();
        for (point, output) in meter.transfer.iter_mut().enumerate() {
            let input = -1.0 + point as f32 / 32.0;
            let u = (input * gain).clamp(-1.0, 1.0);
            let shaped = match self.params[param::CHARACTER] as usize {
                2 => 1.5 * u - 0.5 * u * u * u,
                3 => u,
                _ => (input * gain).tanh() / gain.tanh(),
            };
            let processed = input * (1.0 - self.params[param::AMOUNT]) + shaped * self.params[param::AMOUNT];
            *output = (input * (1.0 - self.params[param::MIX]) + processed * self.params[param::MIX]) * self.params[param::OUTPUT];
        }
        self.meter_count = 0;
        Some(meter)
    }

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
        let Some(descriptor) = DRIVE_PARAMS.get(index) else { return false; };
        if index >= param::CHARACTER && !value.is_finite() { return false; }
        let value = value.clamp(descriptor.min, descriptor.max);
        self.params[index] = match index {
            param::CHARACTER | param::QUALITY => value.round(),
            param::PRE_HP_ENABLED | param::POST_LP_ENABLED => f32::from(value >= 0.5),
            _ => value,
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
        self.pre_hp.configure(self.sample_rate, self.params[param::PRE_HP_FREQUENCY], true);
        self.post_lp.configure(self.sample_rate, self.params[param::POST_LP_FREQUENCY], false);
        let ch = channels.min(2);
        let frames = buffer.len() / channels;
        let drive = self.drive_gain();
        let norm = drive.tanh();
        let amount = self.params[param::AMOUNT].clamp(0.0, 1.0);
        let tone_beta = (-std::f64::consts::TAU * 1000.0 / self.sample_rate as f64).exp();
        let low_gain = 10.0_f64.powf(-6.0 * (self.params[param::TONE] as f64 * 2.0 - 1.0) / 20.0);
        let high_gain = low_gain.recip();
        let tilt = self.params[param::TONE] * 2.0 - 1.0; // -1 darker, +1 brighter

        for frame in 0..frames {
            for c in 0..ch {
                let idx = frame * channels + c;
                let dry = buffer[idx];
                let filtered = self.pre_hp.process(dry, c);
                let shaper_input = if self.params[param::PRE_HP_ENABLED] == 0.0 { dry } else { filtered };
                let sat = (shaper_input * drive).tanh() / norm;
                // One-pole tilt: mixes current with previous sample.
                // tilt > 0 emphasises difference (bright); < 0 smooths.
                let shelved = sat + tilt * (sat - self.z[c]);
                self.z[c] = sat;

                // `Amount` blends between dry and saturated so amount=0 is
                // true passthrough; `Mix` then wet/dries the result.
                let processed = dry * (1.0 - amount) + shelved * amount;
                let legacy = self.delay[self.delay_position][c];
                self.delay[self.delay_position][c] = processed;
                let aligned_dry = self.dry_delay[self.delay_position][c];
                self.dry_delay[self.delay_position][c] = dry;
                let character = self.params[param::CHARACTER] as usize;
                let quality = self.params[param::QUALITY] as usize;
                let mut modern = [0.0; 3];
                for (index, path) in self.oversampling.iter_mut().enumerate() {
                    let audible = index == quality;
                    let output = path.process(shaper_input, c, drive, norm, audible);
                    if audible { modern = output; }
                }
                for (index, sample) in modern.iter_mut().enumerate() {
                    let input = *sample as f64;
                    let low = &mut self.modern_low[index][c];
                    *low = input * (1.0 - tone_beta) + *low * tone_beta;
                    if self.params[param::TONE] != 0.5 { *sample = (*low * low_gain + (input - *low) * high_gain) as f32; }
                }
                let processed = if character == 0 {
                    legacy
                } else {
                    aligned_dry * (1.0 - amount) + modern[character - 1] * amount
                };
                let filtered = self.post_lp.process(processed, c);
                let processed = if self.params[param::POST_LP_ENABLED] == 0.0 { processed } else { filtered };
                buffer[idx] = (aligned_dry * (1.0 - self.params[param::MIX]) + processed * self.params[param::MIX]) * self.params[param::OUTPUT];
                if c == 0 {
                    let point = self.meter_samples as usize % 128;
                    self.meter.input[point] = aligned_dry;
                    self.meter.output[point] = buffer[idx];
                }
            }
            self.delay_position = (self.delay_position + 1) % 64;
            self.meter_samples = self.meter_samples.saturating_add(1);
            self.meter_count = (self.meter_count + 1).min(128);
        }
    }

    fn reset(&mut self) {
        self.z = [0.0; 2];
        self.delay = [[0.0; 2]; 64];
        self.delay_position = 0;
        self.dry_delay = [[0.0; 2]; 64];
        self.oversampling.iter_mut().for_each(oversampling::OversamplingPath::reset);
        self.modern_low = [[0.0; 2]; 3];
        self.pre_hp.reset();
        self.post_lp.reset();
        self.meter = WaveShaperMeter::default();
        self.meter_samples = 0;
        self.meter_count = 0;
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
