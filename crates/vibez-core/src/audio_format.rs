//! Product-level audio import support shared by Local and Remote Browser paths.
//!
//! Keep this matrix aligned with the explicit `symphonia` features in the
//! workspace manifest. An extension makes a source eligible for decoding; the
//! decoder still validates the actual container and codec before any import or
//! Project Media mutation occurs.

use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioFormatSupport {
    pub label: &'static str,
    pub container: &'static str,
    pub codec: &'static str,
    pub extensions: &'static [&'static str],
}

pub const SUPPORTED_AUDIO_FORMATS: &[AudioFormatSupport] = &[
    AudioFormatSupport {
        label: "WAV",
        container: "RIFF/WAVE",
        codec: "PCM",
        extensions: &["wav", "wave"],
    },
    AudioFormatSupport {
        label: "AIFF",
        container: "AIFF",
        codec: "PCM",
        extensions: &["aif", "aiff"],
    },
    AudioFormatSupport {
        label: "FLAC",
        container: "FLAC",
        codec: "FLAC",
        extensions: &["flac"],
    },
    AudioFormatSupport {
        label: "MP3",
        container: "MPEG audio",
        codec: "MPEG Layer III",
        extensions: &["mp3"],
    },
    AudioFormatSupport {
        label: "OGG",
        container: "Ogg",
        codec: "Vorbis",
        extensions: &["ogg"],
    },
    AudioFormatSupport {
        label: "M4A",
        container: "ISO MP4/M4A",
        codec: "AAC",
        extensions: &["m4a"],
    },
];

/// Static extension list accepted by native file pickers.
pub const SUPPORTED_AUDIO_EXTENSIONS: &[&str] =
    &["wav", "wave", "aif", "aiff", "flac", "mp3", "ogg", "m4a"];

pub fn audio_format_for_extension(extension: &str) -> Option<&'static AudioFormatSupport> {
    SUPPORTED_AUDIO_FORMATS.iter().find(|format| {
        format
            .extensions
            .iter()
            .any(|candidate| candidate.eq_ignore_ascii_case(extension))
    })
}

pub fn audio_format_for_path(path: &Path) -> Option<&'static AudioFormatSupport> {
    path.extension()
        .and_then(|extension| extension.to_str())
        .and_then(audio_format_for_extension)
}

pub fn is_supported_audio_path(path: &Path) -> bool {
    audio_format_for_path(path).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matrix_and_picker_extensions_are_identical() {
        let matrix_extensions: Vec<_> = SUPPORTED_AUDIO_FORMATS
            .iter()
            .flat_map(|format| format.extensions.iter().copied())
            .collect();
        assert_eq!(matrix_extensions, SUPPORTED_AUDIO_EXTENSIONS);
    }

    #[test]
    fn lookup_is_case_insensitive_and_raw_aac_is_not_advertised() {
        assert_eq!(audio_format_for_extension("AIFF").unwrap().label, "AIFF");
        assert_eq!(audio_format_for_extension("m4A").unwrap().codec, "AAC");
        assert!(audio_format_for_extension("aac").is_none());
        assert!(audio_format_for_extension("mid").is_none());
    }
}
