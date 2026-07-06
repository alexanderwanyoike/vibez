use std::sync::Arc;

use vibez_core::audio_buffer::DecodedAudio;
use vibez_core::effect::ParamDescriptor;
use vibez_core::midi::InstrumentKind;
use vibez_core::track::DrumPadState;

use crate::Instrument;

const PAD_COUNT: usize = 16;
const BASE_PAD_NOTE: u8 = 36;
const MAX_VOICES: usize = 32;

#[derive(Debug, Clone)]
struct Pad {
    sample: Option<Arc<DecodedAudio>>,
    sample_name: Option<String>,
    gain: f32,
    pan: f32,
    start: f32,
    end: f32,
    coarse_tune: i8,
    fine_tune: f32,
    one_shot: bool,
    choke_group: Option<u8>,
}

impl Default for Pad {
    fn default() -> Self {
        Self {
            sample: None,
            sample_name: None,
            gain: 1.0,
            pan: 0.0,
            start: 0.0,
            end: 1.0,
            coarse_tune: 0,
            fine_tune: 0.0,
            one_shot: true,
            choke_group: None,
        }
    }
}

#[derive(Clone)]
struct Voice {
    active: bool,
    pad_index: usize,
    pitch: u8,
    sample: Arc<DecodedAudio>,
    position: f64,
    end_frame: usize,
    speed: f64,
    gain: f32,
    pan: f32,
    one_shot: bool,
    choke_group: Option<u8>,
    velocity_gain: f32,
}

impl Voice {
    fn inactive() -> Self {
        Self {
            active: false,
            pad_index: 0,
            pitch: 0,
            sample: Arc::new(DecodedAudio {
                channels: Vec::new(),
                sample_rate: 44_100,
            }),
            position: 0.0,
            end_frame: 0,
            speed: 1.0,
            gain: 0.0,
            pan: 0.0,
            one_shot: true,
            choke_group: None,
            velocity_gain: 0.0,
        }
    }
}

pub struct DrumRack {
    #[allow(dead_code)]
    sample_rate: f32,
    pads: Vec<Pad>,
    voices: Vec<Voice>,
}

impl DrumRack {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            pads: (0..PAD_COUNT).map(|_| Pad::default()).collect(),
            voices: (0..MAX_VOICES).map(|_| Voice::inactive()).collect(),
        }
    }

    fn pitch_to_pad(pitch: u8) -> Option<usize> {
        let offset = pitch.checked_sub(BASE_PAD_NOTE)? as usize;
        (offset < PAD_COUNT).then_some(offset)
    }

    fn frame_range(pad: &Pad, sample: &DecodedAudio) -> Option<(usize, usize)> {
        let frames = sample.num_frames();
        if frames == 0 {
            return None;
        }
        let start = ((pad.start.clamp(0.0, 1.0)) * frames as f32) as usize;
        let mut end = ((pad.end.clamp(0.0, 1.0)) * frames as f32) as usize;
        if end <= start {
            end = (start + 1).min(frames);
        }
        if start >= end || start >= frames {
            return None;
        }
        Some((start, end.min(frames)))
    }

    fn take_voice_slot(&mut self) -> &mut Voice {
        if let Some(index) = self.voices.iter().position(|voice| !voice.active) {
            return &mut self.voices[index];
        }
        &mut self.voices[0]
    }
}

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

fn equal_power_pan(pan: f32) -> (f32, f32) {
    let pan = ((pan.clamp(-1.0, 1.0)) + 1.0) * 0.5;
    let angle = pan * std::f32::consts::FRAC_PI_2;
    (angle.cos(), angle.sin())
}

impl Instrument for DrumRack {
    fn instrument_kind(&self) -> InstrumentKind {
        InstrumentKind::DrumRack
    }

    fn param_descriptors(&self) -> &'static [ParamDescriptor] {
        &[]
    }

    fn set_param(&mut self, _index: usize, _value: f32) -> bool {
        false
    }

    fn get_param(&self, _index: usize) -> f32 {
        0.0
    }

    fn note_on(&mut self, pitch: u8, velocity: u8) {
        let Some(pad_index) = Self::pitch_to_pad(pitch) else {
            return;
        };
        let Some(sample) = self.pads[pad_index].sample.clone() else {
            return;
        };
        let Some((start_frame, end_frame)) = Self::frame_range(&self.pads[pad_index], &sample)
        else {
            return;
        };
        let gain = self.pads[pad_index].gain.max(0.0);
        let pan = self.pads[pad_index].pan;
        let coarse_tune = self.pads[pad_index].coarse_tune;
        let fine_tune = self.pads[pad_index].fine_tune;
        let one_shot = self.pads[pad_index].one_shot;
        let choke_group = self.pads[pad_index].choke_group;

        if let Some(group) = choke_group {
            for voice in &mut self.voices {
                if voice.active && voice.choke_group == Some(group) {
                    voice.active = false;
                }
            }
        }

        let semitones = coarse_tune as f32 + fine_tune / 100.0;
        let speed = 2.0_f64.powf(semitones as f64 / 12.0);
        let velocity_gain = (velocity as f32 / 127.0).clamp(0.0, 1.0);

        let voice = self.take_voice_slot();
        *voice = Voice {
            active: true,
            pad_index,
            pitch,
            sample,
            position: start_frame as f64,
            end_frame,
            speed,
            gain,
            pan,
            one_shot,
            choke_group,
            velocity_gain,
        };
    }

    fn note_off(&mut self, pitch: u8) {
        let Some(pad_index) = Self::pitch_to_pad(pitch) else {
            return;
        };
        for voice in &mut self.voices {
            if voice.active
                && voice.pad_index == pad_index
                && voice.pitch == pitch
                && !voice.one_shot
            {
                voice.active = false;
            }
        }
    }

    fn render(&mut self, buffer: &mut [f32], channels: usize) {
        if channels == 0 {
            return;
        }
        let frames = buffer.len() / channels;
        for frame in 0..frames {
            let frame_offset = frame * channels;
            for voice in &mut self.voices {
                if !voice.active {
                    continue;
                }

                if voice.position >= voice.end_frame as f64 {
                    voice.active = false;
                    continue;
                }

                let mono = read_sample(&voice.sample, voice.position, 0)
                    * voice.gain
                    * voice.velocity_gain;
                let (pan_l, pan_r) = equal_power_pan(voice.pan);

                if channels >= 2 {
                    let right = read_sample(&voice.sample, voice.position, 1)
                        * voice.gain
                        * voice.velocity_gain;
                    buffer[frame_offset] += mono * pan_l;
                    buffer[frame_offset + 1] += right * pan_r;
                    for ch in 2..channels {
                        buffer[frame_offset + ch] += mono;
                    }
                } else {
                    buffer[frame_offset] += mono;
                }

                voice.position += voice.speed;
                if voice.position >= voice.end_frame as f64 {
                    voice.active = false;
                }
            }
        }
    }

    fn reset(&mut self) {
        for voice in &mut self.voices {
            voice.active = false;
        }
    }

    fn load_drum_pad_sample(&mut self, pad_index: usize, sample: Arc<DecodedAudio>, name: String) {
        if let Some(pad) = self.pads.get_mut(pad_index) {
            pad.sample = Some(sample);
            pad.sample_name = Some(name);
        }
    }

    fn clear_drum_pad(&mut self, pad_index: usize) {
        if let Some(pad) = self.pads.get_mut(pad_index) {
            pad.sample = None;
            pad.sample_name = None;
        }
        for voice in &mut self.voices {
            if voice.active && voice.pad_index == pad_index {
                voice.active = false;
            }
        }
    }

    fn set_drum_pad_state(&mut self, pad_index: usize, state: DrumPadState) {
        if let Some(pad) = self.pads.get_mut(pad_index) {
            pad.gain = state.gain;
            pad.pan = state.pan;
            pad.start = state.start;
            pad.end = state.end;
            pad.coarse_tune = state.coarse_tune;
            pad.fine_tune = state.fine_tune;
            pad.one_shot = state.one_shot;
            pad.choke_group = state.choke_group;
            if pad.sample_name.is_none() {
                pad.sample_name = state.source.as_ref().map(|source| source.display_name());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_audio(frames: usize, value: f32) -> Arc<DecodedAudio> {
        Arc::new(DecodedAudio {
            channels: vec![vec![value; frames], vec![value; frames]],
            sample_rate: 44_100,
        })
    }

    #[test]
    fn rack_renders_loaded_pad() {
        let mut rack = DrumRack::new(44_100.0);
        rack.load_drum_pad_sample(0, make_test_audio(64, 0.5), "kick.wav".into());
        rack.note_on(36, 127);

        let mut buffer = vec![0.0; 128];
        rack.render(&mut buffer, 2);

        assert!(buffer.iter().any(|sample| sample.abs() > 0.0));
    }

    #[test]
    fn rack_ignores_unmapped_note() {
        let mut rack = DrumRack::new(44_100.0);
        rack.load_drum_pad_sample(0, make_test_audio(64, 0.5), "kick.wav".into());
        rack.note_on(12, 127);

        let mut buffer = vec![0.0; 128];
        rack.render(&mut buffer, 2);

        assert!(buffer.iter().all(|sample| sample.abs() < 1e-6));
    }
}
