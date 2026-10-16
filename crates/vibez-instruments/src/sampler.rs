//! The sampler: one loaded sample played across the keyboard by changing
//! its speed, through an ADSR, eight voices, each new note taking a free
//! voice or else the oldest.

use std::sync::Arc;

use vibez_core::audio_buffer::DecodedAudio;
use vibez_core::effect::ParamDescriptor;
use vibez_core::midi::InstrumentKind;

use crate::synth_envelope::{Envelope, Times};
use crate::Instrument;

/// Sampler parameter descriptors.
pub static SAMPLER_PARAMS: &[ParamDescriptor] = &[
    ParamDescriptor {
        name: "Root Note",
        min: 0.0,
        max: 127.0,
        default: 60.0,
        unit: "note",
    },
    ParamDescriptor {
        name: "Attack",
        min: 0.001,
        max: 2.0,
        default: 0.001,
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
        default: 1.0,
        unit: "",
    },
    ParamDescriptor {
        name: "Release",
        min: 0.001,
        max: 5.0,
        default: 0.01,
        unit: "s",
    },
    ParamDescriptor {
        name: "Start",
        min: 0.0,
        max: 1.0,
        default: 0.0,
        unit: "",
    },
    ParamDescriptor {
        name: "End",
        min: 0.0,
        max: 1.0,
        default: 1.0,
        unit: "",
    },
    ParamDescriptor {
        name: "Loop",
        min: 0.0,
        max: 1.0,
        default: 0.0,
        unit: "",
    },
    ParamDescriptor {
        name: "One-Shot",
        min: 0.0,
        max: 1.0,
        default: 0.0,
        unit: "",
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
/// A sample that stops before its loop or its own end fades out over this
/// long, so stopping mid-sound does not click.
const END_FADE_SECONDS: f64 = 0.002;

/// Parameter indices for the sampler.
const P_ROOT_NOTE: usize = 0;
const P_ATTACK: usize = 1;
const P_DECAY: usize = 2;
const P_SUSTAIN: usize = 3;
const P_RELEASE: usize = 4;
const P_START: usize = 5;
const P_END: usize = 6;
const P_LOOP: usize = 7;
const P_ONE_SHOT: usize = 8;
const P_VOLUME: usize = 9;
const COUNT: usize = 10;

#[derive(Debug, Clone, Copy, Default)]
struct SamplerVoice {
    pitch: u8,
    /// The note count when this voice started; smallest is oldest.
    age: u64,
    velocity: f32,
    position: f64,
    /// Source frames a host frame, for the note and the file's own rate.
    speed: f64,
    envelope: Envelope,
    one_shot: bool,
}

impl SamplerVoice {
    fn active(&self) -> bool {
        self.envelope.active()
    }
}

/// Polyphonic sample-playback instrument.
pub struct Sampler {
    voices: [SamplerVoice; MAX_VOICES],
    sample_rate: f32,
    sample: Option<Arc<DecodedAudio>>,
    sample_name: Option<String>,
    params: [f32; COUNT],
    times: Times,
    age: u64,
}

impl Sampler {
    pub fn new(sample_rate: f32) -> Self {
        let sample_rate = if sample_rate.is_finite() && sample_rate > 0.0 {
            sample_rate
        } else {
            48_000.0
        };
        let params: [f32; COUNT] = std::array::from_fn(|index| SAMPLER_PARAMS[index].default);
        let mut sampler = Self {
            voices: [SamplerVoice::default(); MAX_VOICES],
            sample_rate,
            sample: None,
            sample_name: None,
            params,
            times: Times::new(0.0, 0.0, 0.0, 0.0, sample_rate),
            age: 0,
        };
        sampler.retime();
        sampler
    }

    fn retime(&mut self) {
        let p = &self.params;
        self.times = Times::new(
            p[P_ATTACK],
            p[P_DECAY],
            p[P_SUSTAIN],
            p[P_RELEASE],
            self.sample_rate,
        );
    }

    fn frame_at(&self, position: f32) -> usize {
        self.sample
            .as_ref()
            .map_or(0, |sample| (position * sample.num_frames() as f32) as usize)
    }
}

/// Linear interpolation sample read from audio data.
fn read_sample(audio: &DecodedAudio, position: f64, channel: usize) -> f32 {
    let num_channels = audio.num_channels();
    if num_channels == 0 {
        return 0.0;
    }
    let ch = channel.min(num_channels - 1);
    let idx = position as usize;
    let frac = (position - idx as f64) as f32;
    let s0 = audio.sample(ch, idx);
    let s1 = audio.sample(ch, idx + 1);
    s0 + frac * (s1 - s0)
}

impl Instrument for Sampler {
    fn activation_sample_rate(&self) -> Option<u32> {
        Some(self.sample_rate as u32)
    }

    fn instrument_kind(&self) -> InstrumentKind {
        InstrumentKind::Sampler
    }

    fn param_descriptors(&self) -> &'static [ParamDescriptor] {
        SAMPLER_PARAMS
    }

    fn set_param(&mut self, index: usize, value: f32) -> bool {
        let Some(descriptor) = SAMPLER_PARAMS.get(index) else {
            return false;
        };
        if !value.is_finite() {
            return false;
        }
        self.params[index] = match index {
            P_LOOP | P_ONE_SHOT => f32::from(value >= 0.5),
            _ => value.clamp(descriptor.min, descriptor.max),
        };
        self.retime();
        true
    }

    fn get_param(&self, index: usize) -> f32 {
        self.params.get(index).copied().unwrap_or(0.0)
    }

    fn note_on(&mut self, pitch: u8, velocity: u8) {
        let Some(sample) = self.sample.as_ref() else {
            return;
        };
        if velocity == 0 {
            self.note_off(pitch);
            return;
        }
        let file_rate = f64::from(sample.sample_rate.max(1));
        let speed = ((f64::from(pitch) - f64::from(self.params[P_ROOT_NOTE])) / 12.0).exp2()
            * file_rate
            / f64::from(self.sample_rate);
        let start = self.frame_at(self.params[P_START]) as f64;
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
            .or_else(|| {
                oldest(
                    &mut all
                        .clone()
                        .filter(|&index| voices[index].envelope.released()),
                )
            })
            .or_else(|| oldest(&mut all.clone()))
            .unwrap_or(0);
        let voice = &mut self.voices[index];
        voice.pitch = pitch;
        voice.age = self.age;
        voice.velocity = f32::from(velocity) / 127.0;
        voice.position = start;
        voice.speed = speed;
        voice.one_shot = self.params[P_ONE_SHOT] >= 0.5;
        voice.envelope.trigger();
    }

    fn note_off(&mut self, pitch: u8) {
        for voice in &mut self.voices {
            if voice.active() && voice.pitch == pitch && !voice.one_shot {
                voice.envelope.release();
            }
        }
    }

    fn render(&mut self, buffer: &mut [f32], channels: usize) {
        let Some(audio) = self.sample.as_ref().map(Arc::clone) else {
            return;
        };
        if channels == 0 {
            return;
        }
        let start = self.frame_at(self.params[P_START]) as f64;
        let end = self.frame_at(self.params[P_END]) as f64;
        if end <= start {
            return;
        }
        let looping = self.params[P_LOOP] >= 0.5;
        let volume = self.params[P_VOLUME];
        let fade = END_FADE_SECONDS * f64::from(self.sample_rate);
        for frame in buffer.chunks_exact_mut(channels) {
            let (mut left, mut right) = (0.0f32, 0.0f32);
            for voice in &mut self.voices {
                if !voice.active() {
                    continue;
                }
                let level = voice.envelope.tick(&self.times);
                let mut gain = level * voice.velocity * volume;
                if !looping {
                    let left_frames = (end - voice.position) / voice.speed;
                    gain *= (left_frames / fade).clamp(0.0, 1.0) as f32;
                }
                left += read_sample(&audio, voice.position, 0) * gain;
                right += read_sample(&audio, voice.position, 1) * gain;
                voice.position += voice.speed;
                if voice.position >= end {
                    if looping {
                        voice.position = start + (voice.position - start) % (end - start);
                    } else {
                        voice.envelope = Envelope::default();
                    }
                }
            }
            frame[0] += left;
            if let Some(second) = frame.get_mut(1) {
                *second += right;
            }
        }
    }

    fn reset(&mut self) {
        self.voices = [SamplerVoice::default(); MAX_VOICES];
    }

    fn load_sample(&mut self, sample: Arc<DecodedAudio>, name: String) {
        self.sample = Some(sample);
        self.sample_name = Some(name);
        self.reset();
    }

    fn sample_name(&self) -> Option<&str> {
        self.sample_name.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_sample(frames: usize, value: f32) -> Arc<DecodedAudio> {
        Arc::new(DecodedAudio {
            channels: vec![vec![value; frames], vec![value; frames]],
            sample_rate: 44_100,
        })
    }

    fn make_sine_sample(frames: usize, freq: f64, sample_rate: u32) -> Arc<DecodedAudio> {
        let data: Vec<f32> = (0..frames)
            .map(|i| {
                (2.0 * std::f64::consts::PI * freq * i as f64 / sample_rate as f64).sin() as f32
            })
            .collect();
        Arc::new(DecodedAudio {
            channels: vec![data.clone(), data],
            sample_rate,
        })
    }

    #[test]
    fn silence_without_sample() {
        let mut sampler = Sampler::new(44100.0);
        sampler.note_on(60, 100);
        let mut buf = vec![0.0_f32; 512];
        sampler.render(&mut buf, 2);
        for s in &buf {
            assert!(s.abs() < 1e-6, "Expected silence without sample, got {s}");
        }
    }

    #[test]
    fn produces_sound_with_sample() {
        let mut sampler = Sampler::new(44100.0);
        let sample = make_test_sample(44100, 0.5);
        sampler.load_sample(sample, "test.wav".into());
        sampler.note_on(60, 100);

        let mut buf = vec![0.0_f32; 512];
        sampler.render(&mut buf, 2);
        let energy: f32 = buf.iter().map(|s| s * s).sum();
        assert!(
            energy > 0.01,
            "Sampler should produce sound, energy={energy}"
        );
    }

    #[test]
    fn pitch_shift_octave_up() {
        let mut sampler = Sampler::new(44100.0);
        let sample = make_sine_sample(44100, 440.0, 44100);
        sampler.load_sample(sample, "sine.wav".into());
        sampler.set_param(P_ROOT_NOTE, 60.0);

        // Play at pitch 72 (one octave up from root 60) → speed should be 2x
        sampler.note_on(72, 100);

        // After rendering some frames, check the voice speed
        assert!(
            sampler
                .voices
                .iter()
                .any(|v| v.active() && (v.speed - 2.0).abs() < 1e-6),
            "Octave up should have speed 2.0"
        );
    }

    #[test]
    fn original_pitch_at_root() {
        let mut sampler = Sampler::new(44100.0);
        let sample = make_test_sample(44100, 0.5);
        sampler.load_sample(sample, "test.wav".into());
        sampler.set_param(P_ROOT_NOTE, 60.0);

        sampler.note_on(60, 100);

        assert!(
            sampler
                .voices
                .iter()
                .any(|v| v.active() && (v.speed - 1.0).abs() < 1e-6),
            "Root note should have speed 1.0"
        );
    }

    #[test]
    fn one_shot_ignores_note_off() {
        let mut sampler = Sampler::new(44100.0);
        let sample = make_test_sample(44100, 0.5);
        sampler.load_sample(sample, "test.wav".into());
        sampler.set_param(P_ONE_SHOT, 1.0);

        sampler.note_on(60, 100);
        sampler.note_off(60);

        // Voice should still be active (one-shot ignores note_off)
        let active_count = sampler.voices.iter().filter(|v| v.active()).count();
        assert_eq!(
            active_count, 1,
            "One-shot should keep voice active after note_off"
        );

        // Render some frames — should still produce sound
        let mut buf = vec![0.0_f32; 256];
        sampler.render(&mut buf, 2);
        let energy: f32 = buf.iter().map(|s| s * s).sum();
        assert!(
            energy > 0.01,
            "One-shot should still produce sound after note_off"
        );
    }

    #[test]
    fn sustain_mode_releases_on_note_off() {
        let mut sampler = Sampler::new(44100.0);
        let sample = make_test_sample(44100, 0.5);
        sampler.load_sample(sample, "test.wav".into());
        sampler.set_param(P_RELEASE, 0.001); // very short release

        sampler.note_on(60, 100);
        // Render a few frames to get through attack
        let mut buf = vec![0.0_f32; 512];
        sampler.render(&mut buf, 2);

        sampler.note_off(60);

        // Render enough for release to complete
        let mut buf = vec![0.0_f32; 4410]; // 50ms
        sampler.render(&mut buf, 2);

        let tail_energy: f32 = buf[4000..].iter().map(|s| s * s).sum();
        assert!(
            tail_energy < 0.01,
            "After release, tail should be quiet, energy={tail_energy}"
        );
    }

    #[test]
    fn loop_mode_wraps() {
        let mut sampler = Sampler::new(44100.0);
        // Short sample: 100 frames
        let sample = make_test_sample(100, 0.5);
        sampler.load_sample(sample, "short.wav".into());
        sampler.set_param(P_LOOP, 1.0);

        sampler.note_on(60, 100);

        // Render 500 frames — way past the 100-frame sample
        let mut buf = vec![0.0_f32; 1000]; // 500 stereo frames
        sampler.render(&mut buf, 2);

        // Voice should still be active (looping)
        let active = sampler.voices.iter().any(|v| v.active());
        assert!(active, "Loop mode should keep voice active past sample end");

        // Should still produce sound
        let energy: f32 = buf[500..].iter().map(|s| s * s).sum();
        assert!(
            energy > 0.001,
            "Looped sample should produce sound past end"
        );
    }

    #[test]
    fn start_end_trim() {
        let mut sampler = Sampler::new(44100.0);
        // Create a 1000-frame sample with ascending values
        let data: Vec<f32> = (0..1000).map(|i| i as f32 / 1000.0).collect();
        let sample = Arc::new(DecodedAudio {
            channels: vec![data.clone(), data],
            sample_rate: 44100,
        });
        sampler.load_sample(sample, "ramp.wav".into());

        // Set start=0.5, end=0.8 → frames 500..800
        sampler.set_param(P_START, 0.5);
        sampler.set_param(P_END, 0.8);

        sampler.note_on(60, 127);

        // The voice position should start at frame 500
        let voice = sampler.voices.iter().find(|v| v.active()).unwrap();
        assert!(
            (voice.position - 500.0).abs() < 1.0,
            "Start position should be ~500, got {}",
            voice.position
        );
    }

    #[test]
    fn polyphony() {
        let mut sampler = Sampler::new(44100.0);
        let sample = make_test_sample(44100, 0.5);
        sampler.load_sample(sample, "test.wav".into());

        sampler.note_on(60, 100);
        sampler.note_on(64, 100);
        sampler.note_on(67, 100);

        let active_count = sampler.voices.iter().filter(|v| v.active()).count();
        assert_eq!(active_count, 3, "Should have 3 active voices");

        let mut buf = vec![0.0_f32; 512];
        sampler.render(&mut buf, 2);
        let energy: f32 = buf.iter().map(|s| s * s).sum();
        assert!(energy > 0.03, "Polyphonic output should be louder");
    }

    #[test]
    fn param_get_set_roundtrip() {
        let mut sampler = Sampler::new(44100.0);

        for (idx, desc) in SAMPLER_PARAMS.iter().enumerate() {
            let val = (desc.min + desc.max) / 2.0;
            assert!(sampler.set_param(idx, val));
            let got = sampler.get_param(idx);
            // Boolean params snap to 0/1
            if idx == P_LOOP || idx == P_ONE_SHOT {
                assert!(got == 0.0 || got == 1.0);
            } else {
                assert!(
                    (got - val).abs() < 0.01,
                    "Param {idx} ({}) roundtrip: set {val}, got {got}",
                    desc.name
                );
            }
        }

        // Invalid param
        assert!(!sampler.set_param(99, 1.0));
        assert!((sampler.get_param(99)).abs() < 1e-6);
    }

    #[test]
    fn reset_clears_voices() {
        let mut sampler = Sampler::new(44100.0);
        let sample = make_test_sample(44100, 0.5);
        sampler.load_sample(sample, "test.wav".into());
        sampler.note_on(60, 100);
        sampler.reset();
        for voice in &sampler.voices {
            assert!(!voice.active());
        }
    }

    #[test]
    fn velocity_scaling() {
        let mut sampler_loud = Sampler::new(44100.0);
        let mut sampler_soft = Sampler::new(44100.0);
        let sample = make_test_sample(44100, 0.5);
        sampler_loud.load_sample(sample.clone(), "test.wav".into());
        sampler_soft.load_sample(sample, "test.wav".into());

        sampler_loud.note_on(60, 127);
        sampler_soft.note_on(60, 32);

        let mut buf_loud = vec![0.0_f32; 512];
        let mut buf_soft = vec![0.0_f32; 512];
        sampler_loud.render(&mut buf_loud, 2);
        sampler_soft.render(&mut buf_soft, 2);

        let energy_loud: f32 = buf_loud.iter().map(|s| s * s).sum();
        let energy_soft: f32 = buf_soft.iter().map(|s| s * s).sum();
        assert!(
            energy_loud > energy_soft,
            "Louder velocity should produce more energy: loud={energy_loud}, soft={energy_soft}"
        );
    }

    #[test]
    fn sample_name_tracking() {
        let mut sampler = Sampler::new(44100.0);
        assert!(sampler.sample_name().is_none());

        let sample = make_test_sample(100, 0.5);
        sampler.load_sample(sample, "kick.wav".into());
        assert_eq!(sampler.sample_name(), Some("kick.wav"));
    }

    #[test]
    fn instrument_kind_is_sampler() {
        let sampler = Sampler::new(44100.0);
        assert_eq!(sampler.instrument_kind(), InstrumentKind::Sampler);
    }
}

#[cfg(test)]
#[path = "sampler_quality_tests.rs"]
mod quality_tests;
