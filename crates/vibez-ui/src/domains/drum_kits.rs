//! Drum Rack kits: the factory kits built into Vibez, and turning a kit into
//! the rack's pads or the rack's pads into a kit. Loading and saving files
//! happens in the app; this is what those files mean.

use std::sync::Arc;

use vibez_core::audio_buffer::DecodedAudio;
use vibez_core::drum_kit::{DrumKit, KitPad};
use vibez_core::track::{DrumPadState, MediaSourceRef, DRUM_RACK_PAD_COUNT};
use vibez_instruments::sampler::param;

use crate::state::UiDrumPad;

/// A kit shipped inside Vibez, its samples built in.
pub struct FactoryKit {
    pub name: &'static str,
    /// The folder its samples are credited under, in `assets/kits`.
    pub folder: &'static str,
    pub samples: &'static [(&'static str, &'static [u8])],
    pads: fn() -> Vec<KitPad>,
}

impl FactoryKit {
    pub fn kit(&self) -> DrumKit {
        DrumKit::new(self.name, (self.pads)()).expect("factory kits are valid")
    }

    pub fn sample(&self, name: &str) -> Option<&'static [u8]> {
        self.samples
            .iter()
            .find(|(file, _)| *file == name)
            .map(|(_, bytes)| *bytes)
    }
}

macro_rules! samples {
    ($folder:literal: $($file:literal),* $(,)?) => {
        &[$(($file, include_bytes!(concat!("../../../../assets/kits/", $folder, "/", $file)))),*]
    };
}

/// A pad playing `sample` as the rack's pads do by default, with `edits`.
fn pad(index: usize, name: &str, sample: &str, edits: &[(usize, f32)]) -> KitPad {
    let mut params = vibez_instruments::drum_rack::pad_params(&DrumPadState::default());
    for &(at, value) in edits {
        params[at] = value;
    }
    KitPad {
        pad: index,
        name: name.into(),
        sample: sample.into(),
        params: params.to_vec(),
        choke_group: None,
    }
}

fn choked(mut pad: KitPad) -> KitPad {
    pad.choke_group = Some(1);
    pad
}

fn tuned(semitones: f32) -> [(usize, f32); 1] {
    [(param::TUNE, semitones)]
}

/// The General MIDI drum order from note 36, which the rack's first bank
/// follows so General MIDI drum parts play the right sounds.
fn pads_808() -> Vec<KitPad> {
    vec![
        pad(0, "Kick", "kick.wav", &[]),
        pad(1, "Rim", "rim.wav", &[]),
        pad(2, "Snare", "snare.wav", &[]),
        pad(3, "Clap", "clap.wav", &[]),
        pad(4, "Snare Bright", "snare-bright.wav", &[]),
        pad(5, "Tom Low", "tom-low.wav", &[]),
        choked(pad(6, "Hat Closed", "hat-closed.wav", &[])),
        pad(7, "Conga Low", "conga-low.wav", &[]),
        pad(8, "Maracas", "maracas.wav", &[]),
        pad(9, "Tom Mid", "tom-mid.wav", &[]),
        choked(pad(10, "Hat Open", "hat-open.wav", &[])),
        pad(11, "Conga Mid", "conga-mid.wav", &[]),
        pad(12, "Tom High", "tom-high.wav", &[]),
        pad(13, "Cymbal", "cymbal.wav", &[]),
        pad(14, "Conga High", "conga-high.wav", &[]),
        pad(15, "Cowbell", "cowbell.wav", &[]),
        pad(16, "Claves", "claves.wav", &[]),
        pad(17, "Kick Short", "kick-short.wav", &[]),
        pad(18, "Kick Long", "kick-long.wav", &[]),
        pad(19, "Kick Punch", "kick-punch.wav", &[]),
        pad(20, "Snare Snappy", "snare-snappy.wav", &[]),
        pad(21, "Snare Low", "snare-low.wav", &[]),
        choked(pad(22, "Hat Open Long", "hat-open-long.wav", &[])),
        pad(23, "Cymbal Long", "cymbal-long.wav", &[]),
    ]
}

fn pads_909() -> Vec<KitPad> {
    vec![
        pad(0, "Kick", "kick.wav", &[]),
        pad(1, "Rim", "rim.wav", &[]),
        pad(2, "Snare", "snare.wav", &[]),
        pad(3, "Clap", "clap.wav", &[]),
        pad(4, "Snare Tight", "snare.wav", &tuned(2.0)),
        pad(5, "Tom Low", "tom-low.wav", &[]),
        choked(pad(6, "Hat Closed", "hat-closed.wav", &[])),
        pad(7, "Tom Low High", "tom-low.wav", &tuned(3.0)),
        choked(pad(
            8,
            "Hat Pedal",
            "hat-closed.wav",
            &[(param::END, 0.55), (param::RELEASE, 0.02)],
        )),
        pad(9, "Tom Mid", "tom-low.wav", &tuned(5.0)),
        choked(pad(10, "Hat Open", "hat-open.wav", &[])),
        pad(11, "Tom Mid High", "tom-low.wav", &tuned(7.0)),
        pad(12, "Tom High", "tom-high.wav", &[]),
        pad(13, "Crash", "crash.wav", &[]),
        pad(14, "Tom High High", "tom-high.wav", &tuned(3.0)),
        pad(15, "Ride", "ride.wav", &[]),
    ]
}

pub static FACTORY_KITS: [FactoryKit; 2] = [
    FactoryKit {
        name: "808",
        folder: "808",
        samples: samples!("808":
            "kick.wav", "rim.wav", "snare.wav", "clap.wav", "snare-bright.wav", "tom-low.wav",
            "hat-closed.wav", "conga-low.wav", "maracas.wav", "tom-mid.wav", "hat-open.wav",
            "conga-mid.wav", "tom-high.wav", "cymbal.wav", "conga-high.wav", "cowbell.wav",
            "claves.wav", "kick-short.wav", "kick-long.wav", "kick-punch.wav", "snare-snappy.wav",
            "snare-low.wav", "hat-open-long.wav", "cymbal-long.wav",
        ),
        pads: pads_808,
    },
    FactoryKit {
        name: "909",
        folder: "909",
        samples: samples!("909":
            "kick.wav", "rim.wav", "snare.wav", "clap.wav", "tom-low.wav", "hat-closed.wav",
            "hat-open.wav", "tom-high.wav", "crash.wav", "ride.wav",
        ),
        pads: pads_909,
    },
];

/// One of a kit's samples, decoded and owned by the project.
#[derive(Debug, Clone)]
pub struct KitSample {
    pub file: String,
    pub audio: Arc<DecodedAudio>,
    pub source: MediaSourceRef,
}

/// The rack's pads once `kit` loads: its pads where it names them, the rest
/// empty. A pad whose sample is missing keeps its settings but no sound.
pub fn rack_pads(kit: &DrumKit, samples: &[KitSample]) -> Vec<UiDrumPad> {
    let mut pads = crate::state::default_drum_rack_pads();
    for kit_pad in &kit.pads {
        let Some(slot) = pads.get_mut(kit_pad.pad) else {
            continue;
        };
        let sample = samples.iter().find(|sample| sample.file == kit_pad.sample);
        *slot = UiDrumPad {
            name: Some(kit_pad.name.clone()),
            source: sample.map(|sample| sample.source.clone()),
            audio: sample.map(|sample| Arc::clone(&sample.audio)),
            choke_group: kit_pad.choke_group,
            params: Vec::new(),
        };
        slot.params = vibez_instruments::drum_rack::pad_params(&DrumPadState {
            params: kit_pad.params.clone(),
            ..DrumPadState::default()
        })
        .to_vec();
    }
    pads
}

/// A kit sample's file name and the audio it is written from.
pub type SampleToWrite = (String, Arc<DecodedAudio>);

/// A plain file name for a pad's sample, unique within the kit.
fn sample_file(index: usize, name: &str) -> String {
    let stem: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    format!(
        "{:02}-{}.wav",
        index + 1,
        if stem.is_empty() { "pad" } else { &stem }
    )
}

/// The kit the rack's sounding pads make, named `name`, with the audio each
/// of its samples is written from.
pub fn kit_from_rack(
    name: &str,
    pads: &[UiDrumPad],
) -> Result<(DrumKit, Vec<SampleToWrite>), vibez_core::drum_kit::DrumKitError> {
    let mut kit_pads = Vec::new();
    let mut samples = Vec::new();
    for (index, pad) in pads.iter().enumerate().take(DRUM_RACK_PAD_COUNT) {
        let Some(audio) = &pad.audio else {
            continue;
        };
        let pad_name = pad
            .name
            .clone()
            .unwrap_or_else(|| format!("Pad {}", index + 1));
        let file = sample_file(index, &pad_name);
        samples.push((file.clone(), Arc::clone(audio)));
        kit_pads.push(KitPad {
            pad: index,
            name: pad_name,
            sample: file,
            params: pad.params.clone(),
            choke_group: pad.choke_group,
        });
    }
    Ok((DrumKit::new(name, kit_pads)?, samples))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(frames: usize) -> Arc<DecodedAudio> {
        Arc::new(DecodedAudio {
            channels: vec![vec![0.25; frames]],
            sample_rate: 44_100,
        })
    }

    #[test]
    fn every_factory_kit_names_only_samples_it_ships() {
        for factory in &FACTORY_KITS {
            let kit = factory.kit();
            assert!(!kit.pads.is_empty(), "{}", factory.name);
            for pad in &kit.pads {
                assert!(
                    factory
                        .sample(&pad.sample)
                        .is_some_and(|bytes| bytes.len() > 44),
                    "{}: {}",
                    factory.name,
                    pad.sample
                );
            }
        }
    }

    #[test]
    fn factory_hats_choke_each_other_and_kicks_sit_on_note_36() {
        for factory in &FACTORY_KITS {
            let kit = factory.kit();
            let by_name = |name: &str| kit.pads.iter().find(|pad| pad.name == name).unwrap();
            assert_eq!(by_name("Kick").pad, 0);
            assert_eq!(by_name("Hat Closed").choke_group, Some(1));
            assert_eq!(by_name("Hat Open").choke_group, Some(1));
        }
    }

    #[test]
    fn a_kit_fills_its_pads_and_empties_the_rest() {
        let kit = FACTORY_KITS[1].kit();
        let samples = vec![KitSample {
            file: "tom-low.wav".into(),
            audio: tone(100),
            source: MediaSourceRef::LocalFile {
                path: "/kits/909/tom-low.wav".into(),
            },
        }];
        let pads = rack_pads(&kit, &samples);
        assert_eq!(pads.len(), DRUM_RACK_PAD_COUNT);
        assert_eq!(pads[9].name.as_deref(), Some("Tom Mid"));
        assert_eq!(pads[9].param(param::TUNE), 5.0);
        assert!(pads[9].audio.is_some());
        assert_eq!(pads[0].name.as_deref(), Some("Kick"));
        assert!(pads[0].audio.is_none(), "its sample was not given");
        assert!(pads[40].name.is_none() && pads[40].audio.is_none());
    }

    #[test]
    fn the_rack_saves_as_a_kit_of_its_sounding_pads() {
        let mut pads = crate::state::default_drum_rack_pads();
        pads[0].name = Some("Big Kick 808!".into());
        pads[0].audio = Some(tone(10));
        pads[0].choke_group = Some(2);
        pads[0].sampler_params_mut()[param::PAN] = -0.5;
        pads[5].audio = Some(tone(10));

        let (kit, samples) = kit_from_rack("My Kit", &pads).unwrap();

        assert_eq!(kit.name, "My Kit");
        assert_eq!(kit.pads.len(), 2);
        assert_eq!(kit.pads[0].sample, "01-big-kick-808.wav");
        assert_eq!(kit.pads[0].params[param::PAN], -0.5);
        assert_eq!(kit.pads[0].choke_group, Some(2));
        assert_eq!(kit.pads[1].name, "Pad 6");
        assert_eq!(samples.len(), 2);
        assert_eq!(rack_pads(&kit, &[]).len(), DRUM_RACK_PAD_COUNT);
    }
}
