//! Shared input/output CPAL stream selection and construction errors.

use cpal::traits::DeviceTrait;
use cpal::SampleFormat;

#[derive(Debug, Clone, Copy)]
pub(crate) enum StreamDirection {
    Input,
    Output,
}

impl StreamDirection {
    fn label(self) -> &'static str {
        match self {
            Self::Input => "input",
            Self::Output => "output",
        }
    }
}

#[derive(Debug)]
pub enum StreamOpenError {
    DefaultConfig(cpal::DefaultStreamConfigError),
    SupportedConfigs(cpal::SupportedStreamConfigsError),
    Build(cpal::BuildStreamError),
    Play(cpal::PlayStreamError),
    Unsupported(String),
}

impl std::fmt::Display for StreamOpenError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DefaultConfig(error) => write!(formatter, "default stream config error: {error}"),
            Self::SupportedConfigs(error) => {
                write!(formatter, "supported stream config error: {error}")
            }
            Self::Build(error) => write!(formatter, "failed to build audio stream: {error}"),
            Self::Play(error) => write!(formatter, "failed to play audio stream: {error}"),
            Self::Unsupported(description) => formatter.write_str(description),
        }
    }
}

impl std::error::Error for StreamOpenError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::DefaultConfig(error) => Some(error),
            Self::SupportedConfigs(error) => Some(error),
            Self::Build(error) => Some(error),
            Self::Play(error) => Some(error),
            Self::Unsupported(_) => None,
        }
    }
}

impl From<cpal::DefaultStreamConfigError> for StreamOpenError {
    fn from(error: cpal::DefaultStreamConfigError) -> Self {
        Self::DefaultConfig(error)
    }
}

impl From<cpal::SupportedStreamConfigsError> for StreamOpenError {
    fn from(error: cpal::SupportedStreamConfigsError) -> Self {
        Self::SupportedConfigs(error)
    }
}

impl From<cpal::BuildStreamError> for StreamOpenError {
    fn from(error: cpal::BuildStreamError) -> Self {
        Self::Build(error)
    }
}

impl From<cpal::PlayStreamError> for StreamOpenError {
    fn from(error: cpal::PlayStreamError) -> Self {
        Self::Play(error)
    }
}

pub(crate) fn buffer_size_supported(size: Option<u32>, range: &cpal::SupportedBufferSize) -> bool {
    let Some(size) = size else {
        return true;
    };
    match range {
        cpal::SupportedBufferSize::Range { min, max } => (*min..=*max).contains(&size),
        cpal::SupportedBufferSize::Unknown => true,
    }
}

pub(crate) fn select_stream_config(
    device: &cpal::Device,
    direction: StreamDirection,
    requested_sample_rate: Option<u32>,
    buffer_size: Option<u32>,
    preferred_channels: Option<u16>,
) -> Result<cpal::SupportedStreamConfig, StreamOpenError> {
    let default_result = match direction {
        StreamDirection::Input => device.default_input_config(),
        StreamDirection::Output => device.default_output_config(),
    };
    let default = match default_result {
        Ok(default) => Some(default),
        Err(_) if requested_sample_rate.is_some() => None,
        Err(error) => return Err(error.into()),
    };
    let sample_rate = requested_sample_rate
        .or_else(|| default.as_ref().map(|config| config.sample_rate()))
        .expect("a requested or default sample rate is required");
    let preferred_channels =
        preferred_channels.or_else(|| default.as_ref().map(|config| config.channels()));
    let mut candidates: Vec<_> = match direction {
        StreamDirection::Input => device.supported_input_configs()?.collect(),
        StreamDirection::Output => device.supported_output_configs()?.collect(),
    };
    candidates.retain(|range| {
        (range.min_sample_rate()..=range.max_sample_rate()).contains(&sample_rate)
            && buffer_size_supported(buffer_size, range.buffer_size())
    });
    candidates.sort_by_key(|range| {
        (
            range.sample_format() != SampleFormat::F32,
            preferred_channels.is_some_and(|preferred| range.channels() != preferred),
            range.channels(),
        )
    });
    candidates
        .into_iter()
        .next()
        .and_then(|range| range.try_with_sample_rate(sample_rate))
        .ok_or_else(|| {
            StreamOpenError::Unsupported(format!(
                "audio {} does not support {sample_rate} Hz with {} buffer",
                direction.label(),
                buffer_size
                    .map(|size| format!("a {size}-frame"))
                    .unwrap_or_else(|| "the default".into())
            ))
        })
}
