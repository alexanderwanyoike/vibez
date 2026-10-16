//! The subtractive synth: a band-limited oscillator per voice into a
//! resonant filter and an ADSR, eight voices, each new note taking a free
//! voice or else the oldest.

use vibez_core::effect::ParamDescriptor;
use vibez_core::midi::MidiNote;

use crate::synth_envelope::Times;
use crate::synth_filter::Coefficients;
pub use crate::synth_oscillator::Waveform;
use crate::synth_voice::Voice;

pub mod param {
    pub const WAVEFORM: usize = 0;
    pub const ATTACK: usize = 1;
    pub const DECAY: usize = 2;
    pub const SUSTAIN: usize = 3;
    pub const RELEASE: usize = 4;
    pub const CUTOFF: usize = 5;
    pub const RESONANCE: usize = 6;
    pub const VOLUME: usize = 7;
    pub const COUNT: usize = 8;
}

/// Synth parameter descriptors.
pub static SYNTH_PARAMS: &[ParamDescriptor] = &[
    ParamDescriptor {
        name: "Waveform",
        min: 0.0,
        max: 3.0,
        default: 1.0,
        unit: "",
    },
    ParamDescriptor {
        name: "Attack",
        min: 0.001,
        max: 2.0,
        default: 0.01,
        unit: "s",
    },
    ParamDescriptor {
        name: "Decay",
        min: 0.001,
        max: 2.0,
        default: 0.1,
        unit: "s",
    },
    ParamDescriptor {
        name: "Sustain",
        min: 0.0,
        max: 1.0,
        default: 0.7,
        unit: "",
    },
    ParamDescriptor {
        name: "Release",
        min: 0.001,
        max: 5.0,
        default: 0.3,
        unit: "s",
    },
    ParamDescriptor {
        name: "Filter Cutoff",
        min: 20.0,
        max: 20000.0,
        default: 5000.0,
        unit: "Hz",
    },
    ParamDescriptor {
        name: "Filter Res",
        min: 0.1,
        max: 10.0,
        default: 1.0,
        unit: "Q",
    },
    ParamDescriptor {
        name: "Volume",
        min: 0.0,
        max: 1.0,
        default: 0.8,
        unit: "",
    },
];

const MAX_VOICES: usize = 8;
/// How fast the cutoff and volume follow an edit, so a moved knob does not
/// click or zip.
const SMOOTHING_SECONDS: f32 = 0.005;

/// A one-pole follower for a continuous control.
#[derive(Clone, Copy, Debug)]
struct Smoothed {
    current: f32,
    target: f32,
}

impl Smoothed {
    fn new(value: f32) -> Self {
        Self {
            current: value,
            target: value,
        }
    }

    fn tick(&mut self, coefficient: f32) -> f32 {
        self.current += (self.target - self.current) * coefficient;
        self.current
    }
}

pub struct SubtractiveSynth {
    voices: [Voice; MAX_VOICES],
    sample_rate: f32,
    params: [f32; param::COUNT],
    times: Times,
    cutoff: Smoothed,
    volume: Smoothed,
    smoothing: f32,
    started: bool,
    /// Counts notes, so the oldest voice is the one with the smallest age.
    age: u64,
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
            times: Times::new(0.0, 0.0, 0.0, 0.0, sample_rate),
            cutoff: Smoothed::new(params[param::CUTOFF]),
            volume: Smoothed::new(params[param::VOLUME]),
            smoothing: 1.0 - (-1.0 / (SMOOTHING_SECONDS * sample_rate)).exp(),
            started: false,
            age: 0,
        };
        synth.retime();
        synth
    }

    fn retime(&mut self) {
        let p = &self.params;
        self.times = Times::new(
            p[param::ATTACK],
            p[param::DECAY],
            p[param::SUSTAIN],
            p[param::RELEASE],
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
        let value = if index == param::WAVEFORM {
            value.round()
        } else {
            value
        }
        .clamp(descriptor.min, descriptor.max);
        self.params[index] = value;
        match index {
            param::CUTOFF => self.cutoff.target = value,
            param::VOLUME => self.volume.target = value,
            _ => self.retime(),
        }
        if !self.started {
            self.cutoff.current = self.cutoff.target;
            self.volume.current = self.volume.target;
        }
        true
    }

    pub fn get_param(&self, index: usize) -> f32 {
        self.params.get(index).copied().unwrap_or(0.0)
    }

    pub fn note_on(&mut self, pitch: u8, velocity: u8) {
        if velocity == 0 {
            self.note_off(pitch);
            return;
        }
        self.age += 1;
        let voices = &self.voices;
        let oldest = |candidates: &mut dyn Iterator<Item = usize>| {
            candidates.min_by_key(|&index| voices[index].age)
        };
        let all = 0..MAX_VOICES;
        // A repeated note takes over its own voice rather than stacking.
        let index = all
            .clone()
            .find(|&index| voices[index].active() && voices[index].pitch == pitch)
            .or_else(|| all.clone().find(|&index| !voices[index].active()))
            .or_else(|| oldest(&mut all.clone().filter(|&index| voices[index].released())))
            .or_else(|| oldest(&mut all.clone()))
            .unwrap_or(0);
        self.voices[index].start(pitch, velocity, self.age, self.sample_rate);
    }

    pub fn note_off(&mut self, pitch: u8) {
        for voice in &mut self.voices {
            if voice.active() && voice.pitch == pitch {
                voice.release();
            }
        }
    }

    fn frame(&mut self) -> f32 {
        self.started = true;
        let cutoff = self.cutoff.tick(self.smoothing);
        let volume = self.volume.tick(self.smoothing);
        let coefficients =
            Coefficients::new(self.sample_rate, cutoff, self.params[param::RESONANCE]);
        let waveform = Waveform::from_index(self.params[param::WAVEFORM] as usize);
        let mut sum = 0.0;
        for voice in &mut self.voices {
            if voice.active() {
                sum += voice.generate(waveform, &self.times, coefficients);
            }
        }
        sum * volume
    }

    /// Render notes from a clip into the given interleaved buffer.
    /// `start_beat` / `end_beat` define the time window.
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
        self.cutoff.current = self.cutoff.target;
        self.volume.current = self.volume.target;
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
    use crate::envelope::{Envelope, EnvelopeStage};

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
    fn envelope_adsr_stages() {
        let mut env = Envelope::new(1000.0); // 1kHz for easy math
        env.attack = 0.01; // 10 samples
        env.decay = 0.01;
        env.sustain = 0.5;
        env.release = 0.01;

        assert_eq!(env.stage, EnvelopeStage::Idle);
        assert!(!env.is_active());

        env.trigger();
        assert_eq!(env.stage, EnvelopeStage::Attack);
        assert!(env.is_active());

        // Run through attack (10 samples at rate 0.1/sample)
        for _ in 0..10 {
            env.tick();
        }
        // Should have reached peak
        assert!(
            env.level >= 0.9,
            "After attack, level should be near 1.0, got {}",
            env.level
        );

        // Run through decay to sustain
        for _ in 0..30 {
            env.tick();
        }
        assert!(
            (env.level - 0.5).abs() < 0.1,
            "After decay, level should be near sustain (0.5), got {}",
            env.level
        );

        env.release();
        assert_eq!(env.stage, EnvelopeStage::Release);

        // Run through release
        for _ in 0..100 {
            env.tick();
        }
        assert_eq!(env.stage, EnvelopeStage::Idle);
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
