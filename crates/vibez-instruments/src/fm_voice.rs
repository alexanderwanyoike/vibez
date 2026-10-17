//! One voice of the FM Synth: four sine operators, each with its own
//! envelope, modulating one another's phase as the algorithm wires them.
//!
//! Phase modulation after J. Chowning, "The Synthesis of Complex Audio
//! Spectra by Means of Frequency Modulation", JAES 21(7), 1973, played as
//! Yamaha's DX and TX synths play it. An operator's feedback averages its
//! last two outputs, as the DX7 does to keep high feedback from ringing at
//! half the sample rate; see the FM core of Dexed (github.com/asb2m10/dexed).

use std::sync::OnceLock;

use crate::envelope::{Envelope, Times};
use crate::fm_params::{Algorithm, OPERATORS};

const SINE_SIZE: usize = 2048;
/// A modulator at full level moves its target's phase this many cycles
/// either way at the middle Brightness: an index of pi, bright without
/// turning to noise.
pub const MODULATION_CYCLES: f32 = 0.5;
/// Full feedback moves an operator's own phase this many cycles.
const FEEDBACK_CYCLES: f32 = 0.25;

/// One cycle of a sine and its first sample again, for reading between.
fn sines() -> &'static [f32] {
    static SINES: OnceLock<Vec<f32>> = OnceLock::new();
    SINES.get_or_init(|| {
        (0..=SINE_SIZE)
            .map(|n| (std::f32::consts::TAU * n as f32 / SINE_SIZE as f32).sin())
            .collect()
    })
}

/// Built here, on the thread that makes the synth, never in the audio
/// callback.
pub fn prepare() {
    let _ = sines();
}

fn sine(table: &[f32], phase: f32) -> f32 {
    let at = (phase - phase.floor()) * SINE_SIZE as f32;
    let index = (at as usize).min(SINE_SIZE - 1);
    let fraction = at - index as f32;
    table[index] + fraction * (table[index + 1] - table[index])
}

/// What the operators play for one sample.
#[derive(Clone, Copy)]
pub struct Operators {
    /// Each operator's cycles a sample.
    pub increments: [f32; OPERATORS],
    /// Each operator's level, its envelope applied.
    pub levels: [f32; OPERATORS],
    /// Each operator's feedback, 0 to 1.
    pub feedback: [f32; OPERATORS],
    /// How far a modulator at full level moves its targets, in cycles.
    pub depth: f32,
}

/// The operators' running state.
#[derive(Clone, Copy, Debug, Default)]
pub struct Stack {
    phases: [f32; OPERATORS],
    /// Each operator's last two outputs, its level applied.
    history: [[f32; 2]; OPERATORS],
}

impl Stack {
    /// Every operator one sample on, from D down to A; the carriers' sum,
    /// shared so that more of them are no louder.
    pub fn tick(&mut self, algorithm: &Algorithm, operators: &Operators) -> f32 {
        let table = sines();
        let mut outputs = [0.0f32; OPERATORS];
        let mut sum = 0.0;
        for operator in (0..OPERATORS).rev() {
            let level = operators.levels[operator];
            let [last, before] = self.history[operator];
            let mut offset = operators.feedback[operator] * FEEDBACK_CYCLES * 0.5 * (last + before);
            let modulators = algorithm.modulators[operator];
            for (source, output) in outputs.iter().enumerate().skip(operator + 1) {
                if modulators & (1 << source) != 0 {
                    offset += output;
                }
            }
            let phase = &mut self.phases[operator];
            let out = if level > 0.0 {
                sine(table, *phase + offset) * level
            } else {
                0.0
            };
            *phase = (*phase + operators.increments[operator]).fract();
            self.history[operator] = [out, last];
            if algorithm.carriers & (1 << operator) != 0 {
                sum += out;
            } else {
                outputs[operator] = out * operators.depth;
            }
        }
        sum / algorithm.carriers.count_ones().max(1) as f32
    }
}

/// What every voice shares for one sample.
pub struct Frame<'a> {
    pub sample_rate: f32,
    pub algorithm: &'a Algorithm,
    /// Each operator's ratio, Fine added.
    pub ratios: [f32; OPERATORS],
    pub levels: [f32; OPERATORS],
    pub feedback: [f32; OPERATORS],
    pub key_track: [f32; OPERATORS],
    pub times: [Times; OPERATORS],
    /// How far a modulator at full level moves its targets, Brightness
    /// and the LFO applied.
    pub depth: f32,
    pub velocity_sense: f32,
    /// The LFO's push on the pitch in semitones, and on the volume.
    pub pitch: f32,
    pub volume: f32,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Voice {
    pub pitch: u8,
    pub age: u64,
    velocity: f32,
    /// The operators the voice sounds through, so it ends with them.
    carriers: u8,
    stack: Stack,
    envelopes: [Envelope; OPERATORS],
    /// The pitch, in semitones, `increment` was worked out for.
    increment_for: f32,
    /// Cycles a sample at the note's frequency.
    increment: f32,
    /// The key tracking `tracked` was worked out for, and each operator's
    /// level for this note under it.
    tracked_for: [f32; OPERATORS],
    tracked: [f32; OPERATORS],
}

impl crate::poly::PolyVoice for Voice {
    fn pitch(&self) -> u8 {
        self.pitch
    }

    fn age(&self) -> u64 {
        self.age
    }

    fn active(&self) -> bool {
        self.active()
    }

    fn released(&self) -> bool {
        self.carrier_envelopes().all(Envelope::released)
    }
}

impl Voice {
    fn carrier_envelopes(&self) -> impl Iterator<Item = &Envelope> {
        let carriers = self.carriers;
        self.envelopes
            .iter()
            .enumerate()
            .filter(move |(operator, _)| carriers & (1 << operator) != 0)
            .map(|(_, envelope)| envelope)
    }

    /// Starts `pitch`. A voice taken over keeps its phases and envelope
    /// levels, so a stolen or repeated note never clicks.
    pub fn start(&mut self, pitch: u8, velocity: u8, age: u64, carriers: u8) {
        if !self.active() {
            self.stack = Stack::default();
        }
        self.pitch = pitch;
        self.tracked_for = [f32::NAN; OPERATORS];
        self.age = age;
        self.velocity = f32::from(velocity) / 127.0;
        self.carriers = carriers;
        for envelope in &mut self.envelopes {
            envelope.trigger();
        }
    }

    pub fn release(&mut self) {
        for envelope in &mut self.envelopes {
            envelope.release();
        }
    }

    pub fn active(&self) -> bool {
        self.carrier_envelopes().any(Envelope::active)
    }

    pub fn generate(&mut self, frame: &Frame) -> f32 {
        self.carriers = frame.algorithm.carriers;
        let semitones = f32::from(self.pitch) + frame.pitch;
        if semitones != self.increment_for || self.increment == 0.0 {
            self.increment_for = semitones;
            self.increment = 440.0 * ((semitones - 69.0) / 12.0).exp2() / frame.sample_rate;
        }
        if frame.key_track != self.tracked_for {
            self.tracked_for = frame.key_track;
            let above_middle_c = (f32::from(self.pitch) - 60.0).max(0.0) / 12.0;
            self.tracked = frame
                .key_track
                .map(|amount| (-amount * above_middle_c).exp2());
        }
        let mut levels = [0.0; OPERATORS];
        let mut increments = [0.0; OPERATORS];
        for operator in 0..OPERATORS {
            let envelope = self.envelopes[operator].tick(&frame.times[operator]);
            levels[operator] = frame.levels[operator] * envelope * self.tracked[operator];
            increments[operator] = self.increment * frame.ratios[operator];
        }
        let softened = 1.0 - frame.velocity_sense * (1.0 - self.velocity);
        let operators = Operators {
            increments,
            levels,
            feedback: frame.feedback,
            depth: frame.depth * softened,
        };
        let out = self.stack.tick(frame.algorithm, &operators);
        out * self.velocity * frame.volume
    }
}
