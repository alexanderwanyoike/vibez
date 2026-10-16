//! The drum rack: 64 pads from note 36 up, each a full Sampler playing its
//! sample at its root. Hits on a pad ring on that pad's own voices, so one
//! pad never cuts another; a pad in a choke group releases the group's
//! pads first, as a closed hat stops an open one.

use std::sync::Arc;

use vibez_core::audio_buffer::DecodedAudio;
use vibez_core::effect::ParamDescriptor;
use vibez_core::midi::InstrumentKind;
use vibez_core::track::{drum_rack_pad_index, DrumPadState, DRUM_RACK_PAD_COUNT};

use crate::sampler::{param, Sampler, PAD_NOTE, SAMPLER_PARAMS};
use crate::Instrument;

const _: () = assert!(DRUM_RACK_PAD_COUNT <= u64::BITS as usize);

struct Pad {
    sampler: Sampler,
    choke_group: Option<u8>,
}

pub struct DrumRack {
    sample_rate: f32,
    pads: Vec<Pad>,
    /// One bit for each pad that may be sounding, so a render skips the
    /// rest.
    live: u64,
}

/// A pad's Sampler controls. Pads saved before each pad was a Sampler
/// translate their gain, pan, trim, fades, tuning and mode.
pub fn pad_params(state: &DrumPadState) -> [f32; param::COUNT] {
    let mut params: [f32; param::COUNT] =
        std::array::from_fn(|index| SAMPLER_PARAMS[index].default);
    if !state.params.is_empty() {
        for (slot, value) in params.iter_mut().zip(&state.params) {
            *slot = *value;
        }
        return params;
    }
    // Their equal-power pan held a centred pad 3 dB down; the Sampler's
    // balance does not.
    params[param::VOLUME] = state.gain * std::f32::consts::FRAC_1_SQRT_2;
    params[param::PAN] = state.pan;
    params[param::START] = state.start;
    params[param::END] = state.end;
    params[param::ATTACK] = state.fade_in_ms / 1000.0;
    params[param::RELEASE] = state.fade_out_ms / 1000.0;
    params[param::TUNE] = f32::from(state.coarse_tune);
    params[param::FINE] = state.fine_tune;
    params[param::ONE_SHOT] = f32::from(state.one_shot);
    params
}

impl DrumRack {
    pub fn new(sample_rate: f32) -> Self {
        let mut rack = Self {
            sample_rate,
            pads: (0..DRUM_RACK_PAD_COUNT)
                .map(|_| Pad {
                    sampler: Sampler::new(sample_rate),
                    choke_group: None,
                })
                .collect(),
            live: 0,
        };
        for index in 0..DRUM_RACK_PAD_COUNT {
            rack.set_drum_pad_state(index, DrumPadState::default());
        }
        rack
    }
}

impl Instrument for DrumRack {
    fn activation_sample_rate(&self) -> Option<u32> {
        Some(self.sample_rate as u32)
    }

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
        if velocity == 0 {
            self.note_off(pitch);
            return;
        }
        let Some(index) = drum_rack_pad_index(pitch) else {
            return;
        };
        if let Some(group) = self.pads[index].choke_group {
            for pad in &mut self.pads {
                if pad.choke_group == Some(group) {
                    pad.sampler.choke();
                }
            }
        }
        self.pads[index].sampler.strike(velocity);
        self.live |= 1 << index;
    }

    fn note_off(&mut self, pitch: u8) {
        if let Some(index) = drum_rack_pad_index(pitch) {
            self.pads[index].sampler.note_off(PAD_NOTE);
        }
    }

    fn render(&mut self, buffer: &mut [f32], channels: usize) {
        let mut live = self.live;
        while live != 0 {
            let index = live.trailing_zeros() as usize;
            live &= live - 1;
            let sampler = &mut self.pads[index].sampler;
            sampler.render(buffer, channels);
            if !sampler.sounding() {
                self.live &= !(1 << index);
            }
        }
    }

    fn reset(&mut self) {
        for pad in &mut self.pads {
            pad.sampler.reset();
        }
        self.live = 0;
    }

    fn load_drum_pad_sample(&mut self, pad_index: usize, sample: Arc<DecodedAudio>, name: String) {
        if let Some(pad) = self.pads.get_mut(pad_index) {
            pad.sampler.load_sample(sample, name);
        }
    }

    fn clear_drum_pad(&mut self, pad_index: usize) {
        if let Some(pad) = self.pads.get_mut(pad_index) {
            pad.sampler.unload();
        }
    }

    fn set_drum_pad_state(&mut self, pad_index: usize, state: DrumPadState) {
        let Some(pad) = self.pads.get_mut(pad_index) else {
            return;
        };
        for (index, value) in pad_params(&state).into_iter().enumerate() {
            pad.sampler.set_param(index, value);
        }
        pad.choke_group = state.choke_group;
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

    fn rendered(rack: &mut DrumRack, frames: usize) -> Vec<f32> {
        let mut buffer = vec![0.0; frames * 2];
        rack.render(&mut buffer, 2);
        buffer.chunks(2).map(|frame| frame[0]).collect()
    }

    #[test]
    fn pad_plays_its_sample_at_the_file_rate() {
        let mut rack = DrumRack::new(48_000.0);
        let sample = Arc::new(DecodedAudio {
            channels: vec![vec![0.5; 4_800], vec![0.5; 4_800]],
            sample_rate: 24_000,
        });
        rack.load_drum_pad_sample(0, sample, "slow.wav".into());
        rack.note_on(36, 127);

        let sounding = rendered(&mut rack, 12_000)
            .iter()
            .filter(|sample| sample.abs() > 1e-6)
            .count();

        assert!((9_400..=9_700).contains(&sounding), "{sounding} frames");
    }

    #[test]
    fn hits_on_one_pad_never_cut_another_pad_off() {
        let mut rack = DrumRack::new(48_000.0);
        rack.load_drum_pad_sample(0, make_test_audio(96_000, 0.5), "pad.wav".into());
        rack.load_drum_pad_sample(1, make_test_audio(96_000, 0.0), "silent.wav".into());
        for pad_index in [0, 1] {
            rack.set_drum_pad_state(
                pad_index,
                DrumPadState {
                    one_shot: false,
                    ..DrumPadState::default()
                },
            );
        }
        rack.note_on(36, 127);
        for _ in 0..32 {
            rack.note_on(37, 127);
        }

        let after = rendered(&mut rack, 4_800);

        assert!(after[4_799].abs() > 0.1, "the held pad stopped");
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

    #[test]
    fn rack_renders_a_pad_from_a_later_bank() {
        let mut rack = DrumRack::new(44_100.0);
        rack.load_drum_pad_sample(20, make_test_audio(64, 0.5), "bank-two.wav".into());
        rack.note_on(56, 127);

        let mut buffer = vec![0.0; 128];
        rack.render(&mut buffer, 2);

        assert!(buffer.iter().any(|sample| sample.abs() > 0.0));
    }

    #[test]
    fn a_kit_saved_before_pads_were_samplers_keeps_its_level_and_fades() {
        let mut rack = DrumRack::new(48_000.0);
        rack.load_drum_pad_sample(0, make_test_audio(4_800, 0.5), "click.wav".into());
        rack.set_drum_pad_state(
            0,
            DrumPadState {
                fade_in_ms: 3.0,
                fade_out_ms: 3.0,
                ..DrumPadState::default()
            },
        );
        rack.note_on(36, 127);

        let out = rendered(&mut rack, 6_000);

        let centred = 0.5 * std::f32::consts::FRAC_1_SQRT_2;
        let last = out.iter().rposition(|sample| *sample > 0.0).unwrap();
        assert!(out[0] < 0.01 * centred, "no fade in");
        assert!((out[2_400] - centred).abs() < 1e-3, "{}", out[2_400]);
        assert!(out[last] < 0.01 * centred, "no fade out");
    }

    #[test]
    fn a_gated_pad_releases_over_its_fade_out() {
        let mut rack = DrumRack::new(48_000.0);
        rack.load_drum_pad_sample(0, make_test_audio(48_000, 1.0), "gate.wav".into());
        rack.set_drum_pad_state(
            0,
            DrumPadState {
                fade_out_ms: 10.0,
                one_shot: false,
                ..DrumPadState::default()
            },
        );
        rack.note_on(36, 127);
        let held = rendered(&mut rack, 480);
        rack.note_off(36);

        let release = rendered(&mut rack, 960);

        assert!(release[0] > 0.9 * held[479], "the release stepped");
        assert!(release.windows(2).all(|pair| pair[1] <= pair[0]));
        assert!(release[480] < 0.002, "{}", release[480]);
        assert_eq!(release[959], 0.0);
    }

    #[test]
    fn a_choke_group_releases_the_other_pad_over_its_fade_out() {
        let mut rack = DrumRack::new(48_000.0);
        rack.load_drum_pad_sample(0, make_test_audio(48_000, 1.0), "open.wav".into());
        rack.load_drum_pad_sample(1, make_test_audio(48_000, 0.0), "closed.wav".into());
        for pad_index in [0, 1] {
            rack.set_drum_pad_state(
                pad_index,
                DrumPadState {
                    fade_out_ms: 10.0,
                    choke_group: Some(1),
                    ..DrumPadState::default()
                },
            );
        }
        rack.note_on(36, 127);
        let open = rendered(&mut rack, 480);

        rack.note_on(37, 127);
        let choked = rendered(&mut rack, 960);

        assert!(choked[0] > 0.9 * open[479], "the choke stepped");
        assert!(choked[480] < 0.002, "{}", choked[480]);
        assert_eq!(choked[959], 0.0);
    }

    #[test]
    fn a_pad_plays_its_own_sampler_controls() {
        let mut rack = DrumRack::new(48_000.0);
        rack.load_drum_pad_sample(0, make_test_audio(4_800, 0.5), "kick.wav".into());
        let mut params = pad_params(&DrumPadState::default());
        params[param::VOLUME] = 0.5;
        params[param::PAN] = -1.0;
        rack.set_drum_pad_state(
            0,
            DrumPadState {
                params: params.to_vec(),
                ..DrumPadState::default()
            },
        );
        rack.note_on(36, 127);

        let mut buffer = vec![0.0; 2 * 2_400];
        rack.render(&mut buffer, 2);

        assert!((buffer[2 * 2_000] - 0.25).abs() < 1e-3);
        assert_eq!(buffer[2 * 2_000 + 1], 0.0);
    }
}
