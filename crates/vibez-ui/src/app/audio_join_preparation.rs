//! Background rendering and Project Media staging for joined Audio Clips.

use crate::domains::arrangement::{render_joined_audio, PreparedAudioJoin};
use crate::state::UiClip;

pub(super) async fn prepare_audio_join_async(
    clips: Vec<UiClip>,
) -> Result<PreparedAudioJoin, String> {
    tokio::task::spawn_blocking(move || {
        let joined = render_joined_audio(&clips);
        let source = super::generated_media::stage_generated_audio(
            "Joined Audio",
            "Joined.wav",
            &joined.audio,
        )?;
        Ok(PreparedAudioJoin { joined, source })
    })
    .await
    .map_err(|error| format!("Join task failed: {error}"))?
}
