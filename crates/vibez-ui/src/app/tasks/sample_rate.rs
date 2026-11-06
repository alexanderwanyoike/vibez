//! Bringing decoded audio to the rate the engine plays at.

use vibez_core::audio_buffer::DecodedAudio;

/// The engine reads clip and sampler audio one source frame per output
/// frame, so audio must arrive at the engine's rate to play at its pitch.
pub(in crate::app) async fn at_engine_rate(
    audio: DecodedAudio,
    sample_rate: u32,
) -> Result<DecodedAudio, String> {
    if audio.sample_rate == sample_rate {
        return Ok(audio);
    }
    super::run_off_ui_thread("Resample", move || {
        vibez_audio_io::file_io::into_sample_rate(audio, sample_rate)
            .map_err(|error| error.to_string())
    })
    .await?
}
