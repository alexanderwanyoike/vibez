//! One voice of the synth: its oscillator, filter and envelope.

use crate::synth_envelope::{Envelope, Times};
use crate::synth_filter::{Coefficients, Svf};
use crate::synth_oscillator::{sample, Waveform};

#[derive(Clone, Copy, Debug, Default)]
pub struct Voice {
    pub pitch: u8,
    /// The note count when this voice started; smallest is oldest.
    pub age: u64,
    velocity: f32,
    phase: f64,
    increment: f64,
    envelope: Envelope,
    filter: Svf,
}

impl Voice {
    /// The oscillator keeps its phase and the envelope its level, so a
    /// stolen or repeated voice changes note without a click.
    pub fn start(&mut self, pitch: u8, velocity: u8, age: u64, sample_rate: f32) {
        if !self.envelope.active() {
            self.filter.reset();
        }
        self.pitch = pitch;
        self.age = age;
        self.velocity = f32::from(velocity) / 127.0;
        let frequency = 440.0 * 2f64.powf((f64::from(pitch) - 69.0) / 12.0);
        self.increment = frequency / f64::from(sample_rate);
        self.envelope.trigger();
    }

    pub fn release(&mut self) {
        self.envelope.release();
    }

    pub fn active(&self) -> bool {
        self.envelope.active()
    }

    pub fn released(&self) -> bool {
        self.envelope.released()
    }

    pub fn generate(&mut self, waveform: Waveform, times: &Times, filter: Coefficients) -> f32 {
        let level = self.envelope.tick(times);
        let raw = sample(waveform, self.phase, self.increment);
        self.phase = (self.phase + self.increment).fract();
        self.filter.low(raw, filter) * level * self.velocity
    }
}
