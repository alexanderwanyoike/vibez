//! Portable builtin presets retain validated sound settings independently of device instances.

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::effect::EffectType;
use crate::midi::InstrumentKind;

const FORMAT_VERSION: u32 = 1;
const MAX_NAME_BYTES: usize = 128;
const MAX_KEY_BYTES: usize = 128;
const MAX_PARAMETERS: usize = 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DevicePresetKind {
    Effect(EffectType),
    Instrument(InstrumentKind),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "PresetDocument")]
pub struct DevicePreset {
    format_version: u32,
    kind: DevicePresetKind,
    name: String,
    parameters: BTreeMap<String, f32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DevicePresetError {
    UnsupportedVersion(u32),
    InvalidName,
    TooManyParameters,
    InvalidParameterKey(String),
    NonFiniteParameter(String),
}

impl fmt::Display for DevicePresetError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedVersion(version) => {
                write!(formatter, "Unsupported device preset version {version}")
            }
            Self::InvalidName => write!(
                formatter,
                "A preset name must contain visible text, fit within {MAX_NAME_BYTES} bytes, and contain no control characters"
            ),
            Self::TooManyParameters => {
                write!(formatter, "A preset supports at most {MAX_PARAMETERS} parameters")
            }
            Self::InvalidParameterKey(key) => {
                write!(formatter, "Invalid preset parameter key: {key:?}")
            }
            Self::NonFiniteParameter(key) => {
                write!(formatter, "Preset parameter {key:?} must be finite")
            }
        }
    }
}

impl std::error::Error for DevicePresetError {}

impl DevicePreset {
    pub fn new(
        kind: DevicePresetKind,
        name: impl Into<String>,
        parameters: BTreeMap<String, f32>,
    ) -> Result<Self, DevicePresetError> {
        let name = name.into();
        if name.trim().is_empty()
            || name.len() > MAX_NAME_BYTES
            || name.chars().any(char::is_control)
        {
            return Err(DevicePresetError::InvalidName);
        }
        if parameters.len() > MAX_PARAMETERS {
            return Err(DevicePresetError::TooManyParameters);
        }
        for (key, value) in &parameters {
            if key.is_empty()
                || key.len() > MAX_KEY_BYTES
                || !key
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-'))
            {
                return Err(DevicePresetError::InvalidParameterKey(key.clone()));
            }
            if !value.is_finite() {
                return Err(DevicePresetError::NonFiniteParameter(key.clone()));
            }
        }
        Ok(Self {
            format_version: FORMAT_VERSION,
            kind,
            name,
            parameters,
        })
    }

    pub fn kind(&self) -> DevicePresetKind {
        self.kind
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn parameters(&self) -> &BTreeMap<String, f32> {
        &self.parameters
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PresetDocument {
    format_version: u32,
    kind: DevicePresetKind,
    name: String,
    parameters: BTreeMap<String, f32>,
}

impl TryFrom<PresetDocument> for DevicePreset {
    type Error = DevicePresetError;

    fn try_from(document: PresetDocument) -> Result<Self, Self::Error> {
        if document.format_version != FORMAT_VERSION {
            return Err(DevicePresetError::UnsupportedVersion(
                document.format_version,
            ));
        }
        Self::new(document.kind, document.name, document.parameters)
    }
}
