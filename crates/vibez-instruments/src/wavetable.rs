//! The Wavetable Synth: two oscillators morphing through band-limited
//! wavetables, each with warp and stereo unison, into drive and a resonant
//! filter, with an envelope for the level, a second envelope and an LFO to
//! send where they are wanted. Each note takes a free voice, or else the
//! oldest.

use vibez_core::effect::ParamDescriptor;

use crate::envelope::Times;
use crate::synth::lfo;
use crate::synth_filter::Coefficients;
pub use crate::wavetable_params::{
    dest, filter_type, is_whole, lfo_shape, param, resonance_q, WAVETABLE_PARAMS,
};
use crate::wavetable_params::{osc, MAX_UNISON, MAX_VOICES};
use crate::wavetable_voice::{Frame, OscFrame, Voice};
use crate::wavetables::{table, tables};

/// How fast continuous levels follow an edit, so a moved knob does not
/// click or zip.
const SMOOTHING_SECONDS: f32 = 0.005;
const SMOOTHED: [usize; 9] = [
    param::of(0, osc::POSITION),
    param::of(0, osc::WARP),
    param::of(0, osc::LEVEL),
    param::of(1, osc::POSITION),
    param::of(1, osc::WARP),
    param::of(1, osc::LEVEL),
    param::CUTOFF,
    param::LFO_AMOUNT,
    param::OUTPUT_DB,
];

/// Each unison voice's frequency ratio and stereo gains for `unison`
/// voices spread `cents` either side: evenly across the spread and the
/// stereo field, at equal power, quieter as more are stacked.
pub fn unison(unison: usize, cents: f32) -> ([f32; MAX_UNISON], [[f32; 2]; MAX_UNISON]) {
    let unison = unison.clamp(1, MAX_UNISON);
    let mut ratios = [1.0; MAX_UNISON];
    let mut gains = [[0.0; 2]; MAX_UNISON];
    let scale = 1.0 / (unison as f32).sqrt();
    for voice in 0..unison {
        let spread = if unison == 1 {
            0.0
        } else {
            2.0 * voice as f32 / (unison - 1) as f32 - 1.0
        };
        ratios[voice] = (spread * cents / 1200.0).exp2();
        let angle = (spread + 1.0) * std::f32::consts::FRAC_PI_4;
        gains[voice] = [angle.cos() * scale, angle.sin() * scale];
    }
    (ratios, gains)
}

pub struct WavetableSynth {
    voices: [Voice; MAX_VOICES],
    sample_rate: f32,
    params: [f32; param::COUNT],
    /// The smoothed controls' current values; the rest read `params`.
    current: [f32; param::COUNT],
    amp_times: Times,
    env2_times: Times,
    smoothing: f32,
    started: bool,
    age: u64,
    lfo_phase: f64,
    /// Each oscillator's unison ratios and gains.
    unison: [([f32; MAX_UNISON], [[f32; 2]; MAX_UNISON]); 2],
    /// The cutoff and Q `coefficients` were worked out for.
    filter_for: (f32, f32),
    coefficients: Coefficients,
}

impl WavetableSynth {
    pub fn new(sample_rate: f32) -> Self {
        let sample_rate = if sample_rate.is_finite() && sample_rate > 0.0 {
            sample_rate
        } else {
            48_000.0
        };
        // Built here, on the thread that makes the synth, never in the
        // audio callback.
        let _ = tables();
        let params: [f32; param::COUNT] =
            std::array::from_fn(|index| WAVETABLE_PARAMS[index].default);
        let mut synth = Self {
            voices: [Voice::default(); MAX_VOICES],
            sample_rate,
            params,
            current: params,
            amp_times: Times::new(0.0, 0.0, 0.0, 0.0, sample_rate),
            env2_times: Times::new(0.0, 0.0, 0.0, 0.0, sample_rate),
            smoothing: 1.0 - (-1.0 / (SMOOTHING_SECONDS * sample_rate)).exp(),
            started: false,
            age: 0,
            lfo_phase: 0.0,
            unison: [unison(1, 0.0); 2],
            filter_for: (f32::NAN, f32::NAN),
            coefficients: Coefficients::new(sample_rate, 1_000.0, 1.0),
        };
        synth.retime();
        synth
    }

    fn retime(&mut self) {
        let p = &self.params;
        self.amp_times = Times::new(
            p[param::ATTACK],
            p[param::DECAY],
            p[param::SUSTAIN],
            p[param::RELEASE],
            self.sample_rate,
        );
        self.env2_times = Times::new(
            p[param::ENV2_ATTACK],
            p[param::ENV2_DECAY],
            p[param::ENV2_SUSTAIN],
            p[param::ENV2_RELEASE],
            self.sample_rate,
        );
        self.unison = std::array::from_fn(|o| {
            unison(
                p[param::of(o, osc::UNISON)] as usize,
                p[param::of(o, osc::DETUNE)],
            )
        });
    }

    pub fn set_param(&mut self, index: usize, value: f32) -> bool {
        let Some(descriptor) = WAVETABLE_PARAMS.get(index) else {
            return false;
        };
        if !value.is_finite() {
            return false;
        }
        let value = if is_whole(index) {
            value.round()
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
        self.voices[index].start(pitch, velocity, self.age);
    }

    pub fn note_off(&mut self, pitch: u8) {
        for voice in &mut self.voices {
            if voice.active() && voice.pitch == pitch {
                voice.release();
            }
        }
    }

    fn frame(&mut self) -> [f32; 2] {
        self.started = true;
        for index in SMOOTHED {
            self.current[index] += (self.params[index] - self.current[index]) * self.smoothing;
        }
        let q = resonance_q(self.params[param::RESONANCE]);
        let filter_for = (self.current[param::CUTOFF], q);
        if filter_for != self.filter_for {
            self.filter_for = filter_for;
            self.coefficients = Coefficients::new(self.sample_rate, filter_for.0, q);
        }
        let mut lfo_push = [0.0; dest::COUNT];
        let amount = self.current[param::LFO_AMOUNT];
        if amount > 0.0 {
            lfo_push[self.choice(param::LFO_DEST).min(dest::COUNT - 1)] =
                lfo(self.choice(param::LFO_SHAPE), self.lfo_phase) * amount;
        }
        self.lfo_phase = (self.lfo_phase
            + f64::from(self.params[param::LFO_RATE]) / f64::from(self.sample_rate))
        .fract();
        let all = tables();
        let c = &self.current;
        let p = &self.params;
        let oscillator = |o: usize| {
            let (ratios, gains) = self.unison[o];
            OscFrame {
                table: &all[(p[param::of(o, osc::TABLE)] as usize).min(table::COUNT - 1)],
                position: c[param::of(o, osc::POSITION)],
                warp: c[param::of(o, osc::WARP)],
                level: c[param::of(o, osc::LEVEL)],
                ratios,
                gains,
                unison: (p[param::of(o, osc::UNISON)] as usize).clamp(1, MAX_UNISON),
                octave: p[param::of(o, osc::OCTAVE)].exp2(),
            }
        };
        let frame = Frame {
            sample_rate: self.sample_rate,
            oscillators: [oscillator(0), oscillator(1)],
            filter_type: p[param::FILTER_TYPE] as usize,
            cutoff: c[param::CUTOFF],
            q,
            coefficients: self.coefficients,
            drive: p[param::DRIVE],
            lfo: lfo_push,
            env2_amount: p[param::ENV2_AMOUNT],
            env2_dest: p[param::ENV2_DEST] as usize,
            amp_times: self.amp_times,
            env2_times: self.env2_times,
        };
        let mut sum = [0.0f32; 2];
        for voice in &mut self.voices {
            if voice.active() {
                let [left, right] = voice.generate(&frame);
                sum[0] += left;
                sum[1] += right;
            }
        }
        let output = 10f32.powf(c[param::OUTPUT_DB] / 20.0);
        [sum[0] * output, sum[1] * output]
    }

    /// Adds the synth's output to `buffer`; notes arrive through `note_on`
    /// and `note_off` beforehand. A mono buffer gets both sides' average.
    pub fn render(&mut self, buffer: &mut [f32], channels: usize) {
        let channels = channels.max(1);
        for frame in buffer.chunks_exact_mut(channels) {
            let [left, right] = self.frame();
            if channels == 1 {
                frame[0] += 0.5 * (left + right);
            } else {
                frame[0] += left;
                frame[1] += right;
            }
        }
    }

    pub fn reset(&mut self) {
        self.voices = [Voice::default(); MAX_VOICES];
        self.current = self.params;
        self.lfo_phase = 0.0;
    }
}

impl crate::Instrument for WavetableSynth {
    fn activation_sample_rate(&self) -> Option<u32> {
        Some(self.sample_rate as u32)
    }

    fn instrument_kind(&self) -> vibez_core::midi::InstrumentKind {
        vibez_core::midi::InstrumentKind::WavetableSynth
    }

    fn param_descriptors(&self) -> &'static [ParamDescriptor] {
        WAVETABLE_PARAMS
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
#[path = "wavetable_tests.rs"]
mod tests;
