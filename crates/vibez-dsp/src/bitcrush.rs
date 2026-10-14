//! Bitcrush quantization and source-frame sample-and-hold processing.

use vibez_core::effect::{EffectType, ParamDescriptor};
use vibez_core::id::EffectId;

use crate::{butterworth_filter::ButterworthFilter, effect::AudioEffect};

#[path = "bitcrush_clock.rs"]
mod clock;
#[path = "bitcrush_random.rs"]
mod random;
#[path = "bitcrush_ramp.rs"]
mod ramp;
#[path = "bitcrush_scope.rs"]
mod scope;
use clock::Clock;
use random::Random;
use ramp::Ramp;
use scope::Scope;

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
    drive: Ramp,
    mix: Ramp,
    jitter: Ramp,
    post_lp_amount: Ramp,
    post_lp_cutoff: Ramp,
    post_lp: ButterworthFilter,
    drive_gain: f64,
    started: bool,
    scope: Scope,
    random: Random,
    source_frames: u64,
    hold: [f32; 2],
    clock: Clock,
}

impl BitcrushEffect {
    pub fn new() -> Self {
        Self::with_sample_rate(44_100.0)
    }

    /// Standalone processors share the deterministic identity-zero dither stream.
    pub fn with_sample_rate(sample_rate: f32) -> Self {
        Self::with_identity(sample_rate, 0)
    }

    pub(crate) fn for_instance(sample_rate: f32, effect_id: EffectId) -> Self {
        Self::with_identity(sample_rate, effect_id.raw())
    }

    fn with_identity(sample_rate: f32, identity: u64) -> Self {
        let sample_rate = if sample_rate.is_finite() && sample_rate >= 2.0 {
            sample_rate
        } else {
            48_000.0
        };
        let mut post_lp = ButterworthFilter::EMPTY;
        post_lp.configure(sample_rate, BITCRUSH_PARAMS[param::POST_LP_HZ].default, false);
        Self {
            params: std::array::from_fn(|index| BITCRUSH_PARAMS[index].default),
            sample_rate,
            drive: Ramp::new(0.0),
            mix: Ramp::new(1.0),
            jitter: Ramp::new(0.0),
            post_lp_amount: Ramp::new(0.0),
            post_lp_cutoff: Ramp::new(BITCRUSH_PARAMS[param::POST_LP_HZ].default),
            post_lp,
            drive_gain: 1.0,
            started: false,
            scope: Scope::default(),
            random: Random::for_identity(identity),
            source_frames: 0,
            hold: [0.0; 2],
            clock: Clock::default(),
        }
    }

    fn edit_param(&mut self, index: usize, value: f32, automated: bool) -> bool {
        if !value.is_finite() {
            return false;
        }
        let Some(target) = self.params.get_mut(index) else {
            return false;
        };
        let descriptor = &BITCRUSH_PARAMS[index];
        let value = value.clamp(descriptor.min, descriptor.max);
        let value = if matches!(
            index,
            param::ROUNDING | param::DITHER | param::POST_LP_ENABLED
        ) {
            value.round()
        } else {
            value
        };
        if index == param::DRIVE_DB && (value != *target || automated) {
            let immediate = automated || !self.started;
            self.drive.edit(value, immediate);
            if immediate {
                self.drive_gain = 10.0_f64.powf(f64::from(value) / 20.0);
            }
        }
        if index == param::MIX && (value != *target || automated) {
            self.mix.edit(value, automated || !self.started);
        }
        if index == param::JITTER && (value != *target || automated) {
            self.jitter.edit(value, automated || !self.started);
        }
        if index == param::POST_LP_ENABLED && (value != *target || automated) {
            self.post_lp_amount.edit(value, automated || !self.started);
        }
        if index == param::POST_LP_HZ && (value != *target || automated) {
            let immediate = automated || !self.started;
            self.post_lp_cutoff.edit(value, immediate);
            if immediate {
                self.post_lp.configure(self.sample_rate, value, false);
            }
        }
        *target = value;
        true
    }
}

impl Default for BitcrushEffect {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioEffect for BitcrushEffect {
    fn supports_level_meter(&self) -> bool {
        true
    }

    fn take_sample_hold_meter(&mut self) -> Option<vibez_core::effect::SampleHoldMeter> {
        Some(self.scope.take(self.sample_rate))
    }

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
        self.edit_param(index, value, false)
    }

    fn set_automated_param(&mut self, index: usize, value: f32) -> bool {
        self.edit_param(index, value, true)
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
        if frames == 0 {
            return;
        }
        self.started = true;
        let levels = 2u32.pow(self.params[param::BITS] as u32) as f32;
        let step = 2.0 / levels;
        let divider = self.params[param::SR_REDUCE].round().max(1.0) as u32;
        let dither = self.params[param::DITHER] != 0.0;

        for frame in 0..frames {
            if let Some(value) = self.drive.advance(self.params[param::DRIVE_DB]) {
                self.drive_gain = 10.0_f64.powf(f64::from(value) / 20.0);
            }
            let _ = self.mix.advance(self.params[param::MIX]);
            let _ = self.jitter.advance(self.params[param::JITTER]);
            let _ = self.post_lp_amount.advance(self.params[param::POST_LP_ENABLED]);
            if let Some(cutoff) = self.post_lp_cutoff.advance(self.params[param::POST_LP_HZ]) {
                self.post_lp.configure(self.sample_rate, cutoff, false);
            }
            let mix = self.mix.value();
            let drive_db = self.drive.value();
            let drive_gain = self.drive_gain;
            let acquired = self.clock.advance(
                divider,
                self.jitter.value(),
                self.source_frames,
                &self.random,
            );
            for c in 0..ch {
                let idx = frame * channels + c;
                let dry = if buffer[idx].is_finite() {
                    buffer[idx]
                } else {
                    0.0
                };

                if acquired {
                    // Neutral drive/dither retain the original f32 division for saved projects.
                    let scaled = if drive_db == 0.0 && !dither {
                        f64::from(dry / step)
                    } else {
                        f64::from(dry) * drive_gain / f64::from(step)
                    };
                    let scaled = if dither {
                        scaled + self.random.tpdf(self.source_frames, c)
                    } else {
                        scaled
                    };
                    let quantized = match self.params[param::ROUNDING] as u32 {
                        1 => scaled.floor(),
                        2 => scaled.trunc(),
                        _ => scaled.round(),
                    };
                    let q = quantized * f64::from(step);
                    self.hold[c] = q.clamp(-1.0, 1.0) as f32;
                }
                let wet = self.hold[c];
                // Keeping this history warm avoids a cold filter transient when enabled.
                let filtered = self.post_lp.process(wet, c);
                let amount = self.post_lp_amount.value();
                let wet = if amount == 0.0 {
                    wet
                } else {
                    wet * (1.0 - amount) + filtered * amount
                };
                buffer[idx] = dry * (1.0 - mix) + wet * mix;
                if c == 0 {
                    self.scope.record(dry, buffer[idx], acquired, self.clock.interval());
                }
            }
            self.source_frames = self.source_frames.wrapping_add(1);
        }
    }

    fn reset(&mut self) {
        self.source_frames = 0;
        self.scope = Scope::default();
        self.hold = [0.0; 2];
        self.clock = Clock::default();
        self.mix.edit(self.params[param::MIX], true);
        self.jitter.edit(self.params[param::JITTER], true);
        self.post_lp_amount.edit(self.params[param::POST_LP_ENABLED], true);
        self.post_lp_cutoff.edit(self.params[param::POST_LP_HZ], true);
        self.post_lp.configure(self.sample_rate, self.post_lp_cutoff.value(), false);
        self.post_lp.reset();
        self.drive.edit(self.params[param::DRIVE_DB], true);
        self.drive_gain = 10.0_f64.powf(f64::from(self.drive.value()) / 20.0);
        self.started = false;
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
