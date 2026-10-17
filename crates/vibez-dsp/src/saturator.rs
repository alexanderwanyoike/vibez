//! Saturator: a waveshaper with a choice of curve, shaped by Curve Amount,
//! leaned by Bias and Asymmetry for even harmonics, run at 1x, 2x or 4x
//! through the halfband stages Drive uses, with filters either side, DC
//! removal and gain that can follow the drive.
//!
//! Every curve is drawn into a table whenever its settings change, so the
//! oversampled path reads the table rather than computing powers and logs
//! per sample; a new table fades in over [`RAMP_FRAMES`] so a change never
//! clicks.
//!
//! References:
//! - Soft is the knee family x / (1 + |x|^k)^(1/k), from J. O. Smith,
//!   "Physical Audio Signal Processing", "Soft Clipping".
//! - Tube follows the even-harmonic leaning tanh of D. T. Yeh, J. S. Abel,
//!   J. O. Smith, "Simplified, Physically-Informed Models of Distortion and
//!   Overdrive Guitar Effects Pedals", DAFx-07.
//! - Fold is the sine wavefolder of the West Coast tradition, as in
//!   F. Esqueda, H. Pontynen, V. Valimaki, J. D. Parker, "Virtual Analog
//!   Buchla 259 Wavefolder", DAFx-17.

use vibez_core::effect::{EffectType, ParamDescriptor};

use crate::butterworth_filter::ButterworthFilter;
use crate::effect::AudioEffect;
use crate::oversampled::{quality, rates, Delay, Fade, Path, LATENCY, X2_PAD};
pub use crate::oversampled::{LATENCY_SAMPLES, RAMP_FRAMES};

pub mod param {
    pub const DRIVE_DB: usize = 0;
    pub const BIAS: usize = 1;
    pub const OUTPUT_DB: usize = 2;
    pub const MIX: usize = 3;
    pub const AUTO_GAIN: usize = 4;
    pub const SHAPE: usize = 5;
    pub const CURVE: usize = 6;
    pub const ASYMMETRY: usize = 7;
    pub const PRE_FILTER: usize = 8;
    pub const POST_FILTER: usize = 9;
    pub const DC_REMOVAL: usize = 10;
    pub const QUALITY: usize = 11;
    /// Custom's output, in dB, for driven input at -18, 0 and +18 dB.
    pub const CUSTOM_LOW: usize = 12;
    pub const CUSTOM_MID: usize = 13;
    pub const CUSTOM_HIGH: usize = 14;
    pub const COUNT: usize = 15;
}

pub mod shape {
    pub const SOFT: usize = 0;
    pub const TUBE: usize = 1;
    pub const FOLD: usize = 2;
    pub const CUSTOM: usize = 3;
}

/// The Pre Filter's high-pass choices, Off first.
pub const PRE_FILTER_HZ: [f32; 5] = [0.0, 20.0, 40.0, 80.0, 160.0];
/// The Post Filter's low-pass choices, Off first.
pub const POST_FILTER_HZ: [f32; 6] = [0.0, 20_000.0, 16_000.0, 12_000.0, 8_000.0, 4_000.0];
/// Custom's three points, as driven input in dB.
pub const CUSTOM_INPUT_DB: [f32; 3] = [-18.0, 0.0, 18.0];

const fn descriptor(
    name: &'static str,
    min: f32,
    max: f32,
    default: f32,
    unit: &'static str,
) -> ParamDescriptor {
    ParamDescriptor {
        name,
        min,
        max,
        default,
        unit,
    }
}

pub static SATURATOR_PARAMS: &[ParamDescriptor] = &[
    descriptor("Drive", 0.0, 36.0, 5.0, "dB"),
    descriptor("Bias", -1.0, 1.0, 0.0, ""),
    descriptor("Output", -24.0, 12.0, -2.0, "dB"),
    descriptor("Mix", 0.0, 1.0, 1.0, ""),
    descriptor("Auto Gain", 0.0, 1.0, 0.0, ""),
    descriptor("Shape", 0.0, 3.0, 0.0, ""),
    descriptor("Curve Amount", 0.0, 1.0, 0.75, ""),
    descriptor("Asymmetry", -1.0, 1.0, 0.0, ""),
    descriptor("Pre Filter", 0.0, 4.0, 1.0, ""),
    descriptor("Post Filter", 0.0, 5.0, 1.0, ""),
    descriptor("DC Removal", 0.0, 1.0, 1.0, ""),
    descriptor("Quality", 0.0, 2.0, 2.0, ""),
    descriptor("Custom Low", -36.0, 0.0, -17.0, "dB"),
    descriptor("Custom Mid", -12.0, 12.0, -1.0, "dB"),
    descriptor("Custom High", -6.0, 24.0, 6.0, "dB"),
];

/// The table spans driven input of +-TABLE_REACH; beyond it the curve's
/// end values hold.
const TABLE_REACH: f32 = 64.0;
const TABLE_POINTS: usize = 8_193;
/// How far Bias moves the signal into the curve at full turn.
const BIAS_REACH: f32 = 0.5;
/// How much Asymmetry lifts one half of the wave and lowers the other.
const ASYMMETRY_REACH: f32 = 0.5;
/// The level Auto Gain matches, a sine at -12 dBFS before the drive.
const AUTO_GAIN_LEVEL: f32 = 0.25;
const DC_HZ: f32 = 5.0;

fn value(params: &[f32], index: usize) -> f32 {
    params
        .get(index)
        .copied()
        .unwrap_or(SATURATOR_PARAMS[index].default)
}

fn db_to_gain(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

/// Custom's curve: Hermite through its three points in dB, a straight line
/// below the first and level beyond the last.
fn custom(points: [f32; 3], x: f32) -> f32 {
    let magnitude = x.abs();
    if magnitude < 1e-9 {
        return 0.0;
    }
    let input = 20.0 * magnitude.log10();
    let [a, b, c] = CUSTOM_INPUT_DB;
    let output = if input <= a {
        points[0] + (input - a)
    } else if input >= c {
        points[2]
    } else {
        let (x0, x1, y0, y1, m0, m1) = if input < b {
            let middle = (points[2] - points[0]) / (c - a);
            (a, b, points[0], points[1], 1.0, middle)
        } else {
            let middle = (points[2] - points[0]) / (c - a);
            (b, c, points[1], points[2], middle, 0.0)
        };
        let span = x1 - x0;
        let t = (input - x0) / span;
        let (t2, t3) = (t * t, t * t * t);
        (2.0 * t3 - 3.0 * t2 + 1.0) * y0
            + (t3 - 2.0 * t2 + t) * span * m0
            + (-2.0 * t3 + 3.0 * t2) * y1
            + (t3 - t2) * span * m1
    };
    db_to_gain(output).copysign(x)
}

/// A shape's memoryless curve at `x`, the driven input, before any table.
pub fn curve(shape: usize, amount: f32, points: [f32; 3], x: f32) -> f32 {
    match shape {
        shape::TUBE => {
            let lean = 0.3 * amount;
            (x + lean * x.tanh() * x.tanh()).tanh()
        }
        shape::FOLD => (std::f32::consts::FRAC_PI_2 * (1.0 + 2.0 * amount) * x).sin(),
        shape::CUSTOM => custom(points, x),
        _ => {
            let k = 1.0 + 3.0 * amount;
            x / (1.0 + x.abs().powf(k)).powf(1.0 / k)
        }
    }
}

/// The driven signal as it meets the curve, `asymmetry` and `bias` already
/// scaled to their reach.
fn lean(x: f32, asymmetry: f32, bias: f32) -> f32 {
    let half = if x >= 0.0 {
        1.0 + asymmetry
    } else {
        1.0 - asymmetry
    };
    x * half + bias
}

fn lean_of(params: &[f32], x: f32) -> f32 {
    lean(
        x,
        value(params, param::ASYMMETRY) * ASYMMETRY_REACH,
        value(params, param::BIAS) * BIAS_REACH,
    )
}

fn points(params: &[f32]) -> [f32; 3] {
    [
        value(params, param::CUSTOM_LOW),
        value(params, param::CUSTOM_MID),
        value(params, param::CUSTOM_HIGH),
    ]
}

/// The level Auto Gain adds: what brings a -12 dBFS sine back to its own
/// level through the drive and curve.
pub fn auto_gain(params: &[f32]) -> f32 {
    let drive = db_to_gain(value(params, param::DRIVE_DB));
    let shape = value(params, param::SHAPE).round() as usize;
    let amount = value(params, param::CURVE);
    let points = points(params);
    let steps = 256;
    let (mut input, mut sum, mut squares) = (0.0f32, 0.0f32, 0.0f32);
    for step in 0..steps {
        let sine = (std::f32::consts::TAU * step as f32 / steps as f32).sin() * AUTO_GAIN_LEVEL;
        input += sine * sine;
        let out = curve(shape, amount, points, lean_of(params, sine * drive));
        sum += out;
        squares += out * out;
    }
    // The offset Bias leaves is not loudness.
    let mean = sum / steps as f32;
    let output = squares - mean * sum;
    if output <= 1e-12 {
        return 1.0;
    }
    (input / output).sqrt().clamp(0.05, 20.0)
}

/// What these settings map a steady `input` to, leaving out the filters,
/// DC removal and oversampling, which have memory.
pub fn transfer(params: &[f32], input: f32) -> f32 {
    let drive = db_to_gain(value(params, param::DRIVE_DB));
    let shaped = curve(
        value(params, param::SHAPE).round() as usize,
        value(params, param::CURVE),
        points(params),
        lean_of(params, input * drive),
    );
    let gain = db_to_gain(value(params, param::OUTPUT_DB))
        * if value(params, param::AUTO_GAIN) >= 0.5 {
            auto_gain(params)
        } else {
            1.0
        };
    let mix = value(params, param::MIX);
    (input * (1.0 - mix) + shaped * gain * mix).clamp(-64.0, 64.0)
}

/// The level of each of the first `count` harmonics, in dB against the
/// input's fundamental, when a -6 dBFS sine goes through the curve.
pub fn harmonics_db(params: &[f32], count: usize) -> Vec<f32> {
    let steps = 1024;
    let level = 0.5;
    let wave: Vec<f32> = (0..steps)
        .map(|step| {
            transfer(
                params,
                (std::f32::consts::TAU * step as f32 / steps as f32).sin() * level,
            )
        })
        .collect();
    (1..=count)
        .map(|harmonic| {
            let (mut re, mut im) = (0.0f32, 0.0f32);
            for (step, sample) in wave.iter().enumerate() {
                let phase = std::f32::consts::TAU * (harmonic * step) as f32 / steps as f32;
                re += sample * phase.cos();
                im += sample * phase.sin();
            }
            let amplitude = 2.0 * (re * re + im * im).sqrt() / steps as f32;
            20.0 * (amplitude / level).max(1e-9).log10()
        })
        .collect()
}

/// A curve sampled across the driven range, read with a straight line
/// between points.
struct Table {
    points: Vec<f32>,
}

impl Table {
    fn new() -> Self {
        Self {
            points: vec![0.0; TABLE_POINTS],
        }
    }

    fn draw(&mut self, shape: usize, amount: f32, custom: [f32; 3]) {
        for (index, point) in self.points.iter_mut().enumerate() {
            let x = (index as f32 / (TABLE_POINTS - 1) as f32 * 2.0 - 1.0) * TABLE_REACH;
            *point = curve(shape, amount, custom, x);
        }
    }

    /// Becomes what is heard `blend` of the way from itself to `toward`.
    fn blend_toward(&mut self, toward: &Table, blend: f32) {
        for (point, target) in self.points.iter_mut().zip(&toward.points) {
            *point += blend * (target - *point);
        }
    }

    fn read(&self, x: f32) -> f32 {
        let position = ((x / TABLE_REACH + 1.0) * 0.5 * (TABLE_POINTS - 1) as f32)
            .clamp(0.0, (TABLE_POINTS - 1) as f32);
        let index = (position as usize).min(TABLE_POINTS - 2);
        let t = position - index as f32;
        self.points[index] + t * (self.points[index + 1] - self.points[index])
    }
}

/// A control easing to its target over [`RAMP_FRAMES`].
#[derive(Clone, Copy)]
struct Ramp {
    now: f32,
    target: f32,
    step: f32,
    left: u32,
}

impl Ramp {
    fn at(value: f32) -> Self {
        Self {
            now: value,
            target: value,
            step: 0.0,
            left: 0,
        }
    }

    fn toward(&mut self, target: f32) {
        self.target = target;
        self.step = (target - self.now) / RAMP_FRAMES as f32;
        self.left = RAMP_FRAMES;
    }

    fn settle(&mut self) {
        *self = Self::at(self.target);
    }

    fn next(&mut self) -> f32 {
        if self.left > 0 {
            self.left -= 1;
            self.now = if self.left == 0 {
                self.target
            } else {
                self.now + self.step
            };
        }
        self.now
    }
}

fn finite(sample: f32) -> f32 {
    if sample.is_finite() {
        sample
    } else {
        0.0
    }
}

pub struct SaturatorEffect {
    sample_rate: f32,
    params: [f32; param::COUNT],
    started: bool,
    drive: Ramp,
    bias: Ramp,
    asymmetry: Ramp,
    /// Output and Auto Gain together, worked out on each edit since
    /// [`auto_gain`] is too costly per block.
    gain: Ramp,
    mix: Ramp,
    /// The curve in use, the one it replaces, and how far the new one has
    /// faded in.
    table: Table,
    previous: Table,
    blend: Ramp,
    quality: Fade<{ quality::COUNT }>,
    paths: [Path; 2],
    pre: ButterworthFilter,
    post: ButterworthFilter,
    dc_state: [f32; 2],
    dc_input: [f32; 2],
    dc_coefficient: f32,
    dry: Delay<LATENCY>,
    x1: Delay<LATENCY>,
    x2: Delay<X2_PAD>,
}

impl SaturatorEffect {
    pub fn new(sample_rate: f32) -> Self {
        let sample_rate = if sample_rate.is_finite() && sample_rate >= 2.0 {
            sample_rate
        } else {
            48_000.0
        };
        let mut saturator = Self {
            sample_rate,
            params: std::array::from_fn(|index| SATURATOR_PARAMS[index].default),
            started: false,
            drive: Ramp::at(0.0),
            bias: Ramp::at(0.0),
            asymmetry: Ramp::at(0.0),
            gain: Ramp::at(1.0),
            mix: Ramp::at(0.0),
            table: Table::new(),
            previous: Table::new(),
            blend: Ramp::at(1.0),
            quality: Fade::at(quality::X4),
            paths: [Path::new(), Path::new()],
            pre: ButterworthFilter::EMPTY,
            post: ButterworthFilter::EMPTY,
            dc_state: [0.0; 2],
            dc_input: [0.0; 2],
            dc_coefficient: (-std::f32::consts::TAU * DC_HZ / sample_rate).exp(),
            dry: Delay::EMPTY,
            x1: Delay::EMPTY,
            x2: Delay::EMPTY,
        };
        for index in 0..param::COUNT {
            saturator.apply(index, true);
        }
        saturator
    }

    fn shape(&self) -> usize {
        self.params[param::SHAPE] as usize
    }

    fn draw(&mut self) {
        let (shape, amount, custom) = (
            self.shape(),
            self.params[param::CURVE],
            points(&self.params),
        );
        self.table.draw(shape, amount, custom);
    }

    fn configure_filters(&mut self) {
        let pre = PRE_FILTER_HZ[self.params[param::PRE_FILTER] as usize];
        if pre > 0.0 {
            self.pre.configure(self.sample_rate, pre, true);
        }
        let post = POST_FILTER_HZ[self.params[param::POST_FILTER] as usize];
        if post > 0.0 {
            self.post
                .configure(self.sample_rate, post.min(self.sample_rate * 0.45), false);
        }
    }

    /// Moves the DSP to `params[index]`, at once when `immediate`, otherwise
    /// over [`RAMP_FRAMES`].
    fn apply(&mut self, index: usize, immediate: bool) {
        let value = self.params[index];
        let ease = |ramp: &mut Ramp, target: f32| {
            if immediate {
                *ramp = Ramp::at(target);
            } else {
                ramp.toward(target);
            }
        };
        match index {
            param::DRIVE_DB => ease(&mut self.drive, db_to_gain(value)),
            param::BIAS => ease(&mut self.bias, value * BIAS_REACH),
            param::ASYMMETRY => ease(&mut self.asymmetry, value * ASYMMETRY_REACH),
            param::MIX => ease(&mut self.mix, value),
            param::QUALITY if immediate => self.quality = Fade::at(value as usize),
            param::QUALITY => self.quality.toward(value as usize),
            param::PRE_FILTER | param::POST_FILTER => self.configure_filters(),
            param::SHAPE
            | param::CURVE
            | param::CUSTOM_LOW
            | param::CUSTOM_MID
            | param::CUSTOM_HIGH => {
                // Automation fades too: a curve stepping each block zippers.
                if self.started {
                    let blend = self.blend.now;
                    if blend >= 1.0 {
                        std::mem::swap(&mut self.table, &mut self.previous);
                    } else {
                        // Mid-fade, the fade restarts from what is heard now.
                        self.previous.blend_toward(&self.table, blend);
                    }
                    self.blend = Ramp::at(0.0);
                    self.blend.toward(1.0);
                }
                self.draw();
            }
            _ => {}
        }
        // Drive and every curve setting move what Auto Gain adds.
        if !matches!(
            index,
            param::MIX
                | param::QUALITY
                | param::PRE_FILTER
                | param::POST_FILTER
                | param::DC_REMOVAL
        ) {
            let gain = db_to_gain(self.params[param::OUTPUT_DB])
                * if self.params[param::AUTO_GAIN] >= 0.5 {
                    auto_gain(&self.params)
                } else {
                    1.0
                };
            ease(&mut self.gain, gain);
        }
    }

    fn edit(&mut self, index: usize, value: f32, automated: bool) -> bool {
        let Some(descriptor) = SATURATOR_PARAMS.get(index) else {
            return false;
        };
        if !value.is_finite() {
            return false;
        }
        let value = match index {
            param::AUTO_GAIN | param::DC_REMOVAL => f32::from(value >= 0.5),
            param::SHAPE | param::PRE_FILTER | param::POST_FILTER | param::QUALITY => value.round(),
            _ => value,
        }
        .clamp(descriptor.min, descriptor.max);
        if self.params[index] == value {
            return true;
        }
        // A filter coming back into use starts from silence, not from
        // whatever it last heard.
        if self.params[index] == 0.0 {
            match index {
                param::PRE_FILTER => self.pre.reset(),
                param::POST_FILTER => self.post.reset(),
                _ => {}
            }
        }
        self.params[index] = value;
        self.apply(index, automated || !self.started);
        true
    }
}

impl AudioEffect for SaturatorEffect {
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
        EffectType::Saturator
    }

    fn param_descriptors(&self) -> &'static [ParamDescriptor] {
        SATURATOR_PARAMS
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
        let pre_on = self.params[param::PRE_FILTER] > 0.0;
        let post_on = self.params[param::POST_FILTER] > 0.0;
        let dc_on = self.params[param::DC_REMOVAL] >= 0.5;
        for frame in buffer.chunks_exact_mut(channels) {
            self.started = true;
            let drive = self.drive.next();
            let bias = self.bias.next();
            let asymmetry = self.asymmetry.next();
            let gain = self.gain.next();
            let mix = self.mix.next();
            let blend = self.blend.next();
            let was = self.quality.weights;
            self.quality.advance();
            // A rate coming back into use starts from silence, not from
            // whatever it last heard.
            for (path, rate) in [quality::X2, quality::X4].into_iter().enumerate() {
                if was[rate] == 0.0 && self.quality.weights[rate] > 0.0 {
                    self.paths[path].reset();
                }
            }
            let qualities = self.quality.weights;
            let (table, previous) = (&self.table, &self.previous);
            let shaped = |x: f32| {
                let x = lean(x, asymmetry, bias);
                if blend < 1.0 {
                    previous.read(x) + blend * (table.read(x) - previous.read(x))
                } else {
                    table.read(x)
                }
            };
            let mut dry = [0.0; 2];
            let mut by_rate = [[0.0; 2]; quality::COUNT];
            for c in 0..ch {
                dry[c] = finite(frame[c]);
                let input = if pre_on {
                    finite(self.pre.process(dry[c], c))
                } else {
                    dry[c]
                };
                let rated = rates(&mut self.paths, c, input * drive, &qualities, shaped);
                for (rate, sample) in rated.into_iter().enumerate() {
                    by_rate[rate][c] = sample;
                }
            }
            let dry = self.dry.push(dry);
            let x1 = self.x1.push(by_rate[quality::X1]);
            let x2 = self.x2.push(by_rate[quality::X2]);
            let x4 = by_rate[quality::X4];
            for c in 0..ch {
                let mut wet = qualities[quality::X1] * x1[c]
                    + qualities[quality::X2] * x2[c]
                    + qualities[quality::X4] * x4[c];
                if dc_on {
                    let blocked = wet - self.dc_input[c] + self.dc_coefficient * self.dc_state[c];
                    self.dc_input[c] = wet;
                    self.dc_state[c] = if blocked.abs() < 1e-20 { 0.0 } else { blocked };
                    wet = blocked;
                }
                if post_on {
                    wet = finite(self.post.process(wet, c));
                }
                frame[c] = finite(dry[c] * (1.0 - mix) + wet * gain * mix);
            }
        }
    }

    fn reset(&mut self) {
        self.paths.iter_mut().for_each(Path::reset);
        self.pre.reset();
        self.post.reset();
        self.dc_state = [0.0; 2];
        self.dc_input = [0.0; 2];
        self.dry = Delay::EMPTY;
        self.x1 = Delay::EMPTY;
        self.x2 = Delay::EMPTY;
        // The settings are unchanged, so only the fades finish; nothing is
        // redrawn or measured again.
        for ramp in [
            &mut self.drive,
            &mut self.bias,
            &mut self.asymmetry,
            &mut self.gain,
            &mut self.mix,
            &mut self.blend,
        ] {
            ramp.settle();
        }
        self.quality = Fade::at(self.params[param::QUALITY] as usize);
    }
}

#[cfg(test)]
#[path = "saturator_tests.rs"]
mod tests;
