//! One voice of the sampler: a play head through the sample, forwards or
//! backwards, looping with a crossfade, into the synth's filter, with an
//! envelope each for level and cutoff.

use vibez_core::audio_buffer::DecodedAudio;

use crate::envelope::{Envelope, Times};
use crate::sampler_params::filter_type;
use crate::synth_filter::{Coefficients, Outputs, Svf};

/// A moving cutoff is worked out again every this many samples.
const CONTROL_FRAMES: u8 = 8;
/// The second stage of the 24 dB low-pass adds slope, not more resonance.
const BUTTERWORTH_Q: f32 = std::f32::consts::FRAC_1_SQRT_2;

/// The part of the sample a note plays, in source frames.
#[derive(Clone, Copy, Debug)]
pub struct Region {
    pub start: f64,
    pub end: f64,
    /// The loop inside `start..end`, when looping.
    pub looped: Option<(f64, f64)>,
    pub reverse: bool,
    /// Source frames the loop's ends blend over.
    pub crossfade: f64,
}

/// What every voice shares for one sample.
pub struct Frame<'a> {
    pub audio: &'a DecodedAudio,
    pub region: Region,
    /// Source frames a host frame at the root note.
    pub base_speed: f64,
    /// Semitones from the root the tuning and LFO add.
    pub offset_semitones: f32,
    pub end_fade_frames: f64,
    pub filter_type: usize,
    pub cutoff: f32,
    pub resonance: f32,
    pub sample_rate: f32,
    pub coefficients: Coefficients,
    pub filter_env: f32,
    pub cutoff_octaves: f32,
    pub amp_times: Times,
    pub filter_times: Times,
}

/// Linear interpolation of one channel at a fractional source frame.
fn read(audio: &DecodedAudio, position: f64, channel: usize) -> f32 {
    let channels = audio.num_channels();
    if channels == 0 || position < 0.0 {
        return 0.0;
    }
    let channel = channel.min(channels - 1);
    let index = position as usize;
    let fraction = (position - index as f64) as f32;
    let a = audio.sample(channel, index);
    let b = audio.sample(channel, index + 1);
    a + fraction * (b - a)
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SamplerVoice {
    pub pitch: u8,
    /// The note count when this voice started; smallest is oldest.
    pub age: u64,
    pub one_shot: bool,
    gain: f32,
    position: f64,
    /// The sounding pitch in semitones from the root, gliding to the note.
    current: f32,
    target: f32,
    glide_step: f32,
    glide_left: u32,
    amp: Envelope,
    filter_env: Envelope,
    /// `[stage][channel]`.
    filters: [[Svf; 2]; 2],
    moved: Option<(Coefficients, u8)>,
}

impl SamplerVoice {
    #[allow(clippy::too_many_arguments)]
    pub fn start(
        &mut self,
        pitch: u8,
        semitones: f32,
        gain: f32,
        age: u64,
        from: Option<f32>,
        glide_frames: u32,
        one_shot: bool,
        region: Region,
    ) {
        if !self.amp.active() {
            self.filters = [[Svf::default(); 2]; 2];
        }
        self.pitch = pitch;
        self.age = age;
        self.gain = gain;
        self.one_shot = one_shot;
        self.position = if region.reverse {
            region.end - 1.0
        } else {
            region.start
        };
        self.target = semitones;
        match from {
            Some(from) if glide_frames > 0 => {
                self.current = from;
                self.glide_left = glide_frames;
                self.glide_step = (semitones - from) / glide_frames as f32;
            }
            _ => {
                self.current = semitones;
                self.glide_left = 0;
            }
        }
        self.amp.trigger();
        self.filter_env.trigger();
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

    fn stop(&mut self) {
        self.amp = Envelope::default();
        self.filter_env = Envelope::default();
    }

    /// The sample at the play head, blended near a loop's end with what
    /// lies a loop's length the other way, so the wrap is seamless.
    fn source(&self, frame: &Frame, channel: usize) -> f32 {
        let here = read(frame.audio, self.position, channel);
        let Some((loop_start, loop_end)) = frame.region.looped else {
            return here;
        };
        let length = loop_end - loop_start;
        let fade = frame.region.crossfade;
        if fade <= 0.0 {
            return here;
        }
        let (t, other) = if frame.region.reverse {
            (
                (loop_start + fade - self.position) / fade,
                self.position + length,
            )
        } else {
            (
                (self.position - (loop_end - fade)) / fade,
                self.position - length,
            )
        };
        if t <= 0.0 {
            return here;
        }
        let t = t.min(1.0) as f32;
        here * (1.0 - t) + read(frame.audio, other, channel) * t
    }

    fn filter(&mut self, channel: usize, input: f32, c: Coefficients, kind: usize) -> f32 {
        let first: Outputs = self.filters[0][channel].process(input, c);
        match kind {
            filter_type::LP24 => {
                self.filters[1][channel]
                    .process(first.low, c.with_q(BUTTERWORTH_Q))
                    .low
            }
            filter_type::HP12 => first.high,
            filter_type::BP12 => first.band,
            _ => first.low,
        }
    }

    pub fn generate(&mut self, frame: &Frame) -> (f32, f32) {
        let level = self.amp.tick(&frame.amp_times);
        let opening = self.filter_env.tick(&frame.filter_times);
        if self.glide_left > 0 {
            self.current += self.glide_step;
            self.glide_left -= 1;
            if self.glide_left == 0 {
                self.current = self.target;
            }
        }
        let speed =
            frame.base_speed * f64::from((self.current + frame.offset_semitones) / 12.0).exp2();
        let region = frame.region;
        let mut gain = level * self.gain;
        if region.looped.is_none() {
            let left = if region.reverse {
                self.position - region.start
            } else {
                region.end - self.position
            } / speed;
            gain *= (left / frame.end_fade_frames).clamp(0.0, 1.0) as f32;
        }
        let mut out = [self.source(frame, 0) * gain, self.source(frame, 1) * gain];
        if frame.filter_type != filter_type::OFF {
            let octaves = opening * frame.filter_env + frame.cutoff_octaves;
            let coefficients = if octaves == 0.0 {
                self.moved = None;
                frame.coefficients
            } else {
                let (coefficients, left) = match self.moved {
                    Some((coefficients, left)) if left > 0 => (coefficients, left - 1),
                    _ => (
                        Coefficients::new(
                            frame.sample_rate,
                            frame.cutoff * octaves.exp2(),
                            frame.resonance,
                        ),
                        CONTROL_FRAMES - 1,
                    ),
                };
                self.moved = Some((coefficients, left));
                coefficients
            };
            for (channel, sample) in out.iter_mut().enumerate() {
                *sample = self.filter(channel, *sample, coefficients, frame.filter_type);
            }
        }

        if region.reverse {
            self.position -= speed;
        } else {
            self.position += speed;
        }
        match region.looped {
            Some((loop_start, loop_end)) if region.reverse && self.position < loop_start => {
                self.position += loop_end - loop_start;
            }
            Some((loop_start, loop_end)) if !region.reverse && self.position >= loop_end => {
                self.position -= loop_end - loop_start;
            }
            None if self.position < region.start || self.position >= region.end => self.stop(),
            _ => {}
        }
        (out[0], out[1])
    }
}
