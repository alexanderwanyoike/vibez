//! Echo: a tape-style delay whose repeats wander, wear and smear as tape
//! does. Moving its time glides the play head, so repeats bend in pitch
//! rather than click; wow and flutter move the head while it plays; and
//! saturation, wear and diffusion act inside the feedback loop, so each
//! repeat carries more of them than the last.
//!
//! References:
//! - S. Arnardottir, J. S. Abel, J. O. Smith, "A Digital Model of the
//!   Echoplex Tape Delay", AES 125th Convention (2008): a read head whose
//!   position wanders with wow and flutter, and the saturation and losses
//!   met on each pass through the loop.
//! - O. Niemitalo, "Polynomial Interpolators for High-Quality Resampling of
//!   Oversampled Audio" (2001): the 4-point cubic Hermite read used for the
//!   moving head.
//! - M. R. Schroeder, B. F. Logan, "Colorless Artificial Reverberation",
//!   JAES 9(3) (1961): the allpass sections that diffuse the repeats.

use vibez_core::effect::{EffectType, ParamDescriptor};

use crate::delay::duck::Duck;
use crate::delay::filter::FeedbackFilters;
use crate::delay::ramp::ScalarRamp;
use crate::delay::synced_beats;
use crate::effect::AudioEffect;

pub mod param {
    pub const TIME_MS: usize = 0;
    pub const SYNC: usize = 1;
    /// A synced time, as a place in [`super::GRIDS`].
    pub const GRID: usize = 2;
    pub const FEEDBACK: usize = 3;
    pub const MIX: usize = 4;
    pub const MODE: usize = 5;
    /// How much of the chosen style the repeats carry.
    pub const CHARACTER: usize = 6;
    pub const TONE: usize = 7;
    pub const STYLE: usize = 8;
    pub const WEAR: usize = 9;
    pub const WOW: usize = 10;
    pub const FLUTTER: usize = 11;
    pub const SATURATION: usize = 12;
    pub const DIFFUSION: usize = 13;
    pub const DUCK: usize = 14;
    pub const LOW_CUT: usize = 15;
    pub const HIGH_CUT: usize = 16;
    pub const COUNT: usize = 17;
}

/// Every synced time as `synced_beats`' division and modifier, shortest
/// first, so turning the time up always lengthens it.
pub const GRIDS: [(usize, usize); 21] = [
    (0, 2),
    (0, 0),
    (1, 2),
    (0, 1),
    (1, 0),
    (2, 2),
    (1, 1),
    (2, 0),
    (3, 2),
    (2, 1),
    (3, 0),
    (4, 2),
    (3, 1),
    (4, 0),
    (5, 2),
    (4, 1),
    (5, 0),
    (6, 2),
    (5, 1),
    (6, 0),
    (6, 1),
];

/// The dotted quarter, Echo's starting time.
const DOTTED_QUARTER: f32 = 15.0;

/// The Tape Character choices.
pub mod style {
    pub const CLEAN: usize = 0;
    pub const TAPE: usize = 1;
    pub const DIFFUSE: usize = 2;
}

pub mod mode {
    pub const STEREO: usize = 0;
    pub const PING_PONG: usize = 1;
}

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

pub static ECHO_PARAMS: &[ParamDescriptor] = &[
    descriptor("Time", 1.0, 2000.0, 375.0, "ms"),
    descriptor("Sync", 0.0, 1.0, 1.0, ""),
    descriptor("Grid", 0.0, 20.0, DOTTED_QUARTER, ""),
    descriptor("Feedback", 0.0, 0.98, 0.48, ""),
    descriptor("Mix", 0.0, 1.0, 0.25, ""),
    descriptor("Mode", 0.0, 1.0, 0.0, ""),
    descriptor("Character", 0.0, 1.0, 0.35, ""),
    descriptor("Tone", 0.0, 1.0, 0.55, ""),
    descriptor("Tape Character", 0.0, 2.0, 1.0, ""),
    descriptor("Wear", 0.0, 1.0, 0.15, ""),
    descriptor("Wow", 0.0, 1.0, 0.08, ""),
    descriptor("Flutter", 0.0, 1.0, 0.04, ""),
    descriptor("Saturation", 0.0, 1.0, 0.12, ""),
    descriptor("Diffusion", 0.0, 1.0, 0.18, ""),
    descriptor("Duck", 0.0, 1.0, 0.2, ""),
    descriptor("Low Cut", 20.0, 2_000.0, 180.0, "Hz"),
    descriptor("High Cut", 1_000.0, 20_000.0, 6_200.0, "Hz"),
];

/// Seconds of tape the longest echo may need.
const MAX_SECONDS: f32 = 6.0;
/// How long the play head takes to settle on a new time, as a tape
/// transport does.
const GLIDE_SECONDS: f64 = 0.12;
/// Wow and flutter at full depth, in ms of head travel, and their rates.
const WOW_MS: f64 = 4.0;
const WOW_HZ: f64 = 0.28;
const DRIFT_HZ: f64 = 0.17;
const FLUTTER_MS: f64 = 0.35;
const FLUTTER_HZ: f64 = 6.3;
/// Frames between fresh wow and flutter positions; at their rates of a few
/// hertz a straight line between them is exact enough.
const MOTION_FRAMES: u32 = 16;
/// The allpass sections' delays, mutually prime so their echoes do not
/// line up.
const DIFFUSER_MS: [f32; 4] = [4.7, 3.6, 2.9, 1.9];

/// Each style's weight on wear, wow, flutter, saturation and diffusion.
fn style_weights(style: usize) -> [f32; 5] {
    match style {
        style::CLEAN => [0.0; 5],
        style::DIFFUSE => [0.5, 0.5, 0.5, 0.5, 1.0],
        _ => [1.0; 5],
    }
}

fn value(params: &[f32], index: usize) -> f32 {
    params
        .get(index)
        .copied()
        .unwrap_or(ECHO_PARAMS[index].default)
}

/// Milliseconds between repeats at `bpm`.
pub fn echo_time_ms(params: &[f32], bpm: f64) -> f64 {
    if value(params, param::SYNC) >= 0.5 {
        let (division, modifier) =
            GRIDS[(value(params, param::GRID).round().max(0.0) as usize).min(GRIDS.len() - 1)];
        60_000.0 / bpm.max(1.0) * synced_beats(division, modifier)
    } else {
        f64::from(value(params, param::TIME_MS))
    }
    .min(f64::from(MAX_SECONDS) * 1000.0 - 50.0)
}

/// What the style and Character leave of an advanced control.
fn effective(params: &[f32], index: usize) -> f32 {
    let weights = style_weights(value(params, param::STYLE).round() as usize);
    let weight = match index {
        param::WEAR => weights[0],
        param::WOW => weights[1],
        param::FLUTTER => weights[2],
        param::SATURATION => weights[3],
        _ => weights[4],
    };
    let amount = value(params, index) * weight * value(params, param::CHARACTER) * 2.0;
    if index == param::DIFFUSION && value(params, param::STYLE).round() as usize == style::DIFFUSE {
        // Diffuse always smears, however low its own control.
        amount.max(0.6 * value(params, param::CHARACTER) * 2.0)
    } else {
        amount
    }
    .min(2.0)
}

/// The level of each of the first `count` repeats in dB, the first at 0 dB,
/// as the loop's feedback and wear leave them.
pub fn echo_taps_db(params: &[f32], count: usize) -> Vec<f32> {
    let feedback = value(params, param::FEEDBACK).max(1e-6);
    let wear = 1.0 - 0.12 * effective(params, param::WEAR).min(1.0);
    (0..count)
        .map(|repeat| 20.0 * (feedback * wear).powi(repeat as i32).log10())
        .collect()
}

/// How far wow and flutter move the play head `seconds` in, as a share of
/// their full depth, for the card's plot.
pub fn head_motion(params: &[f32], seconds: f64) -> f32 {
    let wow = f64::from(effective(params, param::WOW));
    let flutter = f64::from(effective(params, param::FLUTTER));
    let travel = wow
        * WOW_MS
        * (0.75 * (std::f64::consts::TAU * WOW_HZ * seconds).sin()
            + 0.25 * (std::f64::consts::TAU * DRIFT_HZ * seconds).sin())
        + flutter * FLUTTER_MS * (std::f64::consts::TAU * FLUTTER_HZ * seconds).sin();
    (travel / (WOW_MS + FLUTTER_MS)) as f32
}

/// One channel of the tape: a ring the record head writes and the play head
/// reads at any fraction of a frame.
struct Tape {
    samples: Vec<f32>,
    write: usize,
}

impl Tape {
    fn new(frames: usize) -> Self {
        Self {
            samples: vec![0.0; frames],
            write: 0,
        }
    }

    /// The sample `delay` frames behind the record head, `delay` at least
    /// one frame and short of the tape's length.
    fn read(&self, delay: f64) -> f32 {
        let length = self.samples.len();
        let whole = delay.ceil();
        let t = (whole - delay) as f32;
        // `whole` frames back, wrapped without dividing.
        let mut index = self.write + length - whole as usize;
        if index >= length {
            index -= length;
        }
        let step = |index: usize| if index + 1 == length { 0 } else { index + 1 };
        let before = if index == 0 { length - 1 } else { index - 1 };
        let (y0, y1) = (self.samples[before], self.samples[index]);
        let next = step(index);
        let (y2, y3) = (self.samples[next], self.samples[step(next)]);
        let c1 = 0.5 * (y2 - y0);
        let c2 = y0 - 2.5 * y1 + 2.0 * y2 - 0.5 * y3;
        let c3 = 0.5 * (y3 - y0) + 1.5 * (y1 - y2);
        ((c3 * t + c2) * t + c1) * t + y1
    }

    fn write(&mut self, sample: f32) {
        self.samples[self.write] = sample;
        self.write += 1;
        if self.write == self.samples.len() {
            self.write = 0;
        }
    }

    fn clear(&mut self) {
        self.samples.iter_mut().for_each(|sample| *sample = 0.0);
        self.write = 0;
    }
}

/// A tanh-shaped clip, a 3/2 Padé fit that meets 1 at 3 and stays there:
/// within 2% of tanh, and cheap enough to run on every repeat.
fn soft_clip(x: f32) -> f32 {
    let x = x.clamp(-3.0, 3.0);
    x * (27.0 + x * x) / (27.0 + 9.0 * x * x)
}

/// A Schroeder allpass section.
struct Allpass {
    buffer: Vec<f32>,
    position: usize,
}

impl Allpass {
    fn new(frames: usize) -> Self {
        Self {
            buffer: vec![0.0; frames.max(1)],
            position: 0,
        }
    }

    fn process(&mut self, input: f32, gain: f32) -> f32 {
        let delayed = self.buffer[self.position];
        let stored = input + delayed * gain;
        // A silent loop settles to zero rather than crawling through
        // denormal numbers.
        self.buffer[self.position] = if stored.abs() < 1e-20 { 0.0 } else { stored };
        self.position += 1;
        if self.position == self.buffer.len() {
            self.position = 0;
        }
        delayed - stored * gain
    }

    fn clear(&mut self) {
        self.buffer.iter_mut().for_each(|sample| *sample = 0.0);
        self.position = 0;
    }
}

/// A repeatable noise for tape hiss.
struct Hiss(u32);

impl Hiss {
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        self.0 as f32 / u32::MAX as f32 * 2.0 - 1.0
    }
}

pub struct EchoEffect {
    sample_rate: f32,
    valid: bool,
    targets: [f32; param::COUNT],
    bpm: f64,
    tapes: [Tape; 2],
    diffusers: [[Allpass; 4]; 2],
    /// Where the play head is, in frames behind the record head.
    head: Option<f64>,
    glide: f64,
    wow_phase: f64,
    drift_phase: f64,
    flutter_phase: f64,
    /// Head travel now, its step per frame, and frames to the next fresh one.
    travel: f64,
    travel_step: f64,
    travel_left: u32,
    mix: ScalarRamp,
    feedback: ScalarRamp,
    routing: ScalarRamp,
    filters: FeedbackFilters,
    duck: Duck,
    /// The wet path's Tone low-pass and the loop's wear low-pass, per side.
    tone_state: [f32; 2],
    wear_state: [f32; 2],
    hiss: Hiss,
}

impl EchoEffect {
    pub fn new(sample_rate: f32) -> Self {
        let valid = sample_rate.is_finite() && (1_000.0..=768_000.0).contains(&sample_rate);
        let rate = if valid { sample_rate } else { 48_000.0 };
        let frames = if valid {
            (rate * MAX_SECONDS) as usize + 8
        } else {
            8
        };
        let diffuser = || {
            DIFFUSER_MS.map(|ms| {
                Allpass::new(if valid {
                    (ms / 1000.0 * rate) as usize
                } else {
                    1
                })
            })
        };
        let targets: [f32; param::COUNT] = std::array::from_fn(|index| ECHO_PARAMS[index].default);
        let mut filters = FeedbackFilters::new(
            rate,
            ECHO_PARAMS[param::LOW_CUT].default,
            ECHO_PARAMS[param::HIGH_CUT].default,
        );
        // The loop's cuts are always in.
        filters.reset(1.0, 1.0);
        let mut echo = Self {
            sample_rate: rate,
            valid,
            targets,
            bpm: 120.0,
            tapes: [Tape::new(frames), Tape::new(frames)],
            diffusers: [diffuser(), diffuser()],
            head: None,
            glide: 1.0 - (-1.0 / (GLIDE_SECONDS * f64::from(rate))).exp(),
            wow_phase: 0.0,
            drift_phase: 0.0,
            flutter_phase: 0.0,
            travel: 0.0,
            travel_step: 0.0,
            travel_left: 0,
            mix: ScalarRamp::new(targets[param::MIX]),
            feedback: ScalarRamp::new(targets[param::FEEDBACK]),
            routing: ScalarRamp::new(targets[param::MODE]),
            filters,
            duck: Duck::new(rate),
            tone_state: [0.0; 2],
            wear_state: [0.0; 2],
            hiss: Hiss(0x2545_f491),
        };
        echo.duck.reset(targets[param::DUCK]);
        echo
    }

    fn ramp_frames(&self) -> usize {
        (self.sample_rate as usize).div_ceil(100)
    }

    fn set_target(&mut self, index: usize, value: f32, automated: bool) -> bool {
        let Some(descriptor) = ECHO_PARAMS.get(index) else {
            return false;
        };
        if !value.is_finite() {
            return false;
        }
        let target = match index {
            param::SYNC | param::MODE => f32::from(value >= 0.5),
            param::GRID | param::STYLE => value.round(),
            _ => value,
        }
        .clamp(descriptor.min, descriptor.max);
        self.targets[index] = target;
        let frames = self.ramp_frames();
        match index {
            param::MIX => self.mix.edit(target, automated, frames),
            param::FEEDBACK => self.feedback.edit(target, automated, frames),
            param::MODE => self.routing.edit(target, automated, frames * 2),
            param::DUCK => self.duck.edit(target, automated, frames),
            param::LOW_CUT => self.filters.set_high_pass_hz(target, automated),
            param::HIGH_CUT => self.filters.set_low_pass_hz(target, automated),
            _ => {}
        }
        true
    }

    /// A one-pole low-pass's coefficient for `hertz`.
    fn one_pole(&self, hertz: f32) -> f32 {
        1.0 - (-std::f32::consts::TAU * hertz / self.sample_rate).exp()
    }
}

impl AudioEffect for EchoEffect {
    fn supports_level_meter(&self) -> bool {
        true
    }

    fn set_audio_context(&mut self, context: vibez_core::audio_context::DeviceAudioContext) {
        self.bpm = context.bpm;
    }

    fn activation_sample_rate(&self) -> Option<u32> {
        self.valid.then_some(self.sample_rate as u32)
    }

    fn processing_configuration_valid(&self) -> bool {
        self.valid
    }

    fn effect_type(&self) -> EffectType {
        EffectType::Echo
    }

    fn param_descriptors(&self) -> &'static [ParamDescriptor] {
        ECHO_PARAMS
    }

    fn set_param(&mut self, index: usize, value: f32) -> bool {
        self.set_target(index, value, false)
    }

    fn set_automated_param(&mut self, index: usize, value: f32) -> bool {
        self.set_target(index, value, true)
    }

    fn get_param(&self, index: usize) -> f32 {
        self.targets.get(index).copied().unwrap_or(0.0)
    }

    fn process(&mut self, buffer: &mut [f32], channels: usize) {
        if channels == 0 {
            return;
        }
        if !self.valid {
            buffer
                .iter_mut()
                .filter(|sample| !sample.is_finite())
                .for_each(|sample| *sample = 0.0);
            return;
        }
        let params = self.targets;
        let rate = f64::from(self.sample_rate);
        let target_head = echo_time_ms(&params, self.bpm) / 1000.0 * rate;
        let wow = f64::from(effective(&params, param::WOW)) * WOW_MS / 1000.0 * rate;
        let flutter = f64::from(effective(&params, param::FLUTTER)) * FLUTTER_MS / 1000.0 * rate;
        let wear = effective(&params, param::WEAR);
        let saturation = effective(&params, param::SATURATION);
        let diffusion = (0.6 * effective(&params, param::DIFFUSION)).min(0.75);
        let drive = 1.0 + 5.0 * saturation;
        let tone = self.one_pole(600.0 * 30f32.powf(params[param::TONE]));
        let worn = self.one_pole(18_000.0 * (1.0 - 0.75 * wear.min(1.0)));
        let hiss_level = 0.0008 * wear;
        let span = f64::from(MOTION_FRAMES);
        let (wow_step, drift_step, flutter_step) = (
            WOW_HZ / rate * span,
            DRIFT_HZ / rate * span,
            FLUTTER_HZ / rate * span,
        );
        let max_head = (self.tapes[0].samples.len() - 4) as f64;

        for frame in buffer.chunks_exact_mut(channels) {
            let head = self.head.get_or_insert(target_head);
            *head += (target_head - *head) * self.glide;
            if self.travel_left == 0 {
                self.wow_phase = (self.wow_phase + wow_step).fract();
                self.drift_phase = (self.drift_phase + drift_step).fract();
                self.flutter_phase = (self.flutter_phase + flutter_step).fract();
                let next = wow
                    * (0.75 * (std::f64::consts::TAU * self.wow_phase).sin()
                        + 0.25 * (std::f64::consts::TAU * self.drift_phase).sin())
                    + flutter * (std::f64::consts::TAU * self.flutter_phase).sin();
                self.travel_step = (next - self.travel) / span;
                self.travel_left = MOTION_FRAMES;
            }
            self.travel += self.travel_step;
            self.travel_left -= 1;
            let at = (*head + self.travel).clamp(1.0, max_head);

            let played = [self.tapes[0].read(at), self.tapes[1].read(at)];
            let dry_left = if frame[0].is_finite() { frame[0] } else { 0.0 };
            let dry_right = match frame.get(1) {
                Some(sample) if sample.is_finite() => *sample,
                Some(_) => 0.0,
                None => dry_left,
            };
            let dry = [dry_left, dry_right];

            // The loop: cuts, wear, saturation, diffusion, then routing.
            let mut looped = self.filters.process(played);
            for (side, looped) in looped.iter_mut().enumerate() {
                let mut sample = *looped;
                if wear > 0.0 {
                    self.wear_state[side] += worn * (sample - self.wear_state[side]);
                    sample = self.wear_state[side];
                }
                if saturation > 0.0 {
                    sample = soft_clip(sample * drive) / drive;
                }
                if diffusion > 0.0 {
                    for allpass in &mut self.diffusers[side] {
                        sample = allpass.process(sample, diffusion);
                    }
                }
                *looped = sample;
            }
            let feedback = self.feedback.next_frame();
            let cross = self.routing.next_frame();
            // Ping-pong records the input on the left only and swaps sides
            // each pass.
            let input = [
                dry[0] * (1.0 - cross) + (dry[0] + dry[1]) * 0.5 * cross,
                dry[1] * (1.0 - cross),
            ];
            let returned = [
                looped[0] * (1.0 - cross) + looped[1] * cross,
                looped[1] * (1.0 - cross) + looped[0] * cross,
            ];
            for side in 0..2 {
                let hiss = self.hiss.next() * hiss_level;
                let record = input[side] + returned[side] * feedback + hiss;
                self.tapes[side].write(if record.is_finite() { record } else { 0.0 });
            }

            for (state, played) in self.tone_state.iter_mut().zip(played) {
                *state += tone * (played - *state);
                if state.abs() < 1e-20 {
                    *state = 0.0;
                }
            }
            let wet = self.duck.process(dry, self.tone_state);
            let mix = self.mix.next_frame();
            frame[0] = dry[0] * (1.0 - mix) + wet[0] * mix;
            if let Some(right) = frame.get_mut(1) {
                *right = dry[1] * (1.0 - mix) + wet[1] * mix;
            }
        }
    }

    fn reset(&mut self) {
        self.tapes.iter_mut().for_each(Tape::clear);
        self.diffusers.iter_mut().flatten().for_each(Allpass::clear);
        self.head = None;
        self.wow_phase = 0.0;
        self.drift_phase = 0.0;
        self.flutter_phase = 0.0;
        self.travel = 0.0;
        self.travel_step = 0.0;
        self.travel_left = 0;
        self.mix.reset(self.targets[param::MIX]);
        self.feedback.reset(self.targets[param::FEEDBACK]);
        self.routing.reset(self.targets[param::MODE]);
        self.filters.reset(1.0, 1.0);
        self.duck.reset(self.targets[param::DUCK]);
        self.tone_state = [0.0; 2];
        self.wear_state = [0.0; 2];
    }
}

#[cfg(test)]
#[path = "echo_tests.rs"]
mod tests;
