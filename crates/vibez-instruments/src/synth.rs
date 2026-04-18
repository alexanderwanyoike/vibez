use vibez_core::effect::ParamDescriptor;
use vibez_core::midi::MidiNote;

use crate::envelope::{Envelope, EnvelopeStage};

/// Waveform shapes for the oscillator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Waveform {
    Sine,
    Saw,
    Square,
    Triangle,
}

impl Waveform {
    pub fn from_index(i: usize) -> Self {
        match i {
            0 => Waveform::Sine,
            1 => Waveform::Saw,
            2 => Waveform::Square,
            3 => Waveform::Triangle,
            _ => Waveform::Sine,
        }
    }
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

/// Single synth voice with oscillator, envelope, and per-voice filter.
#[derive(Debug, Clone)]
struct Voice {
    active: bool,
    pitch: u8,
    velocity: f32,
    phase: f64,
    phase_inc: f64,
    envelope: Envelope,
    // Per-voice one-pole low-pass filter state
    filter_state: f32,
}

impl Voice {
    fn new(sample_rate: f32) -> Self {
        Self {
            active: false,
            pitch: 0,
            velocity: 0.0,
            phase: 0.0,
            phase_inc: 0.0,
            envelope: Envelope::new(sample_rate),
            filter_state: 0.0,
        }
    }

    fn note_on(&mut self, pitch: u8, velocity: u8, sample_rate: f32) {
        self.active = true;
        self.pitch = pitch;
        self.velocity = velocity as f32 / 127.0;
        let freq = 440.0 * 2.0_f64.powf((pitch as f64 - 69.0) / 12.0);
        self.phase_inc = freq / sample_rate as f64;
        self.envelope.trigger();
    }

    fn note_off(&mut self) {
        self.envelope.release();
    }

    fn generate(&mut self, waveform: Waveform, cutoff_coeff: f32) -> f32 {
        if !self.active {
            return 0.0;
        }

        let env = self.envelope.tick();
        if !self.envelope.is_active() {
            self.active = false;
            return 0.0;
        }

        let raw = match waveform {
            Waveform::Sine => (self.phase * std::f64::consts::TAU).sin() as f32,
            Waveform::Saw => (2.0 * self.phase - 1.0) as f32,
            Waveform::Square => {
                if self.phase < 0.5 {
                    1.0
                } else {
                    -1.0
                }
            }
            Waveform::Triangle => (4.0 * (self.phase - 0.5).abs() - 1.0) as f32,
        };

        self.phase += self.phase_inc;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }

        // One-pole low-pass filter
        let filtered = self.filter_state + cutoff_coeff * (raw - self.filter_state);
        self.filter_state = filtered;

        filtered * env * self.velocity
    }
}

/// 8-voice polyphonic subtractive synthesizer.
pub struct SubtractiveSynth {
    voices: Vec<Voice>,
    sample_rate: f32,
    // Parameters
    waveform: Waveform,
    attack: f32,
    decay: f32,
    sustain: f32,
    release: f32,
    filter_cutoff: f32,
    filter_resonance: f32,
    volume: f32,
}

impl SubtractiveSynth {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            voices: (0..MAX_VOICES).map(|_| Voice::new(sample_rate)).collect(),
            sample_rate,
            waveform: Waveform::Saw,
            attack: 0.01,
            decay: 0.1,
            sustain: 0.7,
            release: 0.3,
            filter_cutoff: 5000.0,
            filter_resonance: 1.0,
            volume: 0.8,
        }
    }

    pub fn param_descriptors(&self) -> &'static [ParamDescriptor] {
        SYNTH_PARAMS
    }

    pub fn set_param(&mut self, index: usize, value: f32) -> bool {
        match index {
            0 => {
                self.waveform = Waveform::from_index(value.round() as usize);
                true
            }
            1 => {
                self.attack = value.clamp(0.001, 2.0);
                true
            }
            2 => {
                self.decay = value.clamp(0.001, 2.0);
                true
            }
            3 => {
                self.sustain = value.clamp(0.0, 1.0);
                true
            }
            4 => {
                self.release = value.clamp(0.001, 5.0);
                true
            }
            5 => {
                self.filter_cutoff = value.clamp(20.0, 20000.0);
                true
            }
            6 => {
                self.filter_resonance = value.clamp(0.1, 10.0);
                true
            }
            7 => {
                self.volume = value.clamp(0.0, 1.0);
                true
            }
            _ => false,
        }
    }

    pub fn get_param(&self, index: usize) -> f32 {
        match index {
            0 => self.waveform as u8 as f32,
            1 => self.attack,
            2 => self.decay,
            3 => self.sustain,
            4 => self.release,
            5 => self.filter_cutoff,
            6 => self.filter_resonance,
            7 => self.volume,
            _ => 0.0,
        }
    }

    pub fn note_on(&mut self, pitch: u8, velocity: u8) {
        // Update envelopes on all voices first
        for voice in &mut self.voices {
            voice.envelope.attack = self.attack;
            voice.envelope.decay = self.decay;
            voice.envelope.sustain = self.sustain;
            voice.envelope.release = self.release;
        }

        // Find free voice or steal oldest
        let voice_idx = self.voices.iter().position(|v| !v.active).unwrap_or(0); // steal voice 0 if all active

        self.voices[voice_idx].note_on(pitch, velocity, self.sample_rate);
    }

    pub fn note_off(&mut self, pitch: u8) {
        for voice in &mut self.voices {
            if voice.active && voice.pitch == pitch {
                voice.note_off();
            }
        }
    }

    /// Render notes from a clip into the given stereo interleaved buffer.
    /// `start_beat` / `end_beat` define the time window.
    /// `bpm` is beats per minute for time conversion.
    pub fn render_block(
        &mut self,
        buffer: &mut [f32],
        channels: usize,
        notes: &[MidiNote],
        start_beat: f64,
        bpm: f64,
    ) {
        let ch = channels.clamp(1, 2);
        let frames = buffer.len() / ch;
        let beats_per_sample = bpm / (60.0 * self.sample_rate as f64);

        // Compute cutoff coefficient from frequency
        let cutoff_coeff =
            (2.0 * std::f32::consts::PI * self.filter_cutoff / self.sample_rate).clamp(0.0, 1.0);

        for frame in 0..frames {
            let current_beat = start_beat + frame as f64 * beats_per_sample;

            // Trigger note-ons and note-offs
            for note in notes {
                let note_on_beat = note.start_beat;
                let note_off_beat = note.end_beat();
                let prev_beat = current_beat - beats_per_sample;

                if note_on_beat > prev_beat && note_on_beat <= current_beat {
                    self.note_on(note.pitch, note.velocity);
                }
                if note_off_beat > prev_beat && note_off_beat <= current_beat {
                    self.note_off(note.pitch);
                }
            }

            // Sum voices
            let mut sample = 0.0_f32;
            for voice in &mut self.voices {
                sample += voice.generate(self.waveform, cutoff_coeff);
            }
            sample *= self.volume;

            // Write to buffer
            for c in 0..ch {
                buffer[frame * ch + c] += sample;
            }
        }
    }

    /// Render a single frame (or small buffer slice) of audio into the buffer.
    /// This assumes note_on/note_off have already been called for this frame.
    pub fn render(&mut self, buffer: &mut [f32], channels: usize) {
        let ch = channels.clamp(1, 2);
        let frames = buffer.len() / ch;

        let cutoff_coeff =
            (2.0 * std::f32::consts::PI * self.filter_cutoff / self.sample_rate).clamp(0.0, 1.0);

        for frame in 0..frames {
            let mut sample = 0.0_f32;
            for voice in &mut self.voices {
                sample += voice.generate(self.waveform, cutoff_coeff);
            }
            sample *= self.volume;

            for c in 0..ch {
                buffer[frame * ch + c] += sample;
            }
        }
    }

    pub fn reset(&mut self) {
        for voice in &mut self.voices {
            voice.active = false;
            voice.envelope.stage = EnvelopeStage::Idle;
            voice.envelope.level = 0.0;
            voice.filter_state = 0.0;
            voice.phase = 0.0;
        }
    }
}

impl crate::Instrument for SubtractiveSynth {
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
            assert!(!voice.active);
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
