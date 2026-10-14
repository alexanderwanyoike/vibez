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
    applied: [f32; param::COUNT],
    steps: [f32; param::COUNT],
    remaining: [u32; param::COUNT],
    started: bool,
    /// One-pole high-shelf-ish state, per channel.
    z: [f32; 2],
    delay: [[f32; 2]; 64],
    delay_position: usize,
    dry_delay: [[f32; 2]; 64],
    mix_output_delay: [[f32; 2]; 64],
    oversampling: [oversampling::OversamplingPath; 3],
    sample_rate: f32,
    modern_low: [f64; 2],
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
            applied: std::array::from_fn(|index| DRIVE_PARAMS[index].default),
            steps: [0.0; param::COUNT],
            remaining: [0; param::COUNT],
            started: false,
            z: [0.0; 2],
            delay: [[0.0; 2]; 64],
            delay_position: 0,
            dry_delay: [[0.0; 2]; 64],
            mix_output_delay: [[1.0; 2]; 64],
            oversampling: [2, 4, 8].map(oversampling::OversamplingPath::new),
            sample_rate,
            modern_low: [0.0; 2],
            pre_hp: ButterworthFilter::EMPTY,
            post_lp: ButterworthFilter::EMPTY,
            meter: WaveShaperMeter::default(),
            meter_samples: 0,
            meter_count: 0,
        }
    }

    fn edit_parameter(&mut self, index: usize, value: f32, automated: bool) -> bool {
        let Some(descriptor) = DRIVE_PARAMS.get(index) else { return false; };
        if index >= param::CHARACTER && !value.is_finite() { return false; }
        let value = value.clamp(descriptor.min, descriptor.max);
        let target = match index {
            param::CHARACTER | param::QUALITY => value.round(),
            param::PRE_HP_ENABLED | param::POST_LP_ENABLED => f32::from(value >= 0.5),
            _ => value,
        };
        if !automated && self.params[index] == target { return true; }
        self.params[index] = target;
        let choice = matches!(index, param::CHARACTER | param::QUALITY);
        if automated || !self.started || choice {
            self.applied[index] = target;
            self.remaining[index] = 0;
        } else {
            self.steps[index] = (target - self.applied[index]) / RAMP_FRAMES as f32;
            self.remaining[index] = RAMP_FRAMES;
        }
        true
    }

    fn drive_gain(&self) -> f32 {
        // Amount 0 → 1x, Amount 1 → ~30x
        let a = self.applied[param::AMOUNT].clamp(0.0, 1.0);
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
            let shaped = match self.applied[param::CHARACTER] as usize {
                2 => 1.5 * u - 0.5 * u * u * u,
                3 => u,
                _ => (input * gain).tanh() / gain.tanh(),
            };
            let processed = input * (1.0 - self.applied[param::AMOUNT]) + shaped * self.applied[param::AMOUNT];
            *output = (input * (1.0 - self.applied[param::MIX]) + processed * self.applied[param::MIX]) * self.applied[param::OUTPUT];
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
        self.edit_parameter(index, value, false)
    }

    fn set_automated_param(&mut self, index: usize, value: f32) -> bool {
        self.edit_parameter(index, value, true)
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
        let mut drive = self.drive_gain();
        let mut norm = drive.tanh();
        let mut cached_amount = self.applied[param::AMOUNT];
        let tone_beta = (-std::f64::consts::TAU * 1000.0 / self.sample_rate as f64).exp();
        let mut cached_tone = self.applied[param::TONE];
        let mut low_gain = 10.0_f64.powf(-6.0 * (cached_tone as f64 * 2.0 - 1.0) / 20.0);
        let mut high_gain = low_gain.recip();
        let mut filter_frequencies = [f32::NAN; 2];

        for frame in 0..frames {
            self.started = true;
            for index in 0..param::COUNT {
                if self.remaining[index] > 0 {
                    self.remaining[index] -= 1;
                    self.applied[index] = if self.remaining[index] == 0 { self.params[index] } else { self.applied[index] + self.steps[index] };
                }
            }
            if cached_amount != self.applied[param::AMOUNT] {
                cached_amount = self.applied[param::AMOUNT];
                drive = self.drive_gain();
                norm = drive.tanh();
            }
            if cached_tone != self.applied[param::TONE] {
                cached_tone = self.applied[param::TONE];
                low_gain = 10.0_f64.powf(-6.0 * (cached_tone as f64 * 2.0 - 1.0) / 20.0);
                high_gain = low_gain.recip();
            }
            let frequencies = [self.applied[param::PRE_HP_FREQUENCY], self.applied[param::POST_LP_FREQUENCY]];
            if frequencies[0] != filter_frequencies[0] { self.pre_hp.configure(self.sample_rate, frequencies[0], true); }
            if frequencies[1] != filter_frequencies[1] { self.post_lp.configure(self.sample_rate, frequencies[1], false); }
            filter_frequencies = frequencies;
            let amount = self.applied[param::AMOUNT];
            let tilt = self.applied[param::TONE] * 2.0 - 1.0;
            // Effect automation is scheduled at the input clock, including final-stage controls.
            let [mix, output] = self.mix_output_delay[self.delay_position];
            self.mix_output_delay[self.delay_position] = [self.applied[param::MIX], self.applied[param::OUTPUT]];
            for c in 0..ch {
                let idx = frame * channels + c;
                let dry = buffer[idx];
                let filtered = self.pre_hp.process(dry, c);
                let shaper_input = dry * (1.0 - self.applied[param::PRE_HP_ENABLED]) + filtered * self.applied[param::PRE_HP_ENABLED];
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
                let character = self.applied[param::CHARACTER] as usize;
                let quality = self.applied[param::QUALITY] as usize;
                let mut modern = 0.0;
                for (index, path) in self.oversampling.iter_mut().enumerate() {
                    let selected = if index == quality { character.checked_sub(1) } else { None };
                    modern += path.process(shaper_input, c, drive, norm, selected);
                }
                let input = modern as f64;
                let low = &mut self.modern_low[c];
                *low = input * (1.0 - tone_beta) + *low * tone_beta;
                if self.applied[param::TONE] != 0.5 { modern = (*low * low_gain + (input - *low) * high_gain) as f32; }
                let processed = if character == 0 {
                    legacy
                } else {
                    aligned_dry * (1.0 - amount) + modern * amount
                };
                let filtered = self.post_lp.process(processed, c);
                let processed = processed * (1.0 - self.applied[param::POST_LP_ENABLED]) + filtered * self.applied[param::POST_LP_ENABLED];
                buffer[idx] = (aligned_dry * (1.0 - mix) + processed * mix) * output;
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
        self.applied = self.params;
        self.remaining = [0; param::COUNT];
        self.started = false;
        self.z = [0.0; 2];
        self.delay = [[0.0; 2]; 64];
        self.delay_position = 0;
        self.dry_delay = [[0.0; 2]; 64];
        self.mix_output_delay = [[self.params[param::MIX], self.params[param::OUTPUT]]; 64];
        self.oversampling.iter_mut().for_each(oversampling::OversamplingPath::reset);
        self.modern_low = [0.0; 2];
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
