//! One voice of the synth: two oscillators, a sub and noise into a filter,
//! with an envelope each for the level and the cutoff.

use crate::synth_envelope::{Envelope, Times};
use crate::synth_filter::{Coefficients, Svf};
use crate::synth_oscillator::{sample, Waveform};
use crate::synth_params::filter_type;

/// The second stage of the 24 dB low-pass adds slope, not more resonance.
const BUTTERWORTH_Q: f32 = std::f32::consts::FRAC_1_SQRT_2;

/// What every voice shares for one sample.
pub struct Frame {
    pub sample_rate: f32,
    pub waveform: Waveform,
    pub waveform_2: Waveform,
    /// Each oscillator's frequency against the note's.
    pub ratio: f64,
    pub ratio_2: f64,
    pub level_2: f32,
    pub sub: f32,
    pub noise: f32,
    pub filter_type: usize,
    pub cutoff: f32,
    pub resonance: f32,
    /// The coefficients at `cutoff`, for voices whose cutoff does not move.
    pub coefficients: Coefficients,
    pub filter_env: f32,
    pub cutoff_octaves: f32,
    pub pitch_semitones: f32,
    pub amp_times: Times,
    pub filter_times: Times,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Voice {
    pub pitch: u8,
    /// The note count when this voice started; smallest is oldest.
    pub age: u64,
    velocity: f32,
    /// The sounding pitch in semitones, gliding toward `pitch`.
    current: f32,
    glide_step: f32,
    glide_left: u32,
    phases: [f64; 3],
    noise: u32,
    amp: Envelope,
    filter_env: Envelope,
    filters: [Svf; 2],
}

impl Voice {
    /// Starts `pitch`, gliding from `from` over `glide_frames` when given.
    /// The oscillators keep their phase and the envelopes their level, so
    /// a stolen or repeated voice changes note without a click.
    pub fn start(
        &mut self,
        pitch: u8,
        velocity: u8,
        age: u64,
        from: Option<f32>,
        glide_frames: u32,
        retrigger: bool,
    ) {
        if !self.amp.active() {
            self.filters = [Svf::default(); 2];
            self.noise = 0x9E37_79B9 ^ (age as u32).wrapping_mul(0x85EB_CA6B);
        }
        self.pitch = pitch;
        self.age = age;
        self.velocity = f32::from(velocity) / 127.0;
        let target = f32::from(pitch);
        match from {
            Some(from) if glide_frames > 0 => {
                self.current = from;
                self.glide_left = glide_frames;
                self.glide_step = (target - from) / glide_frames as f32;
            }
            _ => {
                self.current = target;
                self.glide_left = 0;
            }
        }
        if retrigger || !self.amp.active() {
            self.amp.trigger();
            self.filter_env.trigger();
        }
    }

    pub fn release(&mut self) {
        self.amp.release();
        self.filter_env.release();
    }

    pub fn active(&self) -> bool {
        self.amp.active()
    }

    pub fn released(&self) -> bool {
        self.amp.released()
    }

    /// The pitch the voice is sounding now, in semitones.
    pub fn sounding(&self) -> f32 {
        self.current
    }

    pub fn generate(&mut self, frame: &Frame) -> f32 {
        let level = self.amp.tick(&frame.amp_times);
        let opening = self.filter_env.tick(&frame.filter_times);
        if self.glide_left > 0 {
            self.current += self.glide_step;
            self.glide_left -= 1;
            if self.glide_left == 0 {
                self.current = f32::from(self.pitch);
            }
        }
        let semitones = f64::from(self.current + frame.pitch_semitones);
        let increment = 440.0 * ((semitones - 69.0) / 12.0).exp2() / f64::from(frame.sample_rate);
        let increments = [
            increment * frame.ratio,
            increment * frame.ratio_2,
            increment * frame.ratio * 0.5,
        ];
        let mut raw = sample(frame.waveform, self.phases[0], increments[0]);
        if frame.level_2 > 0.0 {
            raw += frame.level_2 * sample(frame.waveform_2, self.phases[1], increments[1]);
        }
        if frame.sub > 0.0 {
            raw += frame.sub * sample(Waveform::Sine, self.phases[2], increments[2]);
        }
        if frame.noise > 0.0 {
            self.noise ^= self.noise << 13;
            self.noise ^= self.noise >> 17;
            self.noise ^= self.noise << 5;
            raw += frame.noise * (self.noise as f32 / u32::MAX as f32 * 2.0 - 1.0);
        }
        for (phase, increment) in self.phases.iter_mut().zip(increments) {
            *phase = (*phase + increment).fract();
        }

        let octaves = opening * frame.filter_env + frame.cutoff_octaves;
        let coefficients = if octaves == 0.0 {
            frame.coefficients
        } else {
            Coefficients::new(
                frame.sample_rate,
                frame.cutoff * octaves.exp2(),
                frame.resonance,
            )
        };
        let first = self.filters[0].process(raw, coefficients);
        let filtered = match frame.filter_type {
            filter_type::LP24 => {
                self.filters[1]
                    .process(first.low, coefficients.with_q(BUTTERWORTH_Q))
                    .low
            }
            filter_type::HP12 => first.high,
            filter_type::BP12 => first.band,
            _ => first.low,
        };
        filtered * level * self.velocity
    }
}
