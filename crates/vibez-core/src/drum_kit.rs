//! A Drum Rack kit: which pads hold which samples, with each pad's name,
//! Sampler values and choke group. A kit is a folder holding `kit.json` and
//! its samples, so it can be moved or shared whole; a kit may come from
//! anyone, so a sample is only ever a plain file name inside that folder.

use serde::{Deserialize, Serialize};

use crate::track::DRUM_RACK_PAD_COUNT;

pub const KIT_FILE: &str = "kit.json";
const FORMAT_VERSION: u32 = 1;
const MAX_NAME_BYTES: usize = 128;
const MAX_PARAMS: usize = 256;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DrumKit {
    pub format_version: u32,
    pub name: String,
    pub pads: Vec<KitPad>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KitPad {
    /// From 0, the pad playing note 36.
    pub pad: usize,
    pub name: String,
    /// A file in the kit's folder.
    pub sample: String,
    /// The pad's Sampler values by index; missing ones take the Sampler's
    /// defaults.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub params: Vec<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub choke_group: Option<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DrumKitError {
    UnsupportedVersion(u32),
    InvalidName(String),
    PadOutOfRange(usize),
    RepeatedPad(usize),
    InvalidSample(String),
    InvalidParams(usize),
}

impl std::fmt::Display for DrumKitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedVersion(version) => {
                write!(f, "This kit is from a newer Vibez (format {version})")
            }
            Self::InvalidName(name) => write!(f, "A kit or pad name is not valid: {name:?}"),
            Self::PadOutOfRange(pad) => write!(f, "The kit names pad {pad}, which the rack lacks"),
            Self::RepeatedPad(pad) => write!(f, "The kit names pad {pad} twice"),
            Self::InvalidSample(sample) => {
                write!(f, "The kit's sample {sample:?} is not a file in its folder")
            }
            Self::InvalidParams(pad) => write!(f, "Pad {pad}'s settings are not valid"),
        }
    }
}

impl std::error::Error for DrumKitError {}

fn valid_name(name: &str) -> bool {
    !name.trim().is_empty() && name.len() <= MAX_NAME_BYTES && !name.chars().any(char::is_control)
}

/// A plain file name: no folders, no way out of the kit's folder.
fn valid_sample(sample: &str) -> bool {
    valid_name(sample)
        && !sample.contains(['/', '\\', ':'])
        && sample != "."
        && sample != ".."
        && !sample.starts_with('.')
}

impl DrumKit {
    pub fn new(name: impl Into<String>, pads: Vec<KitPad>) -> Result<Self, DrumKitError> {
        let kit = Self {
            format_version: FORMAT_VERSION,
            name: name.into(),
            pads,
        };
        kit.validate()?;
        Ok(kit)
    }

    /// Refuses a kit read from a file that a valid kit could not be.
    pub fn validate(&self) -> Result<(), DrumKitError> {
        if self.format_version != FORMAT_VERSION {
            return Err(DrumKitError::UnsupportedVersion(self.format_version));
        }
        if !valid_name(&self.name) {
            return Err(DrumKitError::InvalidName(self.name.clone()));
        }
        let mut seen = [false; DRUM_RACK_PAD_COUNT];
        for pad in &self.pads {
            let slot = seen
                .get_mut(pad.pad)
                .ok_or(DrumKitError::PadOutOfRange(pad.pad))?;
            if std::mem::replace(slot, true) {
                return Err(DrumKitError::RepeatedPad(pad.pad));
            }
            if !valid_name(&pad.name) {
                return Err(DrumKitError::InvalidName(pad.name.clone()));
            }
            if !valid_sample(&pad.sample) {
                return Err(DrumKitError::InvalidSample(pad.sample.clone()));
            }
            if pad.params.len() > MAX_PARAMS || pad.params.iter().any(|value| !value.is_finite()) {
                return Err(DrumKitError::InvalidParams(pad.pad));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pad(pad: usize, sample: &str) -> KitPad {
        KitPad {
            pad,
            name: "Kick".into(),
            sample: sample.into(),
            params: vec![60.0, 0.003],
            choke_group: Some(1),
        }
    }

    fn read(text: &str) -> Result<DrumKit, DrumKitError> {
        let kit: DrumKit = serde_json::from_str(text).expect("a kit document");
        kit.validate().map(|()| kit)
    }

    #[test]
    fn a_kit_round_trips_through_its_file() {
        let kit = DrumKit::new("Dusty", vec![pad(0, "kick.wav"), pad(2, "snare.wav")]).unwrap();
        assert_eq!(read(&serde_json::to_string(&kit).unwrap()).unwrap(), kit);
    }

    #[test]
    fn a_sample_must_be_a_file_in_the_kits_folder() {
        for sample in [
            "../kick.wav",
            "/etc/passwd",
            "sub/kick.wav",
            "C:kick.wav",
            "..",
            ".hidden",
        ] {
            assert_eq!(
                DrumKit::new("Kit", vec![pad(0, sample)]),
                Err(DrumKitError::InvalidSample(sample.into())),
                "{sample}"
            );
        }
    }

    #[test]
    fn pads_must_exist_once_and_hold_finite_values() {
        assert_eq!(
            DrumKit::new("Kit", vec![pad(DRUM_RACK_PAD_COUNT, "a.wav")]),
            Err(DrumKitError::PadOutOfRange(DRUM_RACK_PAD_COUNT))
        );
        assert_eq!(
            DrumKit::new("Kit", vec![pad(3, "a.wav"), pad(3, "b.wav")]),
            Err(DrumKitError::RepeatedPad(3))
        );
        let mut bad = pad(1, "a.wav");
        bad.params[0] = f32::NAN;
        assert_eq!(
            DrumKit::new("Kit", vec![bad]),
            Err(DrumKitError::InvalidParams(1))
        );
    }

    #[test]
    fn a_kit_from_a_newer_vibez_is_refused() {
        let newer = r#"{"format_version": 2, "name": "Kit", "pads": []}"#;
        assert_eq!(read(newer), Err(DrumKitError::UnsupportedVersion(2)));
    }
}
