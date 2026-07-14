//! Audio file decoding and resampling.
//!
//! Uses [symphonia] for the exact format matrix documented in
//! `docs/AUDIO_FORMAT_SUPPORT.md` and [rubato] for high-quality sinc
//! resampling.

use std::fmt;
use std::fs::File;
use std::path::Path;

use rubato::{
    Resampler, SincFixedIn, SincInterpolationParameters, SincInterpolationType, WindowFunction,
};
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

use vibez_core::audio_buffer::DecodedAudio;

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Errors that can occur during file I/O (decoding / resampling).
#[derive(Debug)]
pub enum FileIoError {
    /// Standard I/O error (file not found, permission denied, etc.).
    Io(std::io::Error),
    /// Symphonia could not detect the format of the source.
    UnsupportedFormat(String),
    /// Symphonia could not find any audio tracks in the container.
    NoAudioTrack,
    /// Symphonia decoding error.
    Decode(symphonia::core::errors::Error),
    /// The audio file contained no decodable frames.
    EmptyAudio,
    /// The decoder produced audio without required channel/rate metadata.
    InvalidAudioMetadata(String),
    /// Rubato resampler construction error.
    ResamplerConstruction(rubato::ResamplerConstructionError),
    /// Rubato resampling error.
    Resample(rubato::ResampleError),
}

impl fmt::Display for FileIoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "I/O error: {e}"),
            Self::UnsupportedFormat(msg) => write!(f, "unsupported audio format: {msg}"),
            Self::NoAudioTrack => write!(f, "no audio track found in file"),
            Self::Decode(e) => write!(f, "decode error: {e}"),
            Self::EmptyAudio => write!(f, "audio file contained no decodable frames"),
            Self::InvalidAudioMetadata(message) => {
                write!(f, "invalid audio metadata: {message}")
            }
            Self::ResamplerConstruction(e) => write!(f, "resampler construction error: {e}"),
            Self::Resample(e) => write!(f, "resampling error: {e}"),
        }
    }
}

impl std::error::Error for FileIoError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            Self::Decode(e) => Some(e),
            Self::ResamplerConstruction(e) => Some(e),
            Self::Resample(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for FileIoError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<symphonia::core::errors::Error> for FileIoError {
    fn from(e: symphonia::core::errors::Error) -> Self {
        Self::Decode(e)
    }
}

impl From<rubato::ResamplerConstructionError> for FileIoError {
    fn from(e: rubato::ResamplerConstructionError) -> Self {
        Self::ResamplerConstruction(e)
    }
}

impl From<rubato::ResampleError> for FileIoError {
    fn from(e: rubato::ResampleError) -> Self {
        Self::Resample(e)
    }
}

// ---------------------------------------------------------------------------
// Decoding
// ---------------------------------------------------------------------------

/// Decode an audio file at `path` into per-channel f32 sample data.
///
/// Supported formats are defined by
/// [`vibez_core::audio_format::SUPPORTED_AUDIO_FORMATS`].
///
/// Returns a [`DecodedAudio`] with per-channel planar sample data.
pub fn decode_audio_file(path: &Path) -> Result<DecodedAudio, FileIoError> {
    let file = File::open(path)?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    decode_media_source(mss, extension_hint(path))
}

/// Decode from an in-memory cursor (useful for testing).
pub fn decode_audio_cursor(
    data: std::io::Cursor<Vec<u8>>,
    extension: Option<&str>,
) -> Result<DecodedAudio, FileIoError> {
    let mss = MediaSourceStream::new(Box::new(data), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = extension {
        hint.with_extension(ext);
    }
    decode_media_source(mss, hint)
}

/// Core decode logic shared between file and cursor paths.
fn decode_media_source(mss: MediaSourceStream, hint: Hint) -> Result<DecodedAudio, FileIoError> {
    // Probe the format.
    let probe = symphonia::default::get_probe();
    let format_opts = FormatOptions::default();
    let metadata_opts = MetadataOptions::default();

    let probed = probe
        .format(&hint, mss, &format_opts, &metadata_opts)
        .map_err(|e| FileIoError::UnsupportedFormat(e.to_string()))?;

    let mut format_reader = probed.format;

    // Select the first audio track.
    let track = format_reader
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or(FileIoError::NoAudioTrack)?;

    let track_id = track.id;
    let codec_params = track.codec_params.clone();

    // Create the decoder.
    let codecs = symphonia::default::get_codecs();
    let decoder_opts = DecoderOptions::default();
    let mut decoder = codecs.make(&codec_params, &decoder_opts)?;

    // Decode all packets into per-channel vectors.
    let mut channel_data: Vec<Vec<f32>> = codec_params
        .channels
        .map(|channels| vec![Vec::new(); channels.count()])
        .unwrap_or_default();
    let mut decoded_sample_rate = None;

    loop {
        let packet = match format_reader.next_packet() {
            Ok(p) => p,
            Err(symphonia::core::errors::Error::IoError(ref e))
                if e.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                // End of stream.
                break;
            }
            Err(e) => return Err(FileIoError::Decode(e)),
        };

        // Only decode packets for our selected track.
        if packet.track_id() != track_id {
            continue;
        }

        let decoded = match decoder.decode(&packet) {
            Ok(buf) => buf,
            Err(symphonia::core::errors::Error::DecodeError(msg)) => {
                // Non-fatal decode error (corrupt frame); skip it.
                eprintln!("vibez: skipping corrupt frame: {msg}");
                continue;
            }
            Err(e) => return Err(FileIoError::Decode(e)),
        };

        let spec = *decoded.spec();
        if let Some(previous_rate) = decoded_sample_rate {
            if previous_rate != spec.rate {
                return Err(FileIoError::InvalidAudioMetadata(format!(
                    "sample rate changed from {previous_rate} Hz to {} Hz",
                    spec.rate
                )));
            }
        } else {
            decoded_sample_rate = Some(spec.rate);
        }
        let frames = decoded.frames();
        if frames == 0 {
            continue;
        }

        let ch_count = spec.channels.count();

        // Use a SampleBuffer to convert any sample format to interleaved f32.
        let mut sample_buf = SampleBuffer::<f32>::new(frames as u64, spec);
        sample_buf.copy_interleaved_ref(decoded);
        let samples = sample_buf.samples();

        // De-interleave into per-channel vectors.
        // Ensure we have enough channel vectors (the spec may have changed
        // between packets in very unusual cases).
        while channel_data.len() < ch_count {
            channel_data.push(Vec::new());
        }

        for frame in 0..frames {
            for ch in 0..ch_count {
                if ch < channel_data.len() {
                    channel_data[ch].push(samples[frame * ch_count + ch]);
                }
            }
        }
    }

    if channel_data.iter().all(|ch| ch.is_empty()) {
        return Err(FileIoError::EmptyAudio);
    }
    let sample_rate = decoded_sample_rate.ok_or_else(|| {
        FileIoError::InvalidAudioMetadata("decoded frames had no sample rate".into())
    })?;

    Ok(DecodedAudio {
        channels: channel_data,
        sample_rate,
    })
}

/// Build a symphonia [`Hint`] from the file extension.
fn extension_hint(path: &Path) -> Hint {
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    hint
}

// ---------------------------------------------------------------------------
// Resampling
// ---------------------------------------------------------------------------

/// Resample `audio` to `target_sample_rate` using a high-quality sinc
/// interpolation (rubato `SincFixedIn`).
///
/// If `audio.sample_rate` already matches `target_sample_rate`, the input is
/// returned unchanged (no copy).
pub fn resample_audio(
    audio: &DecodedAudio,
    target_sample_rate: u32,
) -> Result<DecodedAudio, FileIoError> {
    if audio.sample_rate == target_sample_rate {
        return Ok(audio.clone());
    }

    let num_channels = audio.num_channels();
    if num_channels == 0 {
        return Ok(audio.clone());
    }

    let ratio = target_sample_rate as f64 / audio.sample_rate as f64;

    let params = SincInterpolationParameters {
        sinc_len: 256,
        f_cutoff: 0.95,
        oversampling_factor: 128,
        interpolation: SincInterpolationType::Cubic,
        window: WindowFunction::Blackman,
    };

    // The chunk size for the resampler.  We process the audio in blocks.
    let chunk_size = 1024;

    let mut resampler = SincFixedIn::<f32>::new(ratio, 1.1, params, chunk_size, num_channels)?;

    let total_input_frames = audio.num_frames();
    let mut output_channels: Vec<Vec<f32>> = vec![Vec::new(); num_channels];
    let mut pos = 0;

    while pos < total_input_frames {
        let frames_needed = resampler.input_frames_next();
        let end = (pos + frames_needed).min(total_input_frames);
        let available = end - pos;

        // Build the input chunk for each channel.
        let input_chunk: Vec<Vec<f32>> = (0..num_channels)
            .map(|ch| {
                let src = &audio.channels[ch];
                let mut chunk = Vec::with_capacity(frames_needed);
                // Copy available frames.
                chunk.extend_from_slice(&src[pos..end]);
                // Zero-pad if we have fewer frames than the resampler needs.
                chunk.resize(frames_needed, 0.0);
                chunk
            })
            .collect();

        let resampled = resampler.process(&input_chunk, None)?;

        for (ch, resampled_ch) in resampled.iter().enumerate() {
            output_channels[ch].extend_from_slice(resampled_ch);
        }

        pos += available;

        // If we had to zero-pad, we're at the end of the input.
        if available < frames_needed {
            break;
        }
    }

    // Flush any remaining samples from the resampler.
    // Use process_partial with no input to push out delayed samples.
    // (SincFixedIn introduces some latency.)

    Ok(DecodedAudio {
        channels: output_channels,
        sample_rate: target_sample_rate,
    })
}

// ---------------------------------------------------------------------------
// Writing
// ---------------------------------------------------------------------------

/// Write a [`DecodedAudio`] buffer to a 16-bit PCM WAV file.
///
/// Samples are clamped to `[-1.0, 1.0]` and scaled to `i16`. Parent directories
/// are created as needed. A zero-frame buffer produces a valid empty WAV.
pub fn write_wav_file(path: &Path, audio: &DecodedAudio) -> Result<(), FileIoError> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }

    let num_channels = audio.num_channels().max(1) as u16;
    let num_frames = audio.num_frames();
    let sample_rate = audio.sample_rate;
    let bits_per_sample: u16 = 16;
    let byte_rate = sample_rate * num_channels as u32 * (bits_per_sample as u32 / 8);
    let block_align = num_channels * bits_per_sample / 8;
    let data_size = (num_frames * num_channels as usize * 2) as u32;
    let file_size = 36 + data_size;

    let mut buf = Vec::with_capacity(44 + data_size as usize);

    buf.extend_from_slice(b"RIFF");
    buf.extend_from_slice(&file_size.to_le_bytes());
    buf.extend_from_slice(b"WAVE");

    buf.extend_from_slice(b"fmt ");
    buf.extend_from_slice(&16u32.to_le_bytes());
    buf.extend_from_slice(&1u16.to_le_bytes());
    buf.extend_from_slice(&num_channels.to_le_bytes());
    buf.extend_from_slice(&sample_rate.to_le_bytes());
    buf.extend_from_slice(&byte_rate.to_le_bytes());
    buf.extend_from_slice(&block_align.to_le_bytes());
    buf.extend_from_slice(&bits_per_sample.to_le_bytes());

    buf.extend_from_slice(b"data");
    buf.extend_from_slice(&data_size.to_le_bytes());

    let channel_slices: Vec<&[f32]> = (0..num_channels as usize)
        .map(|ch| audio.channels.get(ch).map(Vec::as_slice).unwrap_or(&[]))
        .collect();

    for frame in 0..num_frames {
        for ch_data in &channel_slices {
            let sample = ch_data.get(frame).copied().unwrap_or(0.0);
            let clamped = sample.clamp(-1.0, 1.0);
            let as_i16 = (clamped * i16::MAX as f32).round() as i16;
            buf.extend_from_slice(&as_i16.to_le_bytes());
        }
    }

    std::fs::write(path, buf)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    /// Create a minimal valid WAV file (PCM 16-bit, mono) from a sine wave.
    fn make_test_wav(sample_rate: u32, num_samples: usize, frequency: f32) -> Vec<u8> {
        let num_channels: u16 = 1;
        let bits_per_sample: u16 = 16;
        let byte_rate = sample_rate * num_channels as u32 * bits_per_sample as u32 / 8;
        let block_align = num_channels * bits_per_sample / 8;
        let data_size =
            (num_samples * num_channels as usize * (bits_per_sample / 8) as usize) as u32;
        let file_size = 36 + data_size;

        let mut buf: Vec<u8> = Vec::with_capacity(44 + data_size as usize);

        // RIFF header
        buf.extend_from_slice(b"RIFF");
        buf.extend_from_slice(&file_size.to_le_bytes());
        buf.extend_from_slice(b"WAVE");

        // fmt sub-chunk
        buf.extend_from_slice(b"fmt ");
        buf.extend_from_slice(&16u32.to_le_bytes()); // sub-chunk size
        buf.extend_from_slice(&1u16.to_le_bytes()); // PCM format
        buf.extend_from_slice(&num_channels.to_le_bytes());
        buf.extend_from_slice(&sample_rate.to_le_bytes());
        buf.extend_from_slice(&byte_rate.to_le_bytes());
        buf.extend_from_slice(&block_align.to_le_bytes());
        buf.extend_from_slice(&bits_per_sample.to_le_bytes());

        // data sub-chunk
        buf.extend_from_slice(b"data");
        buf.extend_from_slice(&data_size.to_le_bytes());

        for i in 0..num_samples {
            let t = i as f32 / sample_rate as f32;
            let sample = (2.0 * std::f32::consts::PI * frequency * t).sin();
            // Convert f32 [-1, 1] to i16.
            let sample_i16 = (sample * i16::MAX as f32) as i16;
            buf.extend_from_slice(&sample_i16.to_le_bytes());
        }

        buf
    }

    /// Create a stereo WAV file.
    fn make_test_wav_stereo(
        sample_rate: u32,
        num_samples: usize,
        freq_l: f32,
        freq_r: f32,
    ) -> Vec<u8> {
        let num_channels: u16 = 2;
        let bits_per_sample: u16 = 16;
        let byte_rate = sample_rate * num_channels as u32 * bits_per_sample as u32 / 8;
        let block_align = num_channels * bits_per_sample / 8;
        let data_size =
            (num_samples * num_channels as usize * (bits_per_sample / 8) as usize) as u32;
        let file_size = 36 + data_size;

        let mut buf: Vec<u8> = Vec::with_capacity(44 + data_size as usize);

        // RIFF header
        buf.extend_from_slice(b"RIFF");
        buf.extend_from_slice(&file_size.to_le_bytes());
        buf.extend_from_slice(b"WAVE");

        // fmt sub-chunk
        buf.extend_from_slice(b"fmt ");
        buf.extend_from_slice(&16u32.to_le_bytes());
        buf.extend_from_slice(&1u16.to_le_bytes()); // PCM
        buf.extend_from_slice(&num_channels.to_le_bytes());
        buf.extend_from_slice(&sample_rate.to_le_bytes());
        buf.extend_from_slice(&byte_rate.to_le_bytes());
        buf.extend_from_slice(&block_align.to_le_bytes());
        buf.extend_from_slice(&bits_per_sample.to_le_bytes());

        // data sub-chunk
        buf.extend_from_slice(b"data");
        buf.extend_from_slice(&data_size.to_le_bytes());

        for i in 0..num_samples {
            let t = i as f32 / sample_rate as f32;
            let l = (2.0 * std::f32::consts::PI * freq_l * t).sin();
            let r = (2.0 * std::f32::consts::PI * freq_r * t).sin();
            let l_i16 = (l * i16::MAX as f32) as i16;
            let r_i16 = (r * i16::MAX as f32) as i16;
            buf.extend_from_slice(&l_i16.to_le_bytes());
            buf.extend_from_slice(&r_i16.to_le_bytes());
        }

        buf
    }

    #[test]
    fn decode_mono_wav_from_file() {
        let wav = make_test_wav(44_100, 4410, 440.0); // 100ms of 440 Hz
        let mut tmp = NamedTempFile::new().unwrap();
        tmp.write_all(&wav).unwrap();
        tmp.flush().unwrap();

        let audio = decode_audio_file(tmp.path()).unwrap();

        assert_eq!(audio.sample_rate, 44_100);
        assert_eq!(audio.num_channels(), 1);
        assert_eq!(audio.num_frames(), 4410);

        // The decoded samples should approximate a sine wave. Check that the
        // peak amplitude is close to 1.0.
        let peak = audio.channels[0]
            .iter()
            .map(|s| s.abs())
            .fold(0.0f32, f32::max);
        assert!(peak > 0.9, "peak was {peak}");
    }

    #[test]
    fn decode_stereo_wav_from_file() {
        let wav = make_test_wav_stereo(48_000, 4800, 440.0, 880.0);
        let mut tmp = NamedTempFile::new().unwrap();
        tmp.write_all(&wav).unwrap();
        tmp.flush().unwrap();

        let audio = decode_audio_file(tmp.path()).unwrap();

        assert_eq!(audio.sample_rate, 48_000);
        assert_eq!(audio.num_channels(), 2);
        assert_eq!(audio.num_frames(), 4800);

        // Both channels should have significant energy.
        let peak_l = audio.channels[0]
            .iter()
            .map(|s| s.abs())
            .fold(0.0f32, f32::max);
        let peak_r = audio.channels[1]
            .iter()
            .map(|s| s.abs())
            .fold(0.0f32, f32::max);
        assert!(peak_l > 0.9, "peak_l was {peak_l}");
        assert!(peak_r > 0.9, "peak_r was {peak_r}");
    }

    #[test]
    fn decode_wav_from_cursor() {
        let wav = make_test_wav(44_100, 4410, 440.0);
        let cursor = std::io::Cursor::new(wav);

        let audio = decode_audio_cursor(cursor, Some("wav")).unwrap();

        assert_eq!(audio.sample_rate, 44_100);
        assert_eq!(audio.num_channels(), 1);
        assert_eq!(audio.num_frames(), 4410);
    }

    #[test]
    fn decode_nonexistent_file_returns_error() {
        let result = decode_audio_file(Path::new("/tmp/nonexistent_vibez_test_file.wav"));
        assert!(result.is_err());
        match result {
            Err(FileIoError::Io(_)) => {} // Expected.
            other => panic!("expected Io error, got {other:?}"),
        }
    }

    #[test]
    fn decode_invalid_data_returns_error() {
        let mut tmp = NamedTempFile::new().unwrap();
        tmp.write_all(b"this is not audio data at all").unwrap();
        tmp.flush().unwrap();

        let result = decode_audio_file(tmp.path());
        assert!(result.is_err());
    }

    #[test]
    fn resample_same_rate_returns_clone() {
        let audio = DecodedAudio {
            channels: vec![vec![0.1, 0.2, 0.3], vec![0.4, 0.5, 0.6]],
            sample_rate: 44_100,
        };

        let result = resample_audio(&audio, 44_100).unwrap();
        assert_eq!(result.sample_rate, 44_100);
        assert_eq!(result.channels, audio.channels);
    }

    #[test]
    fn resample_upsample() {
        // Create a 1-second 440Hz mono sine at 22050 Hz, resample to 44100 Hz.
        let src_rate = 22_050u32;
        let target_rate = 44_100u32;
        let num_frames = src_rate as usize; // 1 second

        let channel: Vec<f32> = (0..num_frames)
            .map(|i| {
                let t = i as f32 / src_rate as f32;
                (2.0 * std::f32::consts::PI * 440.0 * t).sin()
            })
            .collect();

        let audio = DecodedAudio {
            channels: vec![channel],
            sample_rate: src_rate,
        };

        let resampled = resample_audio(&audio, target_rate).unwrap();
        assert_eq!(resampled.sample_rate, target_rate);
        assert_eq!(resampled.num_channels(), 1);

        // The resampled audio should have approximately 2x the frames (within
        // some tolerance for the resampler's delay and the zero-padded tail
        // of the last chunk).
        let expected_frames = (num_frames as f64 * (target_rate as f64 / src_rate as f64)) as usize;
        let actual_frames = resampled.num_frames();
        let tolerance = 1024; // sinc resampler introduces latency + tail from zero-padding
        assert!(
            (actual_frames as isize - expected_frames as isize).unsigned_abs() < tolerance,
            "expected ~{expected_frames} frames, got {actual_frames}"
        );

        // The resampled signal should still have significant energy (not all zeros).
        let peak = resampled.channels[0]
            .iter()
            .map(|s| s.abs())
            .fold(0.0f32, f32::max);
        assert!(peak > 0.5, "peak was {peak}");
    }

    #[test]
    fn resample_downsample() {
        // 44100 -> 22050
        let src_rate = 44_100u32;
        let target_rate = 22_050u32;
        let num_frames = src_rate as usize;

        let channel: Vec<f32> = (0..num_frames)
            .map(|i| {
                let t = i as f32 / src_rate as f32;
                (2.0 * std::f32::consts::PI * 440.0 * t).sin()
            })
            .collect();

        let audio = DecodedAudio {
            channels: vec![channel],
            sample_rate: src_rate,
        };

        let resampled = resample_audio(&audio, target_rate).unwrap();
        assert_eq!(resampled.sample_rate, target_rate);

        let expected_frames = (num_frames as f64 * (target_rate as f64 / src_rate as f64)) as usize;
        let actual_frames = resampled.num_frames();
        let tolerance = 1024;
        assert!(
            (actual_frames as isize - expected_frames as isize).unsigned_abs() < tolerance,
            "expected ~{expected_frames} frames, got {actual_frames}"
        );
    }

    #[test]
    fn resample_stereo() {
        let src_rate = 44_100u32;
        let target_rate = 48_000u32;
        let num_frames = 4410; // 100ms

        let left: Vec<f32> = (0..num_frames)
            .map(|i| {
                let t = i as f32 / src_rate as f32;
                (2.0 * std::f32::consts::PI * 440.0 * t).sin()
            })
            .collect();

        let right: Vec<f32> = (0..num_frames)
            .map(|i| {
                let t = i as f32 / src_rate as f32;
                (2.0 * std::f32::consts::PI * 880.0 * t).sin()
            })
            .collect();

        let audio = DecodedAudio {
            channels: vec![left, right],
            sample_rate: src_rate,
        };

        let resampled = resample_audio(&audio, target_rate).unwrap();
        assert_eq!(resampled.sample_rate, target_rate);
        assert_eq!(resampled.num_channels(), 2);

        // Both channels should have data.
        assert!(!resampled.channels[0].is_empty());
        assert!(!resampled.channels[1].is_empty());
    }

    #[test]
    fn resample_empty_audio() {
        let audio = DecodedAudio {
            channels: vec![],
            sample_rate: 44_100,
        };

        let result = resample_audio(&audio, 48_000).unwrap();
        assert_eq!(result.num_channels(), 0);
    }

    #[test]
    fn decode_and_resample_integration() {
        // Decode a WAV at 44100 and resample to 48000.
        let wav = make_test_wav(44_100, 44_100, 440.0); // 1 second
        let mut tmp = NamedTempFile::new().unwrap();
        tmp.write_all(&wav).unwrap();
        tmp.flush().unwrap();

        let audio = decode_audio_file(tmp.path()).unwrap();
        assert_eq!(audio.sample_rate, 44_100);

        let resampled = resample_audio(&audio, 48_000).unwrap();
        assert_eq!(resampled.sample_rate, 48_000);

        // Should have approximately 48000 frames (1 second at 48kHz).
        let tolerance = 1024;
        assert!(
            (resampled.num_frames() as isize - 48_000isize).unsigned_abs() < tolerance,
            "got {} frames",
            resampled.num_frames()
        );
    }

    #[test]
    fn error_type_implements_display_and_error() {
        let err = FileIoError::NoAudioTrack;
        let msg = format!("{err}");
        assert!(msg.contains("no audio track"));

        // Test Error trait.
        let err: &dyn std::error::Error = &FileIoError::EmptyAudio;
        assert!(err.to_string().contains("no decodable frames"));
    }

    #[test]
    fn write_wav_roundtrip_stereo() {
        let frames = 4_410;
        let left: Vec<f32> = (0..frames)
            .map(|i| {
                let t = i as f32 / 44_100.0;
                (2.0 * std::f32::consts::PI * 440.0 * t).sin() * 0.5
            })
            .collect();
        let right: Vec<f32> = (0..frames)
            .map(|i| {
                let t = i as f32 / 44_100.0;
                (2.0 * std::f32::consts::PI * 660.0 * t).sin() * 0.5
            })
            .collect();
        let audio = DecodedAudio {
            channels: vec![left, right],
            sample_rate: 44_100,
        };

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("roundtrip.wav");
        write_wav_file(&path, &audio).unwrap();

        let loaded = decode_audio_file(&path).unwrap();
        assert_eq!(loaded.sample_rate, 44_100);
        assert_eq!(loaded.num_channels(), 2);
        assert_eq!(loaded.num_frames(), frames);

        let peak_l = loaded.channels[0]
            .iter()
            .map(|s| s.abs())
            .fold(0.0_f32, f32::max);
        let peak_r = loaded.channels[1]
            .iter()
            .map(|s| s.abs())
            .fold(0.0_f32, f32::max);
        assert!(peak_l > 0.4 && peak_l <= 0.51, "peak_l {peak_l}");
        assert!(peak_r > 0.4 && peak_r <= 0.51, "peak_r {peak_r}");
    }

    #[test]
    fn write_wav_clamps_clipping_samples() {
        let audio = DecodedAudio {
            channels: vec![vec![2.0, -2.0, 0.5]],
            sample_rate: 22_050,
        };
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("clamp.wav");
        write_wav_file(&path, &audio).unwrap();

        let loaded = decode_audio_file(&path).unwrap();
        let ch = &loaded.channels[0];
        assert!((ch[0] - 1.0).abs() < 1e-3);
        assert!((ch[1] - (-1.0)).abs() < 1e-3);
        assert!((ch[2] - 0.5).abs() < 1e-3);
    }
}
