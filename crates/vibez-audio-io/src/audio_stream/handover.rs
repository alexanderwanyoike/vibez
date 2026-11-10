//! Moves the engine callback from the outgoing output stream to its replacement.

use cpal::traits::StreamTrait;

use super::AudioStreamError;

/// Stop `outgoing` and start `replacement`, resuming `outgoing` when the
/// replacement cannot start. `previous_stream_running` ends up true only if
/// the outgoing stream is driving the engine again.
pub(super) fn hand_over<S: StreamTrait>(
    outgoing: &mut Option<S>,
    replacement: &S,
    previous_stream_running: &mut bool,
) -> Result<(), AudioStreamError> {
    if let Some(stream) = outgoing.as_ref() {
        stream.pause()?;
        *previous_stream_running = false;
    }
    if let Err(error) = replacement.play() {
        if let Some(stream) = outgoing.as_ref() {
            *previous_stream_running = stream.play().is_ok();
        }
        return Err(error.into());
    }
    Ok(())
}
