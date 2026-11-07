//! Stages audio Vibez renders itself as Project Media.

use std::path::Path;
use std::sync::Arc;

use vibez_core::audio_buffer::DecodedAudio;
use vibez_core::track::MediaSourceRef;

/// Rendered audio encoded for saving, and the samples those bytes decode to.
pub(super) struct SavedWav {
    pub(super) bytes: Vec<u8>,
    pub(super) audio: Arc<DecodedAudio>,
}

/// WAV encoding rounds samples, and a later Warp or Transpose amplifies the
/// rounding, so rendered audio must play what reopening will decode.
pub(super) fn saved_wav(audio: &DecodedAudio) -> Result<SavedWav, String> {
    let bytes = vibez_audio_io::file_io::encode_wav(audio);
    let decoded = vibez_audio_io::file_io::decode_audio_cursor(
        std::io::Cursor::new(bytes.clone()),
        Some("wav"),
    )
    .map_err(|error| format!("Rendered audio could not be decoded back: {error}"))?;
    Ok(SavedWav {
        bytes,
        audio: Arc::new(decoded),
    })
}

/// Generated audio as saved: its Project Media and the samples reopening it
/// will decode.
pub(super) struct StagedAudio {
    pub(super) source: MediaSourceRef,
    pub(super) audio: Arc<DecodedAudio>,
}

/// `origin` labels where the audio came from and is never a real file, so a
/// legacy JSON save keeps these bytes instead of pointing back at a source.
pub(super) fn stage_generated_audio(
    origin: &str,
    file_name: &str,
    audio: &DecodedAudio,
) -> Result<StagedAudio, String> {
    let saved = saved_wav(audio)?;
    let source = vibez_project::project_format_v1::stage_local_content(
        Path::new(origin),
        file_name,
        &saved.bytes,
    )
    .map_err(|error| format!("Project Media staging failed: {error}"))?;
    Ok(StagedAudio {
        source,
        audio: saved.audio,
    })
}
