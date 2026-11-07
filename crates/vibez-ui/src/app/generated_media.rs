//! Stages audio Vibez renders itself as Project Media.

use std::path::Path;

use vibez_core::audio_buffer::DecodedAudio;
use vibez_core::track::MediaSourceRef;

/// `origin` labels where the audio came from and is never a real file, so a
/// legacy JSON save keeps these bytes instead of pointing back at a source.
pub(super) fn stage_generated_audio(
    origin: &str,
    file_name: &str,
    audio: &DecodedAudio,
) -> Result<MediaSourceRef, String> {
    let bytes = vibez_audio_io::file_io::encode_wav(audio);
    vibez_project::project_format_v1::stage_local_content(Path::new(origin), file_name, &bytes)
        .map_err(|error| format!("Project Media staging failed: {error}"))
}
