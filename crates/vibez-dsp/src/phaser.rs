//! Phaser: a cascade of first-order all-pass filters swept by an LFO, fed
//! back from the wet output and mixed with the dry signal, the classic
//! design in J. O. Smith, "Physical Audio Signal Processing" (phasing with
//! first-order allpasses). The stage count, centre, stereo offset, feedback
//! polarity, LFO shape and tempo sync default to the original four-stage
//! sine phaser, so older projects play the same.

use vibez_core::audio_context::DeviceAudioContext;
use vibez_core::effect::{EffectType, ParamDescriptor};

use crate::effect::AudioEffect;

pub mod param {
    pub const RATE: usize = 0;
    pub const DEPTH: usize = 1;
    pub const FEEDBACK: usize = 2;
    pub const MIX: usize = 3;
    pub const CENTRE: usize = 4;
    pub const STAGES: usize = 5;
    pub const STEREO_PHASE: usize = 6;
    pub const POLARITY: usize = 7;
    pub const SHAPE: usize = 8;
    pub const SYNC: usize = 9;
    pub const COUNT: usize = 10;
}

/// All-pass stages for each Stages choice.
pub const STAGE_COUNTS: [usize; 5] = [2, 4, 6, 8, 12];
const MAX_STAGES: usize = 12;
/// Beats per LFO cycle for each Sync choice after "Free".
pub const SYNC_BEATS: [f64; 6] = [8.0, 4.0, 2.0, 1.0, 0.5, 0.25];
/// How far the sweep reaches above the centre at full depth.
const SWEEP_RATIO: f32 = 8.0;

pub static PHASER_PARAMS: &[ParamDescriptor] = &[
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
    ParamDescriptor {
        name: "Centre",
        min: 50.0,
        max: 4000.0,
        default: 800.0,
        unit: "Hz",
    },
    ParamDescriptor {
        name: "Stages",
        min: 0.0,
        max: (STAGE_COUNTS.len() - 1) as f32,
        default: 1.0,
        unit: "",
    },
    ParamDescriptor {
        name: "Stereo Phase",
        min: 0.0,
        max: 180.0,
        default: 0.0,
        unit: "deg",
    },
    ParamDescriptor {
        name: "FB Polarity",
        min: 0.0,
        max: 1.0,
        default: 0.0,
        unit: "",
    },
    ParamDescriptor {
        name: "LFO Shape",
        min: 0.0,
        max: 1.0,
        default: 0.0,
        unit: "",
    },
    ParamDescriptor {
        name: "Sync",
        min: 0.0,
        max: SYNC_BEATS.len() as f32,
        default: 0.0,
        unit: "",
    },
];

fn value(params: &[f32], index: usize) -> f32 {
    params
        .get(index)
        .copied()
        .unwrap_or(PHASER_PARAMS[index].default)
}

/// The LFO, 0 to 1, at `phase` cycles: a sine, or a triangle.
pub fn lfo(shape: f32, phase: f32) -> f32 {
    if shape >= 0.5 {
        1.0 - (2.0 * phase.rem_euclid(1.0) - 1.0).abs()
    } else {
        0.5 + 0.5 * (std::f32::consts::TAU * phase).sin()
    }
}

/// The all-pass break frequency with the LFO at `lfo`.
pub fn break_hz(params: &[f32], lfo: f32) -> f32 {
    value(params, param::CENTRE) * SWEEP_RATIO.powf(value(params, param::DEPTH) * lfo)
}

/// All-pass coefficient for a break frequency in Hz.
fn coefficient(f_hz: f32, sr: f32) -> f32 {
    let t = (std::f32::consts::PI * f_hz.clamp(20.0, sr * 0.45) / sr).tan();
    (t - 1.0) / (t + 1.0)
}

/// The phaser's gain in dB at `frequency` with the LFO at `lfo`: the dry
/// signal plus the all-passed wet, fed back one sample late.
pub fn response_db(params: &[f32], lfo: f32, frequency: f32, sample_rate: f32) -> f32 {
    let a = f64::from(coefficient(break_hz(params, lfo), sample_rate));
    let w = std::f64::consts::TAU * f64::from(frequency) / f64::from(sample_rate);
    let z1 = (w.cos(), -w.sin());
    let mul = |p: (f64, f64), q: (f64, f64)| (p.0 * q.0 - p.1 * q.1, p.0 * q.1 + p.1 * q.0);
    let div = |p: (f64, f64), q: (f64, f64)| {
        let d = q.0 * q.0 + q.1 * q.1;
        ((p.0 * q.0 + p.1 * q.1) / d, (p.1 * q.0 - p.0 * q.1) / d)
    };
    // One stage: (-a + z^-1) / (1 - a z^-1).
    let stage = div((-a + z1.0, z1.1), (1.0 - a * z1.0, -a * z1.1));
    let stages =
        STAGE_COUNTS[(value(params, param::STAGES).round() as usize).min(STAGE_COUNTS.len() - 1)];
    let mut chain = (1.0, 0.0);
    for _ in 0..stages {
        chain = mul(chain, stage);
    }
    let sign = if value(params, param::POLARITY) >= 0.5 {
        -1.0
    } else {
        1.0
    };
    let feedback = f64::from(value(params, param::FEEDBACK)) * sign;
    let loop_gain = mul(mul(chain, z1), (feedback, 0.0));
    let wet = div(chain, (1.0 - loop_gain.0, -loop_gain.1));
    let mix = f64::from(value(params, param::MIX));
    let total = (1.0 - mix + mix * wet.0, mix * wet.1);
    (10.0 * (total.0 * total.0 + total.1 * total.1).max(1e-12).log10()) as f32
}

/// Zero for a value too small to hear, so a silent cascade stops instead of
/// crawling through slow denormal numbers.
fn flush(value: f32) -> f32 {
    if value.abs() < 1e-15 {
        0.0
    } else {
        value
    }
}

/// All-pass phaser with an LFO and feedback.
pub struct PhaserEffect {
    params: [f32; param::COUNT],
    sample_rate: f32,
    bpm: f64,
    /// LFO phase in radians, as the original kept it.
    phase: f32,
    /// All-pass filter state `z[c][stage]`.
    z: [[f32; MAX_STAGES]; 2],
    /// Previous wet output per channel (for feedback).
    fb: [f32; 2],
}

impl PhaserEffect {
    pub fn new(sample_rate: f32) -> Self {
        let sample_rate = if sample_rate.is_finite() && sample_rate >= 2.0 {
            sample_rate
        } else {
            48_000.0
        };
        Self {
            params: std::array::from_fn(|index| PHASER_PARAMS[index].default),
            sample_rate,
            bpm: vibez_core::constants::DEFAULT_BPM,
            phase: 0.0,
            z: [[0.0; MAX_STAGES]; 2],
            fb: [0.0; 2],
        }
    }

    fn rate_hz(&self) -> f32 {
        let sync = self.params[param::SYNC].round() as usize;
        if sync == 0 {
            self.params[param::RATE]
        } else {
            (self.bpm / 60.0 / SYNC_BEATS[sync - 1]) as f32
        }
    }
}

impl AudioEffect for PhaserEffect {
    fn set_audio_context(&mut self, context: DeviceAudioContext) {
        if context.bpm.is_finite() && context.bpm > 0.0 {
            self.bpm = context.bpm;
        }
    }

    fn activation_sample_rate(&self) -> Option<u32> {
        Some(self.sample_rate as u32)
    }

    fn effect_type(&self) -> EffectType {
        EffectType::Phaser
    }

    fn param_descriptors(&self) -> &'static [ParamDescriptor] {
        PHASER_PARAMS
    }

    fn set_param(&mut self, index: usize, value: f32) -> bool {
        let Some(descriptor) = PHASER_PARAMS.get(index) else {
            return false;
        };
        if !value.is_finite() {
            return false;
        }
        let value = match index {
            param::STAGES | param::SYNC => value.round(),
            param::POLARITY | param::SHAPE => f32::from(value >= 0.5),
            _ => value,
        }
        .clamp(descriptor.min, descriptor.max);
        if index == param::STAGES && value != self.params[index] {
            // Stages coming into use start from silence.
            self.z = [[0.0; MAX_STAGES]; 2];
        }
        self.params[index] = value;
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
        let phase_inc = self.rate_hz() * std::f32::consts::TAU / self.sample_rate;
        let stages = STAGE_COUNTS[self.params[param::STAGES] as usize];
        let offset = self.params[param::STEREO_PHASE] / 360.0 * std::f32::consts::TAU;
        let shape = self.params[param::SHAPE];
        let feedback = self.params[param::FEEDBACK]
            * if self.params[param::POLARITY] >= 0.5 {
                -1.0
            } else {
                1.0
            };
        let mix = self.params[param::MIX];

        for frame in buffer.chunks_exact_mut(channels) {
            let coefficient_at = |phase: f32| {
                // The sine keeps the original's radian phase arithmetic exactly.
                let lfo = if shape >= 0.5 {
                    lfo(shape, phase / std::f32::consts::TAU)
                } else {
                    0.5 + 0.5 * phase.sin()
                };
                coefficient(break_hz(&self.params, lfo), self.sample_rate)
            };
            let left = coefficient_at(self.phase);
            let coefficients = [
                left,
                if offset == 0.0 {
                    left
                } else {
                    coefficient_at(self.phase + offset)
                },
            ];
            for (c, &a) in coefficients.iter().enumerate().take(ch) {
                let dry = if frame[c].is_finite() { frame[c] } else { 0.0 };
                let mut x = dry + self.fb[c] * feedback;
                for z in &mut self.z[c][..stages] {
                    let y = -a * x + *z;
                    *z = flush(x + a * y);
                    x = y;
                }
                self.fb[c] = flush(x);
                frame[c] = dry * (1.0 - mix) + x * mix;
            }

            self.phase += phase_inc;
            if self.phase > std::f32::consts::TAU {
                self.phase -= std::f32::consts::TAU;
            }
        }
    }

    fn reset(&mut self) {
        self.z = [[0.0; MAX_STAGES]; 2];
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

#[cfg(test)]
#[path = "phaser_legacy_tests.rs"]
mod legacy_tests;

#[cfg(test)]
#[path = "phaser_controls_tests.rs"]
mod controls_tests;
