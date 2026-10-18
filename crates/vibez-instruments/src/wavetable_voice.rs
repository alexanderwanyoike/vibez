//! One voice of the Wavetable Synth: two oscillators of up to eight unison
//! voices each, spread across the stereo field, through drive and a stereo
//! filter, with an envelope for the level and one to modulate.

use crate::envelope::{Envelope, Times};
use crate::synth_filter::{Coefficients, Svf};
use crate::wavetable_params::{
    dest, filter_type, MAX_UNISON, MOD_CUTOFF_OCTAVES, MOD_PITCH_SEMITONES,
};
use crate::wavetables::{level_for, Wavetable};

/// A moving cutoff is worked out again every this many samples, well
/// above any envelope or LFO speed.
const CONTROL_FRAMES: u8 = 8;
/// The second stage of the 24 dB low-pass adds slope, not more resonance.
const BUTTERWORTH_Q: f32 = std::f32::consts::FRAC_1_SQRT_2;

/// Phase distortion, as on Casio's CZ synths: the first half of the cycle
/// is squeezed into less of it and the second stretched over the rest.
#[derive(Clone, Copy)]
pub struct Warp {
    bend: f32,
    rise: f32,
    fall: f32,
}

impl Warp {
    pub fn new(amount: f32) -> Self {
        let bend = 0.5 - 0.48 * amount.clamp(0.0, 1.0);
        Self {
            bend,
            rise: 0.5 / bend,
            fall: 0.5 / (1.0 - bend),
        }
    }

    pub fn apply(self, phase: f32) -> f32 {
        if phase < self.bend {
            phase * self.rise
        } else {
            0.5 + (phase - self.bend) * self.fall
        }
    }

    /// The steepest the warped phase runs, so the table read can drop the
    /// harmonics that would alias.
    pub fn steepest(self) -> f32 {
        self.rise
    }
}

/// A Pade approximation of tanh, saturating at 1 from |x| = 3.
fn soft_clip(x: f32) -> f32 {
    let x = x.clamp(-3.0, 3.0);
    x * (27.0 + x * x) / (27.0 + 9.0 * x * x)
}

/// One oscillator's settings for this sample, modulation applied.
#[derive(Clone, Copy)]
pub struct OscFrame<'a> {
    pub table: &'a Wavetable,
    pub position: f32,
    pub warp: f32,
    pub level: f32,
    /// Each unison voice's frequency against the note's, and its left and
    /// right gains.
    pub ratios: [f32; MAX_UNISON],
    pub gains: [[f32; 2]; MAX_UNISON],
    pub unison: usize,
    /// The oscillator's octave, as a ratio.
    pub octave: f32,
}

/// What every voice shares for one sample.
pub struct Frame<'a> {
    pub sample_rate: f32,
    pub oscillators: [OscFrame<'a>; 2],
    pub filter_type: usize,
    pub cutoff: f32,
    pub q: f32,
    /// The coefficients at `cutoff`, for voices whose cutoff does not move.
    pub coefficients: Coefficients,
    pub drive: f32,
    /// The LFO's push on each destination, the same for every voice.
    pub lfo: [f32; dest::COUNT],
    pub env2_amount: f32,
    pub env2_dest: usize,
    pub amp_times: Times,
    pub env2_times: Times,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Voice {
    pub pitch: u8,
    pub age: u64,
    velocity: f32,
    /// Each oscillator's unison voices' phases.
    phases: [[f32; MAX_UNISON]; 2],
    /// The pitch, in semitones, `increment` was worked out for.
    increment_for: f32,
    /// Cycles a sample at the note's frequency.
    increment: f32,
    amp: Envelope,
    env2: Envelope,
    /// Two stages for each channel.
    filters: [[Svf; 2]; 2],
    moved: Option<(Coefficients, u8)>,
}

impl crate::poly::PolyVoice for Voice {
    fn pitch(&self) -> u8 {
        self.pitch
    }

    fn age(&self) -> u64 {
        self.age
    }

    fn active(&self) -> bool {
        self.amp.active()
    }

    fn released(&self) -> bool {
        self.amp.released()
    }
}

impl Voice {
    /// Starts `pitch`. A voice taken over keeps its phases and envelope
    /// levels, so a stolen or repeated note never clicks; a free one starts
    /// its unison voices at spread phases so they never sum to one spike.
    pub fn start(&mut self, pitch: u8, velocity: u8, age: u64) {
        if !self.amp.active() {
            self.filters = [[Svf::default(); 2]; 2];
            let mut seed = 0x9E37_79B9u32 ^ (age as u32).wrapping_mul(0x85EB_CA6B);
            for phases in &mut self.phases {
                for phase in phases.iter_mut() {
                    seed ^= seed << 13;
                    seed ^= seed >> 17;
                    seed ^= seed << 5;
                    *phase = seed as f32 / u32::MAX as f32;
                }
                // The first unison voice starts at zero, so a single voice
                // begins its cycle cleanly.
                phases[0] = 0.0;
            }
        }
        self.pitch = pitch;
        self.age = age;
        self.velocity = f32::from(velocity) / 127.0;
        self.amp.trigger();
        self.env2.trigger();
    }

    pub fn release(&mut self) {
        self.amp.release();
        self.env2.release();
    }

    pub fn active(&self) -> bool {
        self.amp.active()
    }

    pub fn generate(&mut self, frame: &Frame) -> [f32; 2] {
        let level = self.amp.tick(&frame.amp_times);
        let env2 = self.env2.tick(&frame.env2_times) * frame.env2_amount;
        let mut push = frame.lfo;
        push[frame.env2_dest.min(dest::COUNT - 1)] += env2;

        let semitones = f32::from(self.pitch) + push[dest::PITCH] * MOD_PITCH_SEMITONES;
        if semitones != self.increment_for || self.increment == 0.0 {
            self.increment_for = semitones;
            self.increment = 440.0 * ((semitones - 69.0) / 12.0).exp2() / frame.sample_rate;
        }
        let mut mixed = [0.0f32; 2];
        for (osc, settings) in frame.oscillators.iter().enumerate() {
            if settings.level <= 0.0 {
                continue;
            }
            let position = (settings.position + push[dest::position(osc)]).clamp(0.0, 1.0);
            let amount = (settings.warp + push[dest::warp(osc)]).clamp(0.0, 1.0);
            let base = self.increment * settings.octave;
            let shape = Warp::new(amount);
            let steepest = shape.steepest();
            let widest = settings.ratios[..settings.unison]
                .iter()
                .fold(0.0f32, |widest, ratio| widest.max(*ratio));
            let frames = settings
                .table
                .at(position, level_for(f64::from(base * widest * steepest)));
            for unison in 0..settings.unison {
                let phase = &mut self.phases[osc][unison];
                let warped = if amount > 0.0 {
                    shape.apply(*phase)
                } else {
                    *phase
                };
                let sample = frames.read(warped) * settings.level;
                mixed[0] += sample * settings.gains[unison][0];
                mixed[1] += sample * settings.gains[unison][1];
                *phase = (*phase + base * settings.ratios[unison]).fract();
            }
        }
        if frame.drive > 0.0 {
            let gain = 1.0 + 8.0 * frame.drive;
            for sample in &mut mixed {
                *sample = soft_clip(*sample * gain) / (1.0 + 2.0 * frame.drive);
            }
        }

        let octaves = push[dest::CUTOFF] * MOD_CUTOFF_OCTAVES;
        let coefficients = if octaves == 0.0 {
            self.moved = None;
            frame.coefficients
        } else {
            let (coefficients, left) = match self.moved {
                Some((coefficients, left)) if left > 0 => (coefficients, left - 1),
                _ => (
                    Coefficients::new(frame.sample_rate, frame.cutoff * octaves.exp2(), frame.q),
                    CONTROL_FRAMES - 1,
                ),
            };
            self.moved = Some((coefficients, left));
            coefficients
        };
        let gain = level * self.velocity * (1.0 + push[dest::VOLUME]).clamp(0.0, 2.0);
        let mut out = [0.0; 2];
        for (channel, sample) in mixed.into_iter().enumerate() {
            let stages = &mut self.filters[channel];
            let first = stages[0].process(sample, coefficients);
            let filtered = match frame.filter_type {
                filter_type::LP24 => {
                    stages[1]
                        .process(first.low, coefficients.with_q(BUTTERWORTH_Q))
                        .low
                }
                filter_type::HP12 => first.high,
                filter_type::BP12 => first.band,
                _ => first.low,
            };
            out[channel] = filtered * gain;
        }
        out
    }
}
