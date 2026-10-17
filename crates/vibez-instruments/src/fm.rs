//! The FM Synth: four operators wired by one of eight algorithms, each
//! with a ratio, level, feedback, key tracking and envelope, under four
//! controls that move the whole sound: Brightness, Decay, Motion (the
//! LFO) and Volume. Each note takes a free voice, or else the oldest.

use vibez_core::effect::ParamDescriptor;

use crate::envelope::Times;
pub use crate::fm_params::{
    dest, is_ratio, is_whole, level_gain, lfo_shape, op, param, Algorithm, ALGORITHMS,
    ALGORITHM_NAMES, FM_PARAMS, OPERATORS,
};
use crate::fm_params::{MAX_VOICES, MOD_PITCH_SEMITONES};
use crate::fm_voice::{Frame, Operators, Stack, Voice, MODULATION_CYCLES};
use crate::synth::lfo;

/// How fast continuous levels follow an edit, so a moved knob does not
/// click or zip.
const SMOOTHING_SECONDS: f32 = 0.005;
const SMOOTHED: [usize; 7] = [
    param::of(0, op::LEVEL),
    param::of(1, op::LEVEL),
    param::of(2, op::LEVEL),
    param::of(3, op::LEVEL),
    param::BRIGHTNESS,
    param::MOTION,
    param::VOLUME_DB,
];
/// Decay at either end shortens or lengthens every decay and release this
/// many times.
const DECAY_RANGE: f32 = 4.0;

fn algorithm(params: &[f32]) -> &'static Algorithm {
    &ALGORITHMS[(params[param::ALGORITHM] as usize).min(ALGORITHMS.len() - 1)]
}

fn ratio(params: &[f32], operator: usize) -> f32 {
    (params[param::of(operator, op::RATIO)] + params[param::of(operator, op::FINE)]).max(0.01)
}

/// How far a modulator at full level moves its targets at `brightness`.
fn depth(brightness: f32) -> f32 {
    2.0 * brightness.clamp(0.0, 1.0) * MODULATION_CYCLES
}

/// Two cycles of the sound at its brightest, every envelope at its peak,
/// at `points` samples: what the card draws. Feedback runs two cycles
/// first, so the drawing starts settled.
pub fn preview(params: &[f32], points: usize) -> Vec<f32> {
    if params.len() < param::COUNT || points == 0 {
        return Vec::new();
    }
    crate::fm_voice::prepare();
    let increment = 2.0 / points as f32;
    let operators = Operators {
        increments: std::array::from_fn(|operator| increment * ratio(params, operator)),
        levels: std::array::from_fn(|operator| level_gain(params[param::of(operator, op::LEVEL)])),
        feedback: std::array::from_fn(|operator| params[param::of(operator, op::FEEDBACK)]),
        depth: depth(params[param::BRIGHTNESS]),
    };
    let algorithm = algorithm(params);
    let mut stack = Stack::default();
    for _ in 0..points {
        stack.tick(algorithm, &operators);
    }
    (0..points)
        .map(|_| stack.tick(algorithm, &operators))
        .collect()
}

pub struct FmSynth {
    voices: [Voice; MAX_VOICES],
    sample_rate: f32,
    params: [f32; param::COUNT],
    /// The smoothed controls' current values; the rest read `params`.
    current: [f32; param::COUNT],
    times: [Times; OPERATORS],
    smoothing: f32,
    started: bool,
    age: u64,
    lfo_phase: f64,
    /// The levels in dB `gains` were worked out for, and the gains.
    gains_for: [f32; OPERATORS],
    gains: [f32; OPERATORS],
}

impl FmSynth {
    pub fn new(sample_rate: f32) -> Self {
        let sample_rate = if sample_rate.is_finite() && sample_rate > 0.0 {
            sample_rate
        } else {
            48_000.0
        };
        crate::fm_voice::prepare();
        let params: [f32; param::COUNT] = std::array::from_fn(|index| FM_PARAMS[index].default);
        let mut synth = Self {
            voices: [Voice::default(); MAX_VOICES],
            sample_rate,
            params,
            current: params,
            times: [Times::new(0.0, 0.0, 0.0, 0.0, sample_rate); OPERATORS],
            smoothing: 1.0 - (-1.0 / (SMOOTHING_SECONDS * sample_rate)).exp(),
            started: false,
            age: 0,
            lfo_phase: 0.0,
            gains_for: [f32::NAN; OPERATORS],
            gains: [0.0; OPERATORS],
        };
        synth.retime();
        synth
    }

    fn retime(&mut self) {
        let p = &self.params;
        let scale = DECAY_RANGE.powf(2.0 * p[param::DECAY] - 1.0);
        self.times = std::array::from_fn(|operator| {
            let control = |control: usize| p[param::of(operator, control)];
            Times::new(
                control(op::ATTACK),
                control(op::DECAY) * scale,
                control(op::SUSTAIN),
                control(op::RELEASE) * scale,
                self.sample_rate,
            )
        });
    }

    pub fn set_param(&mut self, index: usize, value: f32) -> bool {
        let Some(descriptor) = FM_PARAMS.get(index) else {
            return false;
        };
        if !value.is_finite() {
            return false;
        }
        let value = if is_whole(index) {
            value.round()
        } else if is_ratio(index) {
            (value * 2.0).round() / 2.0
        } else {
            value
        }
        .clamp(descriptor.min, descriptor.max);
        self.params[index] = value;
        if !self.started || !SMOOTHED.contains(&index) {
            self.current[index] = value;
        }
        self.retime();
        true
    }

    pub fn get_param(&self, index: usize) -> f32 {
        self.params.get(index).copied().unwrap_or(0.0)
    }

    fn choice(&self, index: usize) -> usize {
        self.params[index] as usize
    }

    pub fn note_on(&mut self, pitch: u8, velocity: u8) {
        if velocity == 0 {
            self.note_off(pitch);
            return;
        }
        self.age += 1;
        let count = self.choice(param::VOICES).clamp(1, MAX_VOICES);
        let index = crate::poly::choose(&self.voices[..count], pitch);
        let carriers = algorithm(&self.params).carriers;
        self.voices[index].start(pitch, velocity, self.age, carriers);
    }

    pub fn note_off(&mut self, pitch: u8) {
        for voice in &mut self.voices {
            if voice.active() && voice.pitch == pitch {
                voice.release();
            }
        }
    }

    fn sample(&mut self) -> f32 {
        self.started = true;
        for index in SMOOTHED {
            self.current[index] += (self.params[index] - self.current[index]) * self.smoothing;
        }
        let mut push = [0.0; dest::COUNT];
        let motion = self.current[param::MOTION];
        if motion > 0.0 {
            push[self.choice(param::LFO_DEST).min(dest::COUNT - 1)] =
                lfo(self.choice(param::LFO_SHAPE), self.lfo_phase) * motion;
        }
        self.lfo_phase = (self.lfo_phase
            + f64::from(self.params[param::LFO_RATE]) / f64::from(self.sample_rate))
        .fract();
        let levels: [f32; OPERATORS] =
            std::array::from_fn(|operator| self.current[param::of(operator, op::LEVEL)]);
        if levels != self.gains_for {
            self.gains_for = levels;
            self.gains = levels.map(level_gain);
        }
        let p = &self.params;
        let c = &self.current;
        let frame = Frame {
            sample_rate: self.sample_rate,
            algorithm: algorithm(p),
            ratios: std::array::from_fn(|operator| ratio(p, operator)),
            levels: self.gains,
            feedback: std::array::from_fn(|operator| p[param::of(operator, op::FEEDBACK)]),
            key_track: std::array::from_fn(|operator| p[param::of(operator, op::KEY_TRACK)]),
            times: self.times,
            depth: depth(c[param::BRIGHTNESS]) * (1.0 + push[dest::BRIGHTNESS]).clamp(0.0, 2.0),
            velocity_sense: p[param::VELOCITY],
            pitch: push[dest::PITCH] * MOD_PITCH_SEMITONES,
            volume: (1.0 + push[dest::VOLUME]).clamp(0.0, 2.0),
        };
        let mut sum = 0.0;
        for voice in &mut self.voices {
            if voice.active() {
                sum += voice.generate(&frame);
            }
        }
        sum * 10f32.powf(c[param::VOLUME_DB] / 20.0)
    }

    /// Adds the synth's output to every channel of `buffer`; notes arrive
    /// through `note_on` and `note_off` beforehand.
    pub fn render(&mut self, buffer: &mut [f32], channels: usize) {
        for frame in buffer.chunks_exact_mut(channels.max(1)) {
            let sample = self.sample();
            for channel in frame {
                *channel += sample;
            }
        }
    }

    pub fn reset(&mut self) {
        self.voices = [Voice::default(); MAX_VOICES];
        self.current = self.params;
        self.lfo_phase = 0.0;
    }
}

impl crate::Instrument for FmSynth {
    fn activation_sample_rate(&self) -> Option<u32> {
        Some(self.sample_rate as u32)
    }

    fn instrument_kind(&self) -> vibez_core::midi::InstrumentKind {
        vibez_core::midi::InstrumentKind::FmSynth
    }

    fn param_descriptors(&self) -> &'static [ParamDescriptor] {
        FM_PARAMS
    }

    fn set_param(&mut self, index: usize, value: f32) -> bool {
        self.set_param(index, value)
    }

    fn get_param(&self, index: usize) -> f32 {
        self.get_param(index)
    }

    fn note_on(&mut self, pitch: u8, velocity: u8) {
        self.note_on(pitch, velocity);
    }

    fn note_off(&mut self, pitch: u8) {
        self.note_off(pitch);
    }

    fn render(&mut self, buffer: &mut [f32], channels: usize) {
        self.render(buffer, channels);
    }

    fn reset(&mut self) {
        self.reset();
    }
}

#[cfg(test)]
#[path = "fm_tests.rs"]
mod tests;
