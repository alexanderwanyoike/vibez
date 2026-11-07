//! Stages audio Vibez renders itself as Project Media.

use std::path::Path;
use std::sync::Arc;

use vibez_core::audio_buffer::DecodedAudio;
use vibez_core::track::MediaSourceRef;

/// Generated audio as saved: its Project Media and the samples reopening it
/// will decode.
pub(super) struct StagedAudio {
    pub(super) source: MediaSourceRef,
    pub(super) audio: Arc<DecodedAudio>,
}

/// `origin` labels where the audio came from and is never a real file, so a
/// legacy JSON save keeps these bytes instead of pointing back at a source.
/// The returned audio is decoded from the saved bytes, because WAV encoding
/// rounds samples and a later Warp or Transpose of unrounded audio would
/// sound different once the project is reopened.
pub(super) fn stage_generated_audio(
    origin: &str,
    file_name: &str,
    audio: &DecodedAudio,
) -> Result<StagedAudio, String> {
    let bytes = vibez_audio_io::file_io::encode_wav(audio);
    let source =
        vibez_project::project_format_v1::stage_local_content(Path::new(origin), file_name, &bytes)
            .map_err(|error| format!("Project Media staging failed: {error}"))?;
    let saved =
        vibez_audio_io::file_io::decode_audio_cursor(std::io::Cursor::new(bytes), Some("wav"))
            .map_err(|error| format!("Project Media verification failed: {error}"))?;
    Ok(StagedAudio {
        source,
        audio: Arc::new(saved),
    })
}
