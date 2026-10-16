//! Saturation with a choice of curve, oversampling for the newer curves, a
//! tone tilt, and optional filters either side of the shaper.
//!
//! Classic is the original Drive: tanh over a one-sample tilt at the host
//! rate, unchanged so older projects sound the same. Soft (tanh), Warm (the
//! cubic soft clip 1.5x - 0.5x^3 from J. O. Smith, "Physical Audio Signal
//! Processing") and Hard (a clip) run at 1x, 2x or 4x through the halfband
//! stages in [`crate::oversampling`], and tilt around 1 kHz. Only the
//! curves and rates in use are computed; a switch crossfades over
//! [`RAMP_FRAMES`]. Every setting reports the 4x path's delay, so the
//! device's latency never changes.

use vibez_core::effect::{EffectType, ParamDescriptor};

use crate::butterworth_filter::ButterworthFilter;
use crate::effect::AudioEffect;
use crate::oversampling::Stage;

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

pub mod character {
    pub const CLASSIC: usize = 0;
    pub const SOFT: usize = 1;
    pub const WARM: usize = 2;
    pub const HARD: usize = 3;
    pub const COUNT: usize = 4;
}

pub mod quality {
    pub const X1: usize = 0;
    pub const X2: usize = 1;
    pub const X4: usize = 2;
    pub const COUNT: usize = 3;
}

/// Halfband sizes: the first stage needs a narrow transition, the second,
/// working on a signal already band-limited to a quarter of its rate, does not.
const K1: usize = 15;
const T1: usize = K1 + 1;
const K2: usize = 7;
const T2: usize = K2 + 1;
const BETA1: f64 = 7.0;
const BETA2: f64 = 6.0;
/// The 4x path: the first stage's round trip, then the second's, made a
/// whole number of host samples by one extra sample at twice the rate.
pub const LATENCY_SAMPLES: u32 = (K1 + K2.div_ceil(2)) as u32;
const LATENCY: usize = LATENCY_SAMPLES as usize;
/// The 2x path is short of the 4x path's delay by this much.
const X2_PAD: usize = LATENCY - K1;
pub const RAMP_FRAMES: u32 = 512;
const TONE_SPLIT_HZ: f64 = 1000.0;

pub static DRIVE_PARAMS: &[ParamDescriptor] = &[
    ParamDescriptor {
        name: "Drive",
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
    ParamDescriptor {
        name: "Character",
        min: 0.0,
        max: (character::COUNT - 1) as f32,
        default: character::CLASSIC as f32,
        unit: "",
    },
    ParamDescriptor {
        name: "Oversampling",
        min: 0.0,
        max: (quality::COUNT - 1) as f32,
        default: quality::X2 as f32,
        unit: "",
    },
    ParamDescriptor {
        name: "Pre Filter",
        min: 0.0,
        max: 1.0,
        default: 0.0,
        unit: "",
    },
    ParamDescriptor {
        name: "Pre Frequency",
        min: 20.0,
        max: 1000.0,
        default: 80.0,
        unit: "Hz",
    },
    ParamDescriptor {
        name: "Post Filter",
        min: 0.0,
        max: 1.0,
        default: 0.0,
        unit: "",
    },
    ParamDescriptor {
        name: "Post Frequency",
        min: 1000.0,
        max: 20000.0,
        default: 12000.0,
        unit: "Hz",
    },
];

/// Gain into the shaper: 1x with no drive, about 30x at full drive.
pub fn drive_gain(amount: f32) -> f32 {
    let amount = amount.clamp(0.0, 1.0);
    1.0 + amount * amount * 29.0
}

/// A character's memoryless curve for `driven`, the input times the drive
/// gain, scaled so that full drive peaks at unity.
pub fn shape(character: usize, driven: f32, normalizer: f32) -> f32 {
    match character {
        character::WARM => {
            let limited = driven.clamp(-1.0, 1.0);
            1.5 * limited - 0.5 * limited * limited * limited
        }
        character::HARD => driven.clamp(-1.0, 1.0),
        _ => driven.tanh() / normalizer,
    }
}

/// The amplitude these settings map `input` to, leaving out the tone,
/// filters and oversampling, which have memory.
pub fn transfer(params: &[f32], input: f32) -> f32 {
    let value = |index: usize| {
        params
            .get(index)
            .copied()
            .unwrap_or(DRIVE_PARAMS[index].default)
    };
    let amount = value(param::AMOUNT).clamp(0.0, 1.0);
    let gain = drive_gain(amount);
    let shaped = shape(
        value(param::CHARACTER).round() as usize,
        input * gain,
        gain.tanh(),
    );
    let processed = input * (1.0 - amount) + shaped * amount;
    let mix = value(param::MIX);
    (input * (1.0 - mix) + processed * mix) * value(param::OUTPUT)
}

fn finite(sample: f32) -> f32 {
    if sample.is_finite() {
        sample
    } else {
        0.0
    }
}

/// Weights for a choice, moving from one option to another over
/// [`RAMP_FRAMES`] so a switch never clicks.
#[derive(Clone, Copy)]
struct Fade<const N: usize> {
    weights: [f32; N],
    steps: [f32; N],
    remaining: u32,
}

impl<const N: usize> Fade<N> {
    fn at(choice: usize) -> Self {
        Self {
            weights: std::array::from_fn(|index| f32::from(index == choice)),
            steps: [0.0; N],
            remaining: 0,
        }
    }

    fn toward(&mut self, choice: usize) {
        self.steps = std::array::from_fn(|index| {
            (f32::from(index == choice) - self.weights[index]) / RAMP_FRAMES as f32
        });
        self.remaining = RAMP_FRAMES;
    }

    fn advance(&mut self) {
        if self.remaining == 0 {
            return;
        }
        self.remaining -= 1;
        if self.remaining == 0 {
            self.weights = self.weights.map(f32::round);
        } else {
            for (weight, step) in self.weights.iter_mut().zip(self.steps) {
                *weight += step;
            }
        }
    }
}

/// Stereo frames delayed by `N`.
#[derive(Clone, Copy)]
struct Delay<const N: usize> {
    frames: [[f32; 2]; N],
    position: usize,
}

impl<const N: usize> Delay<N> {
    const EMPTY: Self = Self {
        frames: [[0.0; 2]; N],
        position: 0,
    };

    fn push(&mut self, frame: [f32; 2]) -> [f32; 2] {
        let out = std::mem::replace(&mut self.frames[self.position], frame);
        self.position = (self.position + 1) % N;
        out
    }
}

/// The newer curves at one rate, with that rate's filter state.
struct Path {
    first: Stage<K1, T1>,
    second: Stage<K2, T2>,
    /// The second stage's extra sample at twice the host rate.
    held: [f32; 2],
}

impl Path {
    fn new() -> Self {
        Self {
            first: Stage::new(BETA1),
            second: Stage::new(BETA2),
            held: [0.0; 2],
        }
    }

    fn reset(&mut self) {
        self.first.reset();
        self.second.reset();
        self.held = [0.0; 2];
    }
}

pub struct DriveEffect {
    sample_rate: f32,
    params: [f32; param::COUNT],
    /// Continuous controls as currently applied, ramping toward `params`.
    applied: [f32; param::COUNT],
    steps: [f32; param::COUNT],
    remaining: [u32; param::COUNT],
    started: bool,
    character: Fade<{ character::COUNT }>,
    quality: Fade<{ quality::COUNT }>,
    paths: [Path; 2],
    classic_tilt: [f32; 2],
    tone_low: [f64; 2],
    /// The tone a shelf gain was last worked out for, and that gain.
    tone_gain: (f32, f64),
    pre_hp: ButterworthFilter,
    post_lp: ButterworthFilter,
    filter_frequencies: [f32; 2],
    dry: Delay<LATENCY>,
    classic: Delay<LATENCY>,
    x1: Delay<LATENCY>,
    x2: Delay<X2_PAD>,
}

impl DriveEffect {
    pub fn new(sample_rate: f32) -> Self {
        let sample_rate = if sample_rate.is_finite() && sample_rate >= 2.0 {
            sample_rate
        } else {
            48_000.0
        };
        let params = std::array::from_fn(|index| DRIVE_PARAMS[index].default);
        Self {
            sample_rate,
            params,
            applied: params,
            steps: [0.0; param::COUNT],
            remaining: [0; param::COUNT],
            started: false,
            character: Fade::at(character::CLASSIC),
            quality: Fade::at(quality::X2),
            paths: [Path::new(), Path::new()],
            classic_tilt: [0.0; 2],
            tone_low: [0.0; 2],
            tone_gain: (f32::NAN, 1.0),
            pre_hp: ButterworthFilter::EMPTY,
            post_lp: ButterworthFilter::EMPTY,
            filter_frequencies: [f32::NAN; 2],
            dry: Delay::EMPTY,
            classic: Delay::EMPTY,
            x1: Delay::EMPTY,
            x2: Delay::EMPTY,
        }
    }

    fn edit(&mut self, index: usize, value: f32, automated: bool) -> bool {
        let Some(descriptor) = DRIVE_PARAMS.get(index) else {
            return false;
        };
        if !value.is_finite() {
            return false;
        }
        let value = match index {
            param::CHARACTER | param::QUALITY => value.round(),
            param::PRE_HP_ENABLED | param::POST_LP_ENABLED => f32::from(value >= 0.5),
            _ => value,
        }
        .clamp(descriptor.min, descriptor.max);
        if self.params[index] == value && !automated {
            return true;
        }
        self.params[index] = value;
        let immediate = automated || !self.started;
        match index {
            param::CHARACTER if immediate => self.character = Fade::at(value as usize),
            param::CHARACTER => self.character.toward(value as usize),
            param::QUALITY if immediate => self.quality = Fade::at(value as usize),
            param::QUALITY => self.quality.toward(value as usize),
            _ if immediate => {
                self.applied[index] = value;
                self.remaining[index] = 0;
            }
            _ => {
                self.steps[index] = (value - self.applied[index]) / RAMP_FRAMES as f32;
                self.remaining[index] = RAMP_FRAMES;
            }
        }
        true
    }

    fn advance(&mut self) {
        let filters_were = [
            self.applied[param::PRE_HP_ENABLED],
            self.applied[param::POST_LP_ENABLED],
        ];
        for index in 0..param::COUNT {
            if self.remaining[index] > 0 {
                self.remaining[index] -= 1;
                self.applied[index] = if self.remaining[index] == 0 {
                    self.params[index]
                } else {
                    self.applied[index] + self.steps[index]
                };
            }
        }
        let was = self.quality.weights;
        self.character.advance();
        self.quality.advance();
        // A filter or rate coming back into use starts from silence, not
        // from whatever it last heard.
        if filters_were[0] == 0.0 && self.applied[param::PRE_HP_ENABLED] > 0.0 {
            self.pre_hp.reset();
        }
        if filters_were[1] == 0.0 && self.applied[param::POST_LP_ENABLED] > 0.0 {
            self.post_lp.reset();
        }
        for (path, quality) in [quality::X2, quality::X4].into_iter().enumerate() {
            if was[quality] == 0.0 && self.quality.weights[quality] > 0.0 {
                self.paths[path].reset();
            }
        }
        let frequencies = [
            self.applied[param::PRE_HP_FREQUENCY],
            self.applied[param::POST_LP_FREQUENCY],
        ];
        if frequencies[0] != self.filter_frequencies[0] {
            self.pre_hp
                .configure(self.sample_rate, frequencies[0], true);
        }
        if frequencies[1] != self.filter_frequencies[1] {
            self.post_lp
                .configure(self.sample_rate, frequencies[1], false);
        }
        self.filter_frequencies = frequencies;
    }

    /// The weighted newer curves for one driven sample.
    fn curves(weights: &[f32; character::COUNT], driven: f32, normalizer: f32) -> f32 {
        let mut shaped = 0.0;
        for curve in [character::SOFT, character::WARM, character::HARD] {
            if weights[curve] > 0.0 {
                shaped += weights[curve] * shape(curve, driven, normalizer);
            }
        }
        shaped
    }
}

impl AudioEffect for DriveEffect {
    fn supports_level_meter(&self) -> bool {
        true
    }

    fn activation_sample_rate(&self) -> Option<u32> {
        Some(self.sample_rate as u32)
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
        self.edit(index, value, false)
    }

    fn set_automated_param(&mut self, index: usize, value: f32) -> bool {
        self.edit(index, value, true)
    }

    fn get_param(&self, index: usize) -> f32 {
        self.params.get(index).copied().unwrap_or(0.0)
    }

    fn process(&mut self, buffer: &mut [f32], channels: usize) {
        if channels == 0 {
            return;
        }
        let ch = channels.min(2);
        let tone_beta = (-std::f64::consts::TAU * TONE_SPLIT_HZ / self.sample_rate as f64).exp();
        for frame in buffer.chunks_exact_mut(channels) {
            self.started = true;
            self.advance();
            let amount = self.applied[param::AMOUNT].clamp(0.0, 1.0);
            let gain = drive_gain(amount);
            let normalizer = gain.tanh();
            let characters = self.character.weights;
            let qualities = self.quality.weights;
            let modern = 1.0 - characters[character::CLASSIC];
            let tone = self.applied[param::TONE];
            let pre = self.applied[param::PRE_HP_ENABLED];

            let mut dry = [0.0; 2];
            let mut classic = [0.0; 2];
            let mut rates = [[0.0; 2]; quality::COUNT];
            for c in 0..ch {
                dry[c] = finite(frame[c]);
                let input = if pre > 0.0 {
                    let filtered = finite(self.pre_hp.process(dry[c], c));
                    dry[c] * (1.0 - pre) + filtered * pre
                } else {
                    dry[c]
                };
                if characters[character::CLASSIC] > 0.0 {
                    let shaped = (input * gain).tanh() / normalizer;
                    let tilted = shaped + (tone * 2.0 - 1.0) * (shaped - self.classic_tilt[c]);
                    self.classic_tilt[c] = shaped;
                    classic[c] = characters[character::CLASSIC]
                        * (dry[c] * (1.0 - amount) + tilted * amount);
                }
                if modern == 0.0 {
                    continue;
                }
                let curves = |driven: f32| Self::curves(&characters, driven * gain, normalizer);
                if qualities[quality::X1] > 0.0 {
                    rates[quality::X1][c] = curves(input);
                }
                if qualities[quality::X2] > 0.0 {
                    let stage = &mut self.paths[0].first;
                    let pair = stage.up(c, input).map(curves);
                    rates[quality::X2][c] = stage.down(c, pair);
                }
                if qualities[quality::X4] > 0.0 {
                    let path = &mut self.paths[1];
                    let doubled = path.first.up(c, input);
                    let shaped = doubled.map(|sample| {
                        let pair = path.second.up(c, sample).map(curves);
                        path.second.down(c, pair)
                    });
                    let held = std::mem::replace(&mut path.held[c], shaped[1]);
                    rates[quality::X4][c] = path.first.down(c, [held, shaped[0]]);
                }
            }

            let dry = self.dry.push(dry);
            let classic = self.classic.push(classic);
            let x1 = self.x1.push(rates[quality::X1]);
            let x2 = self.x2.push(rates[quality::X2]);
            let x4 = rates[quality::X4];
            if self.tone_gain.0 != tone {
                self.tone_gain = (tone, 10.0_f64.powf(-6.0 * (tone as f64 * 2.0 - 1.0) / 20.0));
            }
            let low_gain = self.tone_gain.1;
            let mix = self.applied[param::MIX];
            let output = self.applied[param::OUTPUT];
            let post = self.applied[param::POST_LP_ENABLED];
            for c in 0..ch {
                let mut processed = classic[c];
                if modern > 0.0 {
                    let shaped = (qualities[quality::X1] * x1[c]
                        + qualities[quality::X2] * x2[c]
                        + qualities[quality::X4] * x4[c]) as f64;
                    let low = &mut self.tone_low[c];
                    *low = shaped * (1.0 - tone_beta) + *low * tone_beta;
                    let toned = if tone == 0.5 {
                        shaped
                    } else {
                        *low * low_gain + (shaped - *low) / low_gain
                    };
                    processed += modern * dry[c] * (1.0 - amount) + toned as f32 * amount;
                }
                let processed = if post > 0.0 {
                    let filtered = finite(self.post_lp.process(processed, c));
                    processed * (1.0 - post) + filtered * post
                } else {
                    processed
                };
                frame[c] = finite((dry[c] * (1.0 - mix) + processed * mix) * output);
            }
        }
    }

    fn reset(&mut self) {
        let sample_rate = self.sample_rate;
        let params = self.params;
        *self = Self::new(sample_rate);
        for (index, value) in params.into_iter().enumerate() {
            self.edit(index, value, true);
        }
    }
}

#[cfg(test)]
#[path = "drive_tests.rs"]
mod tests;
