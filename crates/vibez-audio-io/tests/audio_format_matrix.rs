use std::path::{Path, PathBuf};

use vibez_audio_io::file_io::{decode_audio_file, FileIoError};
use vibez_core::audio_format::{audio_format_for_path, SUPPORTED_AUDIO_FORMATS};

#[derive(Debug, Clone, Copy)]
struct Fixture {
    file_name: &'static str,
    channels: usize,
    sample_rate: u32,
}

const FIXTURES: &[Fixture] = &[
    Fixture {
        file_name: "mono-44100-s16.wav",
        channels: 1,
        sample_rate: 44_100,
    },
    Fixture {
        file_name: "stereo-48000-s24.aiff",
        channels: 2,
        sample_rate: 48_000,
    },
    Fixture {
        file_name: "mono-32000-s24.flac",
        channels: 1,
        sample_rate: 32_000,
    },
    Fixture {
        file_name: "stereo-44100.mp3",
        channels: 2,
        sample_rate: 44_100,
    },
    Fixture {
        file_name: "mono-48000.ogg",
        channels: 1,
        sample_rate: 48_000,
    },
    Fixture {
        file_name: "stereo-44100.m4a",
        channels: 2,
        sample_rate: 44_100,
    },
];

fn fixture_path(file_name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(file_name)
}

#[test]
fn every_advertised_format_has_a_decodable_fixture_with_truthful_metadata() {
    assert_eq!(FIXTURES.len(), SUPPORTED_AUDIO_FORMATS.len());

    for fixture in FIXTURES {
        let path = fixture_path(fixture.file_name);
        let advertised = audio_format_for_path(&path)
            .unwrap_or_else(|| panic!("{} is absent from the support matrix", path.display()));
        let audio = decode_audio_file(&path).unwrap_or_else(|error| {
            panic!("{} ({}) failed: {error}", path.display(), advertised.label)
        });

        assert_eq!(
            audio.num_channels(),
            fixture.channels,
            "{}",
            fixture.file_name
        );
        assert_eq!(
            audio.sample_rate, fixture.sample_rate,
            "{}",
            fixture.file_name
        );
        let duration = audio.num_frames() as f64 / audio.sample_rate as f64;
        assert!(
            (0.20..=0.30).contains(&duration),
            "{} decoded to unexpected {duration:.4}s",
            fixture.file_name
        );
    }
}

#[test]
fn advertised_wav_and_aiff_aliases_decode_the_same_bytes() {
    let directory = tempfile::tempdir().unwrap();
    for (fixture, alias) in [
        ("mono-44100-s16.wav", "alias.wave"),
        ("stereo-48000-s24.aiff", "alias.aif"),
    ] {
        let alias_path = directory.path().join(alias);
        std::fs::copy(fixture_path(fixture), &alias_path).unwrap();
        assert!(audio_format_for_path(&alias_path).is_some());
        let audio = decode_audio_file(&alias_path).unwrap();
        assert!(audio.num_frames() > 0, "{alias}");
    }
}

#[test]
fn corrupt_advertised_file_is_a_recoverable_decode_error() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("corrupt.wav");
    std::fs::write(&path, b"RIFF not actually audio").unwrap();

    let error = decode_audio_file(&path).unwrap_err();
    assert!(matches!(
        error,
        FileIoError::UnsupportedFormat(_) | FileIoError::Decode(_) | FileIoError::EmptyAudio
    ));
    assert!(!error.to_string().is_empty());
}
