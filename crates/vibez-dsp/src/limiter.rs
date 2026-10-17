//! Limiter: a lookahead brickwall that holds the output under its ceiling.
//!
//! Each channel's peak (or, with True Peak on, its peak between samples
//! too) sets the gain it needs to stay under the ceiling. Stereo Link
//! blends the channels' needs, a running minimum holds the deepest need
//! across the lookahead, release lets the gain rise again, and a moving
//! average across the same window turns the drop into a smooth ramp that
//! is fully down by the time the delayed audio reaches the peak. Auto
//! release pairs a fast and a slow envelope so transients recover quickly
//! while sustained limiting does not pump. A clip at the ceiling catches
//! whatever the gain leaves, and the Loud style adds a soft clip ahead of
//! the limiter; with Quality on both clips run at 4x.
//!
//! References:
//! - Lookahead by running minimum then moving average: G. Hughes
//!   (Signalsmith Audio), "Designing a straightforward limiter" (2022).
//! - Release smoothing and program-dependent release: D. Giannoulis,
//!   M. Massberg, J. D. Reiss, "Digital Dynamic Range Compressor Design: A
//!   Tutorial and Analysis", JAES 60(6), 2012.
//! - True peak by 4x interpolation: ITU-R BS.1770-4, Annex 2.

use vibez_core::effect::{DynamicsMeter, EffectType, ParamDescriptor};

use crate::delay::ramp::ScalarRamp;
use crate::effect::AudioEffect;
use crate::oversampled::{quality, rates, Delay, Fade, Path, LATENCY, RAMP_FRAMES};

pub mod param {
    pub const INPUT_DB: usize = 0;
    pub const CEILING_DB: usize = 1;
    /// Milliseconds; below [`super::AUTO_RELEASE_BELOW_MS`] the release
    /// follows the material.
    pub const RELEASE_MS: usize = 2;
    pub const STYLE: usize = 3;
    pub const LOOKAHEAD_MS: usize = 4;
    pub const STEREO_LINK: usize = 5;
    pub const QUALITY: usize = 6;
    pub const TRUE_PEAK: usize = 7;
    pub const COUNT: usize = 8;
}

pub mod style {
    pub const TRANSPARENT: usize = 0;
    pub const PUNCHY: usize = 1;
    pub const LOUD: usize = 2;
    pub const COUNT: usize = 3;
}

/// A Release below this many milliseconds means Auto.
pub const AUTO_RELEASE_BELOW_MS: f32 = 1.0;
/// The audio waits this long whatever the Lookahead, so the latency the
/// engine compensates never changes.
pub const LOOKAHEAD_MAX_MS: f32 = 5.0;

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

pub static LIMITER_PARAMS: &[ParamDescriptor] = &[
    descriptor("Input", -12.0, 24.0, 0.0, "dB"),
    descriptor("Ceiling", -24.0, 0.0, -1.0, "dB"),
    descriptor("Release", 0.0, 500.0, 0.0, "ms"),
    descriptor("Style", 0.0, 2.0, 0.0, ""),
    descriptor("Lookahead", 0.5, LOOKAHEAD_MAX_MS, 2.0, "ms"),
    descriptor("Stereo Link", 0.0, 1.0, 1.0, ""),
    descriptor("Quality", 0.0, 1.0, 1.0, ""),
    descriptor("True Peak", 0.0, 1.0, 1.0, ""),
];

/// Taps per phase of the true-peak interpolator.
const TP_TAPS: usize = 12;
const TP_PHASES: usize = 4;
/// How far behind the input the interpolated peaks lie, in samples.
const TP_DELAY: usize = TP_TAPS / 2;
/// Auto release's two envelopes, and how slowly the slow one engages.
const AUTO_FAST_MS: f32 = 40.0;
const AUTO_SLOW_MS: f32 = 600.0;
const AUTO_SLOW_ATTACK_MS: f32 = 200.0;
/// Punchy halves the attack ramp and the release.
const PUNCHY_SCALE: f32 = 0.5;
/// Loud's soft clip leaves this much headroom above the ceiling for the
/// limiter to take.
const LOUD_HEADROOM: f32 = 1.413;
const GAIN_FLOOR: f32 = 1e-6;

fn value(params: &[f32], index: usize) -> f32 {
    params
        .get(index)
        .copied()
        .unwrap_or(LIMITER_PARAMS[index].default)
}

fn db_to_gain(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

fn gain_to_db(gain: f32) -> f32 {
    20.0 * gain.max(GAIN_FLOOR).log10()
}

/// Whether these settings release automatically.
pub fn auto_release(params: &[f32]) -> bool {
    value(params, param::RELEASE_MS) < AUTO_RELEASE_BELOW_MS
}

/// A Pade approximation of tanh, saturating at 1 from |x| = 3.
fn soft_clip(x: f32) -> f32 {
    let x = x.clamp(-3.0, 3.0);
    x * (27.0 + x * x) / (27.0 + 9.0 * x * x)
}

/// The 4x interpolator's coefficients, phase by phase: a Kaiser-windowed
/// sinc whose phases each pass DC at unity.
fn interpolator() -> [[f32; TP_TAPS]; TP_PHASES] {
    let taps = TP_TAPS * TP_PHASES;
    let centre = (taps - 1) as f64 / 2.0;
    let beta = 8.0;
    let bessel = |x: f64| {
        let (mut sum, mut term) = (1.0, 1.0);
        for k in 1..32 {
            term *= (x / 2.0) / k as f64;
            sum += term * term;
        }
        sum
    };
    let mut phases = [[0.0f32; TP_TAPS]; TP_PHASES];
    for (phase, coefficients) in phases.iter_mut().enumerate() {
        let mut sum = 0.0;
        let mut raw = [0.0f64; TP_TAPS];
        for (tap, value) in raw.iter_mut().enumerate() {
            let m = (tap * TP_PHASES + phase) as f64;
            let t = (m - centre) / TP_PHASES as f64;
            let sinc = if t.abs() < 1e-12 {
                1.0
            } else {
                (std::f64::consts::PI * t).sin() / (std::f64::consts::PI * t)
            };
            let ratio = (m - centre) / centre;
            let window = bessel(beta * (1.0 - ratio * ratio).max(0.0).sqrt()) / bessel(beta);
            *value = sinc * window;
            sum += *value;
        }
        for (coefficient, value) in coefficients.iter_mut().zip(raw) {
            *coefficient = (value / sum) as f32;
        }
    }
    phases
}

/// The smallest of the last `window` values pushed, kept in a ring of
/// candidates so each push costs little.
struct RunningMin {
    values: Vec<f32>,
    times: Vec<u64>,
    head: usize,
    len: usize,
}

impl RunningMin {
    fn new(capacity: usize) -> Self {
        Self {
            values: vec![0.0; capacity + 1],
            times: vec![0; capacity + 1],
            head: 0,
            len: 0,
        }
    }

    fn clear(&mut self) {
        self.head = 0;
        self.len = 0;
    }

    fn push(&mut self, time: u64, value: f32, window: usize) -> f32 {
        let capacity = self.values.len();
        while self.len > 0 {
            let back = (self.head + self.len - 1) % capacity;
            if self.values[back] < value {
                break;
            }
            self.len -= 1;
        }
        let slot = (self.head + self.len) % capacity;
        self.values[slot] = value;
        self.times[slot] = time;
        self.len += 1;
        while time - self.times[self.head] >= window as u64 {
            self.head = (self.head + 1) % capacity;
            self.len -= 1;
        }
        self.values[self.head]
    }
}

/// One channel's gain computer and its histories.
struct Channel {
    /// The input as the interpolator sees it.
    taps: [f32; TP_TAPS],
    tap: usize,
    /// What the channel needed, newest last, for rebuilding the running
    /// minimum when the window changes.
    needs: Vec<f32>,
    held: RunningMin,
    release: f32,
    slow: f32,
    /// What release gave, for the moving average.
    released: Vec<f32>,
    sum: f64,
    /// The smoothed gain, waiting to meet the delayed audio.
    gains: Vec<f32>,
    /// The audio, waiting for its gain.
    audio: Vec<f32>,
}

impl Channel {
    fn new(lookahead_max: usize, audio_delay: usize) -> Self {
        Self {
            taps: [0.0; TP_TAPS],
            tap: 0,
            needs: vec![1.0; lookahead_max],
            held: RunningMin::new(lookahead_max),
            release: 1.0,
            slow: 1.0,
            released: vec![1.0; lookahead_max],
            sum: 0.0,
            gains: vec![1.0; lookahead_max],
            audio: vec![0.0; audio_delay],
        }
    }

    /// The input `TP_DELAY` samples ago, and its peak including what lies
    /// between samples when `true_peak`.
    fn detect(
        &mut self,
        input: f32,
        phases: &[[f32; TP_TAPS]; TP_PHASES],
        true_peak: bool,
    ) -> (f32, f32) {
        self.taps[self.tap] = input;
        self.tap = (self.tap + 1) % TP_TAPS;
        let at = |back: usize| self.taps[(self.tap + TP_TAPS - 1 - back) % TP_TAPS];
        let delayed = at(TP_DELAY);
        let mut peak = delayed.abs().max(at(TP_DELAY - 1).abs());
        if true_peak {
            for coefficients in phases {
                let interpolated: f32 = coefficients
                    .iter()
                    .enumerate()
                    .map(|(back, coefficient)| coefficient * at(back))
                    .sum();
                peak = peak.max(interpolated.abs());
            }
        }
        (delayed, peak)
    }
}

/// Times derived from the settings and sample rate.
#[derive(Clone, Copy)]
struct Timing {
    /// Samples the hold and the moving average span.
    window: usize,
    release: f32,
    fast: f32,
    slow_release: f32,
    slow_attack: f32,
    auto: bool,
}

fn coefficient(ms: f32, sample_rate: f32) -> f32 {
    (-1.0 / (ms.max(0.01) * 0.001 * sample_rate)).exp()
}

pub struct LimiterEffect {
    sample_rate: f32,
    params: [f32; param::COUNT],
    phases: [[f32; TP_TAPS]; TP_PHASES],
    lookahead_max: usize,
    timing: Timing,
    time: u64,
    input: ScalarRamp,
    ceiling: ScalarRamp,
    link: ScalarRamp,
    loud: ScalarRamp,
    loud_was: f32,
    quality: Fade<{ quality::COUNT }>,
    channels: [Channel; 2],
    /// The ceiling each delayed sample's gain was worked out for, so the
    /// clip meets the same ceiling while Ceiling glides.
    ceilings: Vec<f32>,
    /// Loud's soft clip at each rate, its 1x output and the unclipped
    /// signal waiting to line up with it.
    loud_paths: [Path; 2],
    loud_x1: Delay<LATENCY>,
    clean: Delay<LATENCY>,
    /// The ceiling clip at each rate and its 1x output waiting.
    clip_paths: [Path; 2],
    clip_x1: Delay<LATENCY>,
    meter: DynamicsMeter,
}

impl LimiterEffect {
    pub fn new(sample_rate: f32) -> Self {
        let sample_rate = if sample_rate.is_finite() && sample_rate >= 2.0 {
            sample_rate
        } else {
            48_000.0
        };
        let lookahead_max = ((LOOKAHEAD_MAX_MS * 0.001 * sample_rate).round() as usize).max(2);
        // Detection already runs `TP_DELAY` behind, so the audio waits only
        // for the gain's ramp.
        let audio_delay = lookahead_max - 1;
        let params: [f32; param::COUNT] =
            std::array::from_fn(|index| LIMITER_PARAMS[index].default);
        let mut limiter = Self {
            sample_rate,
            params,
            phases: interpolator(),
            lookahead_max,
            timing: Timing {
                window: 1,
                release: 0.0,
                fast: 0.0,
                slow_release: 0.0,
                slow_attack: 0.0,
                auto: true,
            },
            time: 0,
            input: ScalarRamp::new(db_to_gain(params[param::INPUT_DB])),
            ceiling: ScalarRamp::new(db_to_gain(params[param::CEILING_DB])),
            link: ScalarRamp::new(params[param::STEREO_LINK]),
            loud: ScalarRamp::new(0.0),
            loud_was: 0.0,
            quality: Fade::at(quality::X4),
            channels: [
                Channel::new(lookahead_max, audio_delay),
                Channel::new(lookahead_max, audio_delay),
            ],
            ceilings: vec![db_to_gain(params[param::CEILING_DB]); audio_delay],
            loud_paths: [Path::new(), Path::new()],
            loud_x1: Delay::EMPTY,
            clean: Delay::EMPTY,
            clip_paths: [Path::new(), Path::new()],
            clip_x1: Delay::EMPTY,
            meter: DynamicsMeter::default(),
        };
        limiter.retime();
        limiter
    }

    fn style(&self) -> usize {
        (self.params[param::STYLE] as usize).min(style::COUNT - 1)
    }

    /// Works out the window and release times again, and rebuilds the
    /// histories the window reads so a change never jumps.
    fn retime(&mut self) {
        let scale = if self.style() == style::PUNCHY {
            PUNCHY_SCALE
        } else {
            1.0
        };
        let lookahead_ms = self.params[param::LOOKAHEAD_MS] * scale;
        let window = ((lookahead_ms * 0.001 * self.sample_rate).round() as usize)
            .clamp(1, self.lookahead_max);
        let release_ms = self.params[param::RELEASE_MS].max(AUTO_RELEASE_BELOW_MS) * scale;
        let rate = self.sample_rate;
        self.timing = Timing {
            window,
            release: coefficient(release_ms, rate),
            fast: coefficient(AUTO_FAST_MS * scale, rate),
            slow_release: coefficient(AUTO_SLOW_MS * scale, rate),
            slow_attack: coefficient(AUTO_SLOW_ATTACK_MS, rate),
            auto: self.params[param::RELEASE_MS] < AUTO_RELEASE_BELOW_MS,
        };
        let (time, max) = (self.time, self.lookahead_max);
        for channel in &mut self.channels {
            channel.held.clear();
            // The window covers what was already heard, not the sample
            // about to be.
            for back in (1..=window).rev() {
                let at = time.saturating_sub(back as u64);
                let need = channel.needs[(at as usize) % max];
                channel.held.push(at, need, window);
            }
            channel.sum = (1..=window)
                .map(|back| {
                    f64::from(channel.released[(time.saturating_sub(back as u64) as usize) % max])
                })
                .sum();
        }
    }

    fn edit(&mut self, index: usize, value: f32, automated: bool) -> bool {
        let Some(descriptor) = LIMITER_PARAMS.get(index) else {
            return false;
        };
        if !value.is_finite() {
            return false;
        }
        let value = match index {
            param::QUALITY | param::TRUE_PEAK => f32::from(value >= 0.5),
            param::STYLE => value.round(),
            _ => value,
        }
        .clamp(descriptor.min, descriptor.max);
        if self.params[index] == value {
            return true;
        }
        self.params[index] = value;
        let frames = RAMP_FRAMES as usize;
        match index {
            param::INPUT_DB => self.input.edit(db_to_gain(value), automated, frames),
            param::CEILING_DB => self.ceiling.edit(db_to_gain(value), automated, frames),
            param::STEREO_LINK => self.link.edit(value, automated, frames),
            param::QUALITY => {
                let rate = if value >= 0.5 {
                    quality::X4
                } else {
                    quality::X1
                };
                if automated {
                    self.quality = Fade::at(rate);
                } else {
                    self.quality.toward(rate);
                }
            }
            param::STYLE => {
                let loud = f32::from(self.style() == style::LOUD);
                self.loud.edit(loud, automated, frames);
                self.retime();
            }
            param::RELEASE_MS | param::LOOKAHEAD_MS => self.retime(),
            _ => {}
        }
        true
    }

    /// The gain channel `c` gets now, given what it needs.
    fn gain(&mut self, c: usize, need: f32) -> f32 {
        let timing = self.timing;
        let (time, max) = (self.time, self.lookahead_max);
        let channel = &mut self.channels[c];
        let slot = (time as usize) % max;
        channel.needs[slot] = need;
        let held = channel.held.push(time, need, timing.window);
        let rise = |envelope: f32, coefficient: f32| {
            if held < envelope {
                held
            } else {
                held - (held - envelope) * coefficient
            }
        };
        let released = if timing.auto {
            channel.release = rise(channel.release, timing.fast);
            // The slow envelope only follows reduction that lasts, so a
            // lone transient recovers on the fast one.
            channel.slow = if held < channel.slow {
                held + (channel.slow - held) * timing.slow_attack
            } else {
                held - (held - channel.slow) * timing.slow_release
            };
            channel.release.min(channel.slow)
        } else {
            channel.release = rise(channel.release, timing.release);
            channel.release
        };
        let leaving = channel.released[(time + max as u64 - timing.window as u64) as usize % max];
        channel.sum += f64::from(released) - f64::from(leaving);
        channel.released[slot] = released;
        let smoothed = (channel.sum / timing.window as f64) as f32;
        // The window may be shorter than the delay, so the gain waits the
        // difference.
        let waited =
            channel.gains[(time + max as u64 - (max - timing.window) as u64) as usize % max];
        channel.gains[slot] = smoothed.min(1.0);
        if max == timing.window {
            channel.gains[slot]
        } else {
            waited
        }
    }

    fn processed(&mut self, frame: [f32; 2], ch: usize) -> [f32; 2] {
        let input_gain = self.input.next_frame();
        let ceiling = self.ceiling.next_frame();
        let link = self.link.next_frame();
        let loud = self.loud.next_frame();
        if self.loud_was == 0.0 && loud > 0.0 {
            self.loud_paths.iter_mut().for_each(Path::reset);
            self.loud_x1 = Delay::EMPTY;
        }
        self.loud_was = loud;
        let was = self.quality.weights;
        self.quality.advance();
        let qualities = self.quality.weights;
        if was[quality::X4] == 0.0 && qualities[quality::X4] > 0.0 {
            self.loud_paths[1].reset();
            self.clip_paths[1].reset();
        }
        // Loud's soft clip, lined up with the clean signal either way.
        let driven = [frame[0] * input_gain, frame[1] * input_gain];
        let clean = self.clean.push(driven);
        let mut shaped = clean;
        if loud > 0.0 {
            let knee = ceiling * LOUD_HEADROOM;
            let clip = |x: f32| knee * soft_clip(x / knee);
            let mut by_rate = [[0.0; 2]; quality::COUNT];
            for c in 0..ch {
                for (rate, sample) in rates(&mut self.loud_paths, c, driven[c], &qualities, clip)
                    .into_iter()
                    .enumerate()
                {
                    by_rate[rate][c] = sample;
                }
            }
            let x1 = self.loud_x1.push(by_rate[quality::X1]);
            for c in 0..ch {
                let clipped = qualities[quality::X1] * x1[c]
                    + qualities[quality::X4] * by_rate[quality::X4][c];
                shaped[c] = clean[c] + loud * (clipped - clean[c]);
            }
        }
        let true_peak = self.params[param::TRUE_PEAK] >= 0.5;
        let mut delayed = [0.0; 2];
        let mut needs = [1.0f32; 2];
        for c in 0..ch {
            let (sample, peak) = self.channels[c].detect(shaped[c], &self.phases, true_peak);
            delayed[c] = sample;
            needs[c] = if peak > ceiling {
                (ceiling / peak).max(GAIN_FLOOR)
            } else {
                1.0
            };
            self.meter.detector_peak[c] = self.meter.detector_peak[c].max(driven[c].abs());
        }
        if ch == 2 {
            // Linking blends each channel's need towards the deeper one,
            // in decibels so half a link is half the reduction.
            let deeper = needs[0].min(needs[1]);
            for need in &mut needs {
                *need = (need.ln() + link * (deeper.ln() - need.ln())).exp();
            }
        }
        let slot = (self.time as usize) % self.ceilings.len();
        let ceiling = std::mem::replace(&mut self.ceilings[slot], ceiling);
        let mut limited = [0.0; 2];
        for c in 0..ch {
            let gain = self.gain(c, needs[c]);
            let audio = &mut self.channels[c].audio;
            let slot = (self.time as usize) % audio.len();
            let waited = std::mem::replace(&mut audio[slot], delayed[c]);
            limited[c] = waited * gain;
            let reduction = -gain_to_db(gain);
            self.meter.current_reduction_db[c] = reduction;
            self.meter.peak_reduction_db[c] = self.meter.peak_reduction_db[c].max(reduction);
        }
        // The ceiling clip catches what the gain leaves between samples.
        let clip = |x: f32| x.clamp(-ceiling, ceiling);
        let mut by_rate = [[0.0; 2]; quality::COUNT];
        for c in 0..ch {
            for (rate, sample) in rates(&mut self.clip_paths, c, limited[c], &qualities, clip)
                .into_iter()
                .enumerate()
            {
                by_rate[rate][c] = sample;
            }
        }
        let x1 = self.clip_x1.push(by_rate[quality::X1]);
        let mut out = [0.0; 2];
        for c in 0..ch {
            let clipped =
                qualities[quality::X1] * x1[c] + qualities[quality::X4] * by_rate[quality::X4][c];
            out[c] = clipped.clamp(-ceiling, ceiling);
        }
        self.time += 1;
        out
    }
}

fn finite(sample: f32) -> f32 {
    if sample.is_finite() {
        sample
    } else {
        0.0
    }
}

impl AudioEffect for LimiterEffect {
    fn supports_level_meter(&self) -> bool {
        true
    }

    fn take_dynamics_meter(&mut self) -> Option<DynamicsMeter> {
        let meter = self.meter;
        self.meter.peak_reduction_db = [0.0; 2];
        self.meter.detector_peak = [0.0; 2];
        Some(meter)
    }

    fn activation_sample_rate(&self) -> Option<u32> {
        Some(self.sample_rate as u32)
    }

    fn latency_samples(&self) -> u32 {
        (LATENCY + TP_DELAY + self.lookahead_max - 1 + LATENCY) as u32
    }

    fn effect_type(&self) -> EffectType {
        EffectType::Limiter
    }

    fn param_descriptors(&self) -> &'static [ParamDescriptor] {
        LIMITER_PARAMS
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
        for frame in buffer.chunks_exact_mut(channels) {
            let mut input = [0.0; 2];
            for c in 0..ch {
                input[c] = finite(frame[c]);
            }
            let out = self.processed(input, ch);
            for c in 0..ch {
                frame[c] = finite(out[c]);
            }
        }
    }

    fn reset(&mut self) {
        let (lookahead_max, audio_delay) = (self.lookahead_max, self.lookahead_max - 1);
        for channel in &mut self.channels {
            channel.taps = [0.0; TP_TAPS];
            channel.tap = 0;
            channel.needs.iter_mut().for_each(|need| *need = 1.0);
            channel.held.clear();
            channel.release = 1.0;
            channel.slow = 1.0;
            channel.released.iter_mut().for_each(|value| *value = 1.0);
            channel.gains.iter_mut().for_each(|gain| *gain = 1.0);
            channel.audio.iter_mut().for_each(|sample| *sample = 0.0);
            debug_assert_eq!(channel.audio.len(), audio_delay);
            debug_assert_eq!(channel.gains.len(), lookahead_max);
        }
        self.loud_paths.iter_mut().for_each(Path::reset);
        self.clip_paths.iter_mut().for_each(Path::reset);
        self.loud_x1 = Delay::EMPTY;
        self.clean = Delay::EMPTY;
        self.clip_x1 = Delay::EMPTY;
        let ceiling = db_to_gain(self.params[param::CEILING_DB]);
        self.ceilings.iter_mut().for_each(|value| *value = ceiling);
        self.time = 0;
        self.meter = DynamicsMeter::default();
        self.retime();
    }
}

#[cfg(test)]
#[path = "limiter_tests.rs"]
mod tests;
