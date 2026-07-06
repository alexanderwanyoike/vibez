use std::sync::Arc;

use vibez_core::audio_buffer::DecodedAudio;
use vibez_core::effect::ParamDescriptor;
use vibez_core::midi::InstrumentKind;

use crate::envelope::{Envelope, EnvelopeStage};
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

#[derive(Debug, Clone)]
struct SamplerVoice {
    active: bool,
    pitch: u8,
    velocity: f32,
    position: f64,
    speed: f64,
    envelope: Envelope,
    one_shot: bool,
}

impl SamplerVoice {
    fn new(sample_rate: f32) -> Self {
        Self {
            active: false,
            pitch: 0,
            velocity: 0.0,
            position: 0.0,
            speed: 1.0,
            envelope: Envelope::new(sample_rate),
            one_shot: false,
        }
    }
}

/// Polyphonic sample-playback instrument.
///
/// Load an audio file, map it to a root note, and play it pitched across the
/// keyboard via playback-speed shifting with ADSR envelope.
pub struct Sampler {
    voices: Vec<SamplerVoice>,
    #[allow(dead_code)]
    sample_rate: f32,
    sample: Option<Arc<DecodedAudio>>,
    sample_name: Option<String>,
    // Parameters
    root_note: f32,
    attack: f32,
    decay: f32,
    sustain: f32,
    release: f32,
    start: f32,
    end: f32,
    loop_enabled: bool,
    one_shot: bool,
    volume: f32,
}

impl Sampler {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            voices: (0..MAX_VOICES)
                .map(|_| SamplerVoice::new(sample_rate))
                .collect(),
            sample_rate,
            sample: None,
            sample_name: None,
            root_note: 60.0,
            attack: 0.001,
            decay: 0.1,
            sustain: 1.0,
            release: 0.01,
            start: 0.0,
            end: 1.0,
            loop_enabled: false,
            one_shot: false,
            volume: 0.8,
        }
    }

    /// Compute the start frame from the normalized start parameter.
    fn start_frame(&self) -> usize {
        if let Some(ref sample) = self.sample {
            (self.start * sample.num_frames() as f32) as usize
        } else {
            0
        }
    }

    /// Compute the end frame from the normalized end parameter.
    fn end_frame(&self) -> usize {
        if let Some(ref sample) = self.sample {
            (self.end * sample.num_frames() as f32) as usize
        } else {
            0
        }
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
    fn instrument_kind(&self) -> InstrumentKind {
        InstrumentKind::Sampler
    }

    fn param_descriptors(&self) -> &'static [ParamDescriptor] {
        SAMPLER_PARAMS
    }

    fn set_param(&mut self, index: usize, value: f32) -> bool {
        match index {
            P_ROOT_NOTE => {
                self.root_note = value.clamp(0.0, 127.0);
                true
            }
            P_ATTACK => {
                self.attack = value.clamp(0.001, 2.0);
                true
            }
            P_DECAY => {
                self.decay = value.clamp(0.001, 2.0);
                true
            }
            P_SUSTAIN => {
                self.sustain = value.clamp(0.0, 1.0);
                true
            }
            P_RELEASE => {
                self.release = value.clamp(0.001, 5.0);
                true
            }
            P_START => {
                self.start = value.clamp(0.0, 1.0);
                true
            }
            P_END => {
                self.end = value.clamp(0.0, 1.0);
                true
            }
            P_LOOP => {
                self.loop_enabled = value >= 0.5;
                true
            }
            P_ONE_SHOT => {
                self.one_shot = value >= 0.5;
                true
            }
            P_VOLUME => {
                self.volume = value.clamp(0.0, 1.0);
                true
            }
            _ => false,
        }
    }

    fn get_param(&self, index: usize) -> f32 {
        match index {
            P_ROOT_NOTE => self.root_note,
            P_ATTACK => self.attack,
            P_DECAY => self.decay,
            P_SUSTAIN => self.sustain,
            P_RELEASE => self.release,
            P_START => self.start,
            P_END => self.end,
            P_LOOP => {
                if self.loop_enabled {
                    1.0
                } else {
                    0.0
                }
            }
            P_ONE_SHOT => {
                if self.one_shot {
                    1.0
                } else {
                    0.0
                }
            }
            P_VOLUME => self.volume,
            _ => 0.0,
        }
    }

    fn note_on(&mut self, pitch: u8, velocity: u8) {
        let speed = 2.0_f64.powf((pitch as f64 - self.root_note as f64) / 12.0);
        let start_pos = self.start_frame() as f64;

        // Update envelope params on all voices
        for voice in &mut self.voices {
            voice.envelope.attack = self.attack;
            voice.envelope.decay = self.decay;
            voice.envelope.sustain = self.sustain;
            voice.envelope.release = self.release;
        }

        // Find free voice or steal oldest
        let voice_idx = self.voices.iter().position(|v| !v.active).unwrap_or(0);

        let voice = &mut self.voices[voice_idx];
        voice.active = true;
        voice.pitch = pitch;
        voice.velocity = velocity as f32 / 127.0;
        voice.position = start_pos;
        voice.speed = speed;
        voice.one_shot = self.one_shot;
        voice.envelope.trigger();
    }

    fn note_off(&mut self, pitch: u8) {
        for voice in &mut self.voices {
            if voice.active && voice.pitch == pitch && !voice.one_shot {
                voice.envelope.release();
            }
        }
    }

    fn render(&mut self, buffer: &mut [f32], channels: usize) {
        let audio = match self.sample {
            Some(ref s) => Arc::clone(s),
            None => return,
        };

        let ch = channels.clamp(1, 2);
        let frames = buffer.len() / ch;
        let start_f = self.start_frame();
        let end_f = self.end_frame();

        if end_f <= start_f {
            return;
        }

        let loop_enabled = self.loop_enabled;
        let volume = self.volume;

        for frame in 0..frames {
            let mut sample_l = 0.0_f32;
            let mut sample_r = 0.0_f32;

            for voice in &mut self.voices {
                if !voice.active {
                    continue;
                }

                let env = voice.envelope.tick();
                if !voice.envelope.is_active() {
                    voice.active = false;
                    continue;
                }

                // Read interpolated sample
                let sl = read_sample(&audio, voice.position, 0);
                let sr = if ch >= 2 {
                    read_sample(&audio, voice.position, 1)
                } else {
                    sl
                };

                let gain = env * voice.velocity * volume;
                sample_l += sl * gain;
                sample_r += sr * gain;

                // Advance position
                voice.position += voice.speed;

                // Check end boundary
                if voice.position >= end_f as f64 {
                    if loop_enabled {
                        // Wrap back to start
                        let loop_len = (end_f - start_f) as f64;
                        voice.position =
                            start_f as f64 + (voice.position - start_f as f64) % loop_len;
                    } else {
                        // Sample finished
                        voice.active = false;
                        voice.envelope.stage = EnvelopeStage::Idle;
                        voice.envelope.level = 0.0;
                    }
                }
            }

            buffer[frame * ch] += sample_l;
            if ch >= 2 {
                buffer[frame * ch + 1] += sample_r;
            }
        }
    }

    fn reset(&mut self) {
        for voice in &mut self.voices {
            voice.active = false;
            voice.envelope.stage = EnvelopeStage::Idle;
            voice.envelope.level = 0.0;
            voice.position = 0.0;
        }
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
                .any(|v| v.active && (v.speed - 2.0).abs() < 1e-6),
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
                .any(|v| v.active && (v.speed - 1.0).abs() < 1e-6),
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
        let active_count = sampler.voices.iter().filter(|v| v.active).count();
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
        let active = sampler.voices.iter().any(|v| v.active);
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
        let voice = sampler.voices.iter().find(|v| v.active).unwrap();
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

        let active_count = sampler.voices.iter().filter(|v| v.active).count();
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
            assert!(!voice.active);
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
