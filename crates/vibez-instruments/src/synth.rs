//! The subtractive synth: two band-limited oscillators, a sub and noise
//! per voice into a resonant filter, with an envelope each for level and
//! cutoff and one LFO. Poly takes a free voice for each note, or else the
//! oldest; Mono and Legato play the last held note on one voice.

use vibez_core::effect::ParamDescriptor;
use vibez_core::midi::MidiNote;

use crate::synth_envelope::Times;
use crate::synth_filter::Coefficients;
pub use crate::synth_oscillator::Waveform;
pub use crate::synth_params::{
    filter_type, is_whole, lfo_shape, lfo_target, param, voice_mode, SYNTH_PARAMS,
};
use crate::synth_params::{LFO_CUTOFF_OCTAVES, LFO_PITCH_SEMITONES, MAX_VOICES};
use crate::synth_voice::{Frame, Voice};

/// How fast continuous levels follow an edit, so a moved knob does not
/// click or zip.
const SMOOTHING_SECONDS: f32 = 0.005;
/// Held notes Mono and Legato remember to fall back to.
const HELD_NOTES: usize = 16;
const SMOOTHED: [usize; 6] = [
    param::CUTOFF,
    param::VOLUME,
    param::LEVEL_2,
    param::SUB,
    param::NOISE,
    param::LFO_AMOUNT,
];

/// The LFO, -1 to 1, at `phase` cycles.
pub fn lfo(shape: usize, phase: f64) -> f32 {
    let x = phase.rem_euclid(1.0) as f32;
    match shape {
        lfo_shape::TRIANGLE => triangle(x),
        lfo_shape::SQUARE => {
            if x < 0.5 {
                1.0
            } else {
                -1.0
            }
        }
        lfo_shape::SAW => 2.0 * x - 1.0,
        _ => (std::f32::consts::TAU * x).sin(),
    }
}

/// The filter's gain in dB at `frequency`: the trapezoidal SVF is the
/// analogue prototype at the prewarped frequency, so this is exact.
pub fn filter_response_db(params: &[f32], frequency: f32, sample_rate: f32) -> f32 {
    let value = |index: usize| {
        params
            .get(index)
            .copied()
            .unwrap_or(SYNTH_PARAMS[index].default)
    };
    let nyquist = sample_rate * 0.5;
    let warp = |hz: f32| (std::f32::consts::PI * hz.min(nyquist * 0.999) / sample_rate).tan();
    let cutoff = value(param::CUTOFF).min(sample_rate * 0.45);
    let w = f64::from(warp(frequency) / warp(cutoff));
    let response = |q: f32| {
        let k = 1.0 / f64::from(q.max(0.05));
        // 1 / (s^2 + k s + 1) at s = jw.
        let (re, im) = (1.0 - w * w, k * w);
        (re * re + im * im, k)
    };
    let (denominator, k) = response(value(param::RESONANCE));
    let power = match value(param::FILTER_TYPE) as usize {
        filter_type::LP24 => {
            let (second, _) = response(std::f32::consts::FRAC_1_SQRT_2);
            1.0 / (denominator * second)
        }
        filter_type::HP12 => w.powi(4) / denominator,
        filter_type::BP12 => (k * w).powi(2) / denominator,
        _ => 1.0 / denominator,
    };
    (10.0 * power.max(1e-12).log10()) as f32
}

/// A triangle in step with a sine: 0 rising at the cycle's start.
fn triangle(x: f32) -> f32 {
    4.0 * ((x - 0.25).rem_euclid(1.0) - 0.5).abs() - 1.0
}

pub struct SubtractiveSynth {
    voices: [Voice; MAX_VOICES],
    sample_rate: f32,
    params: [f32; param::COUNT],
    /// The smoothed controls' current values; the rest read `params`.
    current: [f32; param::COUNT],
    amp_times: Times,
    filter_times: Times,
    smoothing: f32,
    started: bool,
    /// Counts notes, so the oldest voice is the one with the smallest age.
    age: u64,
    /// Notes held down, oldest first, with their velocities.
    held: [(u8, u8); HELD_NOTES],
    held_count: usize,
    last_pitch: Option<f32>,
    lfo_phase: f64,
    /// Each oscillator's frequency against the note's.
    ratios: (f64, f64),
    /// The cutoff and Q `coefficients` were worked out for.
    filter_for: (f32, f32),
    coefficients: Coefficients,
}

impl SubtractiveSynth {
    pub fn new(sample_rate: f32) -> Self {
        let sample_rate = if sample_rate.is_finite() && sample_rate > 0.0 {
            sample_rate
        } else {
            48_000.0
        };
        let params: [f32; param::COUNT] = std::array::from_fn(|index| SYNTH_PARAMS[index].default);
        let mut synth = Self {
            voices: [Voice::default(); MAX_VOICES],
            sample_rate,
            params,
            current: params,
            amp_times: Times::new(0.0, 0.0, 0.0, 0.0, sample_rate),
            filter_times: Times::new(0.0, 0.0, 0.0, 0.0, sample_rate),
            smoothing: 1.0 - (-1.0 / (SMOOTHING_SECONDS * sample_rate)).exp(),
            started: false,
            age: 0,
            held: [(0, 0); HELD_NOTES],
            held_count: 0,
            last_pitch: None,
            lfo_phase: 0.0,
            ratios: (1.0, 1.0),
            filter_for: (f32::NAN, f32::NAN),
            coefficients: Coefficients::new(sample_rate, 1_000.0, 1.0),
        };
        synth.retime();
        synth
    }

    fn retime(&mut self) {
        let p = &self.params;
        let ratio = |octave: usize, detune: usize| f64::from(p[octave] + p[detune] / 12.0).exp2();
        self.ratios = (
            ratio(param::OCTAVE, param::DETUNE),
            ratio(param::OCTAVE_2, param::DETUNE_2),
        );
        self.amp_times = Times::new(
            p[param::ATTACK],
            p[param::DECAY],
            p[param::SUSTAIN],
            p[param::RELEASE],
            self.sample_rate,
        );
        self.filter_times = Times::new(
            p[param::FILTER_ATTACK],
            p[param::FILTER_DECAY],
            p[param::FILTER_SUSTAIN],
            p[param::FILTER_RELEASE],
            self.sample_rate,
        );
    }

    pub fn param_descriptors(&self) -> &'static [ParamDescriptor] {
        SYNTH_PARAMS
    }

    pub fn set_param(&mut self, index: usize, value: f32) -> bool {
        let Some(descriptor) = SYNTH_PARAMS.get(index) else {
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

    fn glide_frames(&self) -> u32 {
        (self.params[param::GLIDE] / 1000.0 * self.sample_rate).round() as u32
    }

    pub fn note_on(&mut self, pitch: u8, velocity: u8) {
        if velocity == 0 {
            self.note_off(pitch);
            return;
        }
        self.age += 1;
        self.forget(pitch);
        if self.held_count == HELD_NOTES {
            self.held.copy_within(1.., 0);
            self.held_count -= 1;
        }
        self.held[self.held_count] = (pitch, velocity);
        self.held_count += 1;
        let from = self.last_pitch;
        self.last_pitch = Some(f32::from(pitch));
        let glide = self.glide_frames();
        let mode = self.choice(param::VOICE_MODE);
        if mode != voice_mode::POLY {
            let voice = &mut self.voices[0];
            let from = if voice.active() {
                Some(voice.sounding())
            } else {
                from
            };
            let legato = mode == voice_mode::LEGATO && self.held_count > 1 && voice.active();
            voice.start(pitch, velocity, self.age, from, glide, !legato);
            return;
        }
        let count = self.choice(param::VOICES).clamp(1, MAX_VOICES);
        let voices = &self.voices[..count];
        let oldest = |candidates: &mut dyn Iterator<Item = usize>| {
            candidates.min_by_key(|&index| voices[index].age)
        };
        let all = 0..count;
        // A repeated note takes over its own voice rather than stacking.
        let index = all
            .clone()
            .find(|&index| voices[index].active() && voices[index].pitch == pitch)
            .or_else(|| all.clone().find(|&index| !voices[index].active()))
            .or_else(|| oldest(&mut all.clone().filter(|&index| voices[index].released())))
            .or_else(|| oldest(&mut all.clone()))
            .unwrap_or(0);
        self.voices[index].start(pitch, velocity, self.age, from, glide, true);
    }

    fn forget(&mut self, pitch: u8) -> bool {
        let Some(index) = self.held[..self.held_count]
            .iter()
            .position(|&(held, _)| held == pitch)
        else {
            return false;
        };
        self.held.copy_within(index + 1..self.held_count, index);
        self.held_count -= 1;
        true
    }

    pub fn note_off(&mut self, pitch: u8) {
        let was_held = self.forget(pitch);
        let mode = self.choice(param::VOICE_MODE);
        if mode != voice_mode::POLY
            && was_held
            && self.voices[0].active()
            && self.voices[0].pitch == pitch
        {
            // Back to the latest note still held.
            if let Some(&(previous, velocity)) = self.held[..self.held_count].last() {
                self.age += 1;
                let from = Some(self.voices[0].sounding());
                let glide = self.glide_frames();
                self.voices[0].start(
                    previous,
                    velocity,
                    self.age,
                    from,
                    glide,
                    mode == voice_mode::MONO,
                );
                self.last_pitch = Some(f32::from(previous));
                return;
            }
        }
        for voice in &mut self.voices {
            if voice.active() && voice.pitch == pitch {
                voice.release();
            }
        }
    }

    fn frame(&mut self) -> f32 {
        self.started = true;
        for index in SMOOTHED {
            self.current[index] += (self.params[index] - self.current[index]) * self.smoothing;
        }
        let filter_for = (self.current[param::CUTOFF], self.params[param::RESONANCE]);
        if filter_for != self.filter_for {
            self.filter_for = filter_for;
            self.coefficients = Coefficients::new(self.sample_rate, filter_for.0, filter_for.1);
        }
        let p = &self.params;
        let c = &self.current;
        let modulation = if c[param::LFO_AMOUNT] == 0.0 {
            0.0
        } else {
            lfo(self.choice(param::LFO_SHAPE), self.lfo_phase) * c[param::LFO_AMOUNT]
        };
        self.lfo_phase =
            (self.lfo_phase + f64::from(p[param::LFO_RATE]) / f64::from(self.sample_rate)).fract();
        let target = self.choice(param::LFO_TARGET);
        let frame = Frame {
            sample_rate: self.sample_rate,
            waveform: Waveform::from_index(p[param::WAVEFORM] as usize),
            waveform_2: Waveform::from_index(p[param::WAVEFORM_2] as usize),
            ratio: self.ratios.0,
            ratio_2: self.ratios.1,
            level_2: c[param::LEVEL_2],
            sub: c[param::SUB],
            noise: c[param::NOISE],
            filter_type: p[param::FILTER_TYPE] as usize,
            cutoff: c[param::CUTOFF],
            resonance: p[param::RESONANCE],
            coefficients: self.coefficients,
            filter_env: p[param::FILTER_ENV],
            cutoff_octaves: if target == lfo_target::CUTOFF {
                modulation * LFO_CUTOFF_OCTAVES
            } else {
                0.0
            },
            pitch_semitones: if target == lfo_target::PITCH {
                modulation * LFO_PITCH_SEMITONES
            } else {
                0.0
            },
            amp_times: self.amp_times,
            filter_times: self.filter_times,
        };
        let mut sum = 0.0;
        for voice in &mut self.voices {
            if voice.active() {
                sum += voice.generate(&frame);
            }
        }
        let tremolo = if target == lfo_target::VOLUME {
            1.0 - c[param::LFO_AMOUNT] * 0.5 + modulation * 0.5
        } else {
            1.0
        };
        sum * c[param::VOLUME] * tremolo
    }

    /// Render notes from a clip into the given interleaved buffer.
    pub fn render_block(
        &mut self,
        buffer: &mut [f32],
        channels: usize,
        notes: &[MidiNote],
        start_beat: f64,
        bpm: f64,
    ) {
        let channels = channels.max(1);
        let beats_per_sample = bpm / (60.0 * self.sample_rate as f64);
        for (index, frame) in buffer.chunks_exact_mut(channels).enumerate() {
            let beat = start_beat + index as f64 * beats_per_sample;
            let previous = beat - beats_per_sample;
            for note in notes {
                if note.start_beat > previous && note.start_beat <= beat {
                    self.note_on(note.pitch, note.velocity);
                }
                if note.end_beat() > previous && note.end_beat() <= beat {
                    self.note_off(note.pitch);
                }
            }
            let sample = self.frame();
            for output in frame.iter_mut().take(2) {
                *output += sample;
            }
        }
    }

    /// Adds the synth's output to `buffer`; notes arrive through `note_on`
    /// and `note_off` beforehand.
    pub fn render(&mut self, buffer: &mut [f32], channels: usize) {
        for frame in buffer.chunks_exact_mut(channels.max(1)) {
            let sample = self.frame();
            for output in frame.iter_mut().take(2) {
                *output += sample;
            }
        }
    }

    pub fn reset(&mut self) {
        self.voices = [Voice::default(); MAX_VOICES];
        self.current = self.params;
        self.held_count = 0;
        self.last_pitch = None;
        self.lfo_phase = 0.0;
    }
}

impl crate::Instrument for SubtractiveSynth {
    fn activation_sample_rate(&self) -> Option<u32> {
        Some(self.sample_rate as u32)
    }

    fn instrument_kind(&self) -> vibez_core::midi::InstrumentKind {
        vibez_core::midi::InstrumentKind::SubtractiveSynth
    }

    fn param_descriptors(&self) -> &'static [ParamDescriptor] {
        self.param_descriptors()
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
mod tests {
    use super::*;

    #[test]
    fn synth_silence_without_notes() {
        let mut synth = SubtractiveSynth::new(44100.0);
        let mut buf = vec![0.0_f32; 512];
        synth.render_block(&mut buf, 2, &[], 0.0, 120.0);
        for s in &buf {
            assert!(s.abs() < 1e-6);
        }
    }

    #[test]
    fn synth_produces_sound() {
        let mut synth = SubtractiveSynth::new(44100.0);
        let notes = vec![MidiNote {
            pitch: 69,
            velocity: 100,
            start_beat: 0.0,
            duration_beats: 4.0,
        }];
        let mut buf = vec![0.0_f32; 1024];
        synth.render_block(&mut buf, 2, &notes, 0.0, 120.0);
        let energy: f32 = buf.iter().map(|s| s * s).sum();
        assert!(energy > 0.1, "Synth should produce sound, energy={energy}");
    }

    #[test]
    fn synth_note_off_releases() {
        let mut synth = SubtractiveSynth::new(44100.0);
        synth.set_param(4, 0.01); // very short release

        let notes = vec![MidiNote {
            pitch: 60,
            velocity: 100,
            start_beat: 0.0,
            duration_beats: 0.001, // very short note
        }];
        // Render enough for the note to start and release
        let mut buf = vec![0.0_f32; 4410]; // 50ms at 44100
        synth.render_block(&mut buf, 2, &notes, 0.0, 120.0);

        // Last samples should be near silent
        let tail_energy: f32 = buf[4000..].iter().map(|s| s * s).sum();
        assert!(
            tail_energy < 0.01,
            "Tail should be quiet after release, energy={tail_energy}"
        );
    }

    #[test]
    fn synth_polyphony() {
        let mut synth = SubtractiveSynth::new(44100.0);
        let notes = vec![
            MidiNote {
                pitch: 60,
                velocity: 100,
                start_beat: 0.0,
                duration_beats: 4.0,
            },
            MidiNote {
                pitch: 64,
                velocity: 100,
                start_beat: 0.0,
                duration_beats: 4.0,
            },
            MidiNote {
                pitch: 67,
                velocity: 100,
                start_beat: 0.0,
                duration_beats: 4.0,
            },
        ];
        let mut buf = vec![0.0_f32; 1024];
        synth.render_block(&mut buf, 2, &notes, 0.0, 120.0);
        let energy: f32 = buf.iter().map(|s| s * s).sum();
        assert!(energy > 0.3, "Chord should be louder than single note");
    }

    #[test]
    fn synth_waveforms() {
        for wf_idx in 0..4 {
            let mut synth = SubtractiveSynth::new(44100.0);
            synth.set_param(0, wf_idx as f32);
            let notes = vec![MidiNote {
                pitch: 69,
                velocity: 100,
                start_beat: 0.0,
                duration_beats: 4.0,
            }];
            let mut buf = vec![0.0_f32; 512];
            synth.render_block(&mut buf, 2, &notes, 0.0, 120.0);
            let energy: f32 = buf.iter().map(|s| s * s).sum();
            assert!(
                energy > 0.01,
                "Waveform {wf_idx} should produce sound, energy={energy}"
            );
        }
    }

    #[test]
    fn synth_param_access() {
        let mut synth = SubtractiveSynth::new(44100.0);
        // Waveform default = Saw = 1
        assert!((synth.get_param(0) - 1.0).abs() < 1e-3);
        synth.set_param(0, 0.0); // Sine
        assert!((synth.get_param(0) - 0.0).abs() < 1e-3);

        // Volume
        assert!((synth.get_param(7) - 0.8).abs() < 1e-3);
        synth.set_param(7, 0.5);
        assert!((synth.get_param(7) - 0.5).abs() < 1e-3);

        // Invalid
        assert!(!synth.set_param(99, 1.0));
        assert!((synth.get_param(99) - 0.0).abs() < 1e-3);
    }

    #[test]
    fn synth_reset() {
        let mut synth = SubtractiveSynth::new(44100.0);
        synth.note_on(60, 100);
        synth.reset();
        for voice in &synth.voices {
            assert!(!voice.active());
        }
    }

    #[test]
    fn synth_velocity_scaling() {
        let mut synth_loud = SubtractiveSynth::new(44100.0);
        let mut synth_soft = SubtractiveSynth::new(44100.0);

        let notes_loud = vec![MidiNote {
            pitch: 69,
            velocity: 127,
            start_beat: 0.0,
            duration_beats: 4.0,
        }];
        let notes_soft = vec![MidiNote {
            pitch: 69,
            velocity: 32,
            start_beat: 0.0,
            duration_beats: 4.0,
        }];

        let mut buf_loud = vec![0.0_f32; 512];
        let mut buf_soft = vec![0.0_f32; 512];
        synth_loud.render_block(&mut buf_loud, 2, &notes_loud, 0.0, 120.0);
        synth_soft.render_block(&mut buf_soft, 2, &notes_soft, 0.0, 120.0);

        let energy_loud: f32 = buf_loud.iter().map(|s| s * s).sum();
        let energy_soft: f32 = buf_soft.iter().map(|s| s * s).sum();
        assert!(
            energy_loud > energy_soft,
            "Louder velocity should produce more energy"
        );
    }

    #[test]
    fn synth_mono_output() {
        let mut synth = SubtractiveSynth::new(44100.0);
        let notes = vec![MidiNote {
            pitch: 69,
            velocity: 100,
            start_beat: 0.0,
            duration_beats: 4.0,
        }];
        let mut buf = vec![0.0_f32; 256];
        synth.render_block(&mut buf, 1, &notes, 0.0, 120.0);
        let energy: f32 = buf.iter().map(|s| s * s).sum();
        assert!(energy > 0.01, "Mono output should produce sound");
    }

    #[test]
    fn waveform_from_index() {
        assert_eq!(Waveform::from_index(0), Waveform::Sine);
        assert_eq!(Waveform::from_index(1), Waveform::Saw);
        assert_eq!(Waveform::from_index(2), Waveform::Square);
        assert_eq!(Waveform::from_index(3), Waveform::Triangle);
        assert_eq!(Waveform::from_index(99), Waveform::Sine); // fallback
    }
}

#[cfg(test)]
#[path = "synth_quality_tests.rs"]
mod quality_tests;

#[cfg(test)]
#[path = "synth_controls_tests.rs"]
mod controls_tests;
