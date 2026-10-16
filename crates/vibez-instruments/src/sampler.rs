//! The sampler: one loaded sample played across the keyboard by changing
//! its speed, forwards or backwards, looping with a crossfade, through the
//! synth's filter and envelopes, within a key and velocity zone. Eight
//! voices, each new note taking a free voice or else the oldest.

use std::sync::Arc;

use vibez_core::audio_buffer::DecodedAudio;
use vibez_core::effect::ParamDescriptor;
use vibez_core::midi::InstrumentKind;

use crate::envelope::Times;
pub use crate::sampler_params::{filter_type, is_whole, param, SAMPLER_PARAMS};
use crate::sampler_voice::{Frame, Region, SamplerVoice};
use crate::synth::lfo;
use crate::synth_filter::Coefficients;
use crate::synth_params::{lfo_target, LFO_CUTOFF_OCTAVES, LFO_PITCH_SEMITONES};
use crate::Instrument;

const MAX_VOICES: usize = 8;
/// A sample that stops before its loop or its own end fades out over this
/// long, so stopping mid-sound does not click.
const END_FADE_SECONDS: f32 = 0.002;
/// How fast continuous levels follow an edit, so a moved knob does not
/// click or zip.
const SMOOTHING_SECONDS: f32 = 0.005;
const SMOOTHED: [usize; 4] = [param::VOLUME, param::CUTOFF, param::LFO_AMOUNT, param::PAN];

/// The original names, which the older tests use.
#[cfg(test)]
const P_ROOT_NOTE: usize = param::ROOT_NOTE;
#[cfg(test)]
const P_RELEASE: usize = param::RELEASE;
#[cfg(test)]
const P_START: usize = param::START;
#[cfg(test)]
const P_END: usize = param::END;
#[cfg(test)]
const P_LOOP: usize = param::LOOP;
#[cfg(test)]
const P_ONE_SHOT: usize = param::ONE_SHOT;

/// Polyphonic sample-playback instrument.
pub struct Sampler {
    voices: [SamplerVoice; MAX_VOICES],
    sample_rate: f32,
    sample: Option<Arc<DecodedAudio>>,
    sample_name: Option<String>,
    params: [f32; param::COUNT],
    /// The smoothed controls' current values; the rest read `params`.
    current: [f32; param::COUNT],
    amp_times: Times,
    filter_times: Times,
    smoothing: f32,
    started: bool,
    age: u64,
    last_pitch: Option<f32>,
    lfo_phase: f64,
    filter_for: (f32, f32),
    coefficients: Coefficients,
}

impl Sampler {
    pub fn new(sample_rate: f32) -> Self {
        let sample_rate = if sample_rate.is_finite() && sample_rate > 0.0 {
            sample_rate
        } else {
            48_000.0
        };
        let params: [f32; param::COUNT] =
            std::array::from_fn(|index| SAMPLER_PARAMS[index].default);
        let mut sampler = Self {
            voices: [SamplerVoice::default(); MAX_VOICES],
            sample_rate,
            sample: None,
            sample_name: None,
            params,
            current: params,
            amp_times: Times::new(0.0, 0.0, 0.0, 0.0, sample_rate),
            filter_times: Times::new(0.0, 0.0, 0.0, 0.0, sample_rate),
            smoothing: 1.0 - (-1.0 / (SMOOTHING_SECONDS * sample_rate)).exp(),
            started: false,
            age: 0,
            last_pitch: None,
            lfo_phase: 0.0,
            filter_for: (f32::NAN, f32::NAN),
            coefficients: Coefficients::new(sample_rate, 1_000.0, 1.0),
        };
        sampler.retime();
        sampler
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
        self.filter_times = Times::new(
            p[param::FILTER_ATTACK],
            p[param::FILTER_DECAY],
            p[param::FILTER_SUSTAIN],
            p[param::FILTER_RELEASE],
            self.sample_rate,
        );
    }

    fn choice(&self, index: usize) -> usize {
        self.params[index].round().max(0.0) as usize
    }

    /// The part of the sample a note plays, from the trim, loop and
    /// direction controls.
    fn region(&self) -> Option<Region> {
        let sample = self.sample.as_ref()?;
        let frames = sample.num_frames() as f64;
        let at = |index: usize| f64::from(self.params[index]) * frames;
        let (start, end) = (at(param::START), at(param::END));
        if end - start < 1.0 {
            return None;
        }
        let looped = (self.params[param::LOOP] >= 0.5).then(|| {
            let loop_start = at(param::LOOP_START).clamp(start, end);
            let loop_end = at(param::LOOP_END).clamp(start, end);
            if loop_end - loop_start >= 2.0 {
                (loop_start, loop_end)
            } else {
                (start, end)
            }
        });
        let reverse = self.params[param::REVERSE] >= 0.5;
        // The blend reads a loop's length beyond the loop, which must lie
        // in the file.
        let crossfade = looped.map_or(0.0, |(loop_start, loop_end)| {
            let room = if reverse {
                frames - loop_end
            } else {
                loop_start
            };
            (f64::from(self.params[param::CROSSFADE]) / 1000.0 * f64::from(sample.sample_rate))
                .min(loop_end - loop_start)
                .min(room)
                .max(0.0)
        });
        Some(Region {
            start,
            end,
            looped,
            reverse,
            crossfade,
        })
    }

    fn in_zone(&self, pitch: u8, velocity: u8) -> bool {
        let (pitch, velocity) = (f32::from(pitch), f32::from(velocity));
        (self.params[param::KEY_LOW]..=self.params[param::KEY_HIGH]).contains(&pitch)
            && (self.params[param::VELOCITY_LOW]..=self.params[param::VELOCITY_HIGH])
                .contains(&velocity)
    }
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
        let value = match index {
            param::LOOP | param::ONE_SHOT | param::REVERSE => f32::from(value >= 0.5),
            // The root keeps any fraction an older project stored.
            param::ROOT_NOTE => value,
            _ if is_whole(index) => value.round(),
            _ => value,
        }
        .clamp(descriptor.min, descriptor.max);
        self.params[index] = value;
        if !self.started || !SMOOTHED.contains(&index) {
            self.current[index] = value;
        }
        self.retime();
        true
    }

    fn get_param(&self, index: usize) -> f32 {
        self.params.get(index).copied().unwrap_or(0.0)
    }

    fn note_on(&mut self, pitch: u8, velocity: u8) {
        if velocity == 0 {
            self.note_off(pitch);
            return;
        }
        if !self.in_zone(pitch, velocity) {
            return;
        }
        let Some(region) = self.region() else {
            return;
        };
        let p = &self.params;
        let semitones = f32::from(pitch) - p[param::ROOT_NOTE];
        let amount = p[param::VELOCITY_AMOUNT];
        let gain = 1.0 - amount + amount * f32::from(velocity) / 127.0;
        let glide = (p[param::GLIDE] / 1000.0 * self.sample_rate).round() as u32;
        let one_shot = p[param::ONE_SHOT] >= 0.5;
        let from = self.last_pitch;
        self.last_pitch = Some(semitones);
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
        self.voices[index].start(
            pitch, semitones, gain, self.age, from, glide, one_shot, region,
        );
    }

    fn note_off(&mut self, pitch: u8) {
        for voice in &mut self.voices {
            if voice.active() && voice.pitch == pitch && !voice.one_shot {
                voice.release();
            }
        }
    }

    fn render(&mut self, buffer: &mut [f32], channels: usize) {
        let Some(audio) = self.sample.as_ref().map(Arc::clone) else {
            return;
        };
        let Some(region) = self.region() else {
            return;
        };
        if channels == 0 {
            return;
        }
        let base_speed = f64::from(audio.sample_rate.max(1)) / f64::from(self.sample_rate);
        for out in buffer.chunks_exact_mut(channels) {
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
            self.lfo_phase = (self.lfo_phase
                + f64::from(p[param::LFO_RATE]) / f64::from(self.sample_rate))
            .fract();
            let target = self.choice(param::LFO_TARGET);
            let frame = Frame {
                audio: &audio,
                region,
                base_speed,
                offset_semitones: p[param::TUNE]
                    + p[param::FINE] / 100.0
                    + if target == lfo_target::PITCH {
                        modulation * LFO_PITCH_SEMITONES
                    } else {
                        0.0
                    },
                end_fade_frames: f64::from(END_FADE_SECONDS * self.sample_rate),
                filter_type: self.choice(param::FILTER_TYPE),
                cutoff: c[param::CUTOFF],
                resonance: p[param::RESONANCE],
                sample_rate: self.sample_rate,
                coefficients: self.coefficients,
                filter_env: p[param::FILTER_ENV],
                cutoff_octaves: if target == lfo_target::CUTOFF {
                    modulation * LFO_CUTOFF_OCTAVES
                } else {
                    0.0
                },
                amp_times: self.amp_times,
                filter_times: self.filter_times,
            };
            let (mut left, mut right) = (0.0f32, 0.0f32);
            for voice in &mut self.voices {
                if voice.active() {
                    let (l, r) = voice.generate(&frame);
                    left += l;
                    right += r;
                }
            }
            let tremolo = if target == lfo_target::VOLUME {
                1.0 - c[param::LFO_AMOUNT] * 0.5 + modulation * 0.5
            } else {
                1.0
            };
            let gain = c[param::VOLUME] * tremolo;
            // A balance, not equal power, so a centred sampler keeps its
            // level.
            let pan = c[param::PAN];
            out[0] += left * gain * (1.0 - pan).min(1.0);
            if let Some(second) = out.get_mut(1) {
                *second += right * gain * (1.0 + pan).min(1.0);
            }
        }
    }

    fn reset(&mut self) {
        self.voices = [SamplerVoice::default(); MAX_VOICES];
        self.current = self.params;
        self.last_pitch = None;
        self.lfo_phase = 0.0;
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

    /// Rising zero crossings a second, for a sampler at 44.1 kHz.
    fn frequency(sampler: &mut Sampler) -> f32 {
        let mut buf = vec![0.0_f32; 44_100 * 2];
        sampler.render(&mut buf, 2);
        buf.chunks(2)
            .map(|frame| frame[0])
            .collect::<Vec<_>>()
            .windows(2)
            .filter(|pair| pair[0] <= 0.0 && pair[1] > 0.0)
            .count() as f32
    }

    #[test]
    fn pitch_shift_octave_up() {
        let mut sampler = Sampler::new(44100.0);
        let sample = make_sine_sample(88_200, 440.0, 44100);
        sampler.load_sample(sample, "sine.wav".into());
        sampler.set_param(P_ROOT_NOTE, 60.0);
        sampler.note_on(72, 100);
        assert!((frequency(&mut sampler) - 880.0).abs() < 2.0);
    }

    #[test]
    fn original_pitch_at_root() {
        let mut sampler = Sampler::new(44100.0);
        let sample = make_sine_sample(88_200, 440.0, 44100);
        sampler.load_sample(sample, "sine.wav".into());
        sampler.set_param(P_ROOT_NOTE, 60.0);
        sampler.note_on(60, 100);
        assert!((frequency(&mut sampler) - 440.0).abs() < 2.0);
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

        // A hundred frames in, the play head reads frame 600.
        let mut buf = vec![0.0_f32; 202];
        sampler.render(&mut buf, 2);
        let expected = 0.6 * 0.8;
        assert!(
            (buf[200] - expected).abs() < 0.02,
            "{} vs {expected}",
            buf[200]
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
            // Switches snap to 0/1, and choices, notes and velocities to whole.
            if idx == P_LOOP || idx == P_ONE_SHOT || idx == param::REVERSE {
                assert!(got == 0.0 || got == 1.0);
            } else if is_whole(idx) && idx != P_ROOT_NOTE {
                assert_eq!(got, val.round());
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

#[cfg(test)]
#[path = "sampler_controls_tests.rs"]
mod controls_tests;
