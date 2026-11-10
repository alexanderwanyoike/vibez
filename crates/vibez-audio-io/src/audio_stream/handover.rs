//! Moves the engine callback from the outgoing output stream to its replacement.

use cpal::traits::StreamTrait;

use super::AudioStreamError;

/// Stop `outgoing` and start `replacement`, resuming `outgoing` when the
/// replacement cannot start. `previous_stream_running` ends up true only if
/// the outgoing stream is driving the engine again. An outgoing stream that
/// cannot be stopped is dropped, leaving `outgoing` empty.
pub(super) fn hand_over<S: StreamTrait>(
    outgoing: &mut Option<S>,
    replacement: &S,
    previous_stream_running: &mut bool,
) -> Result<(), AudioStreamError> {
    if let Some(stream) = outgoing.as_ref() {
        if stream.pause().is_err() {
            // A stream that cannot be paused has usually lost its device.
            // Dropping it is the only way left to guarantee its callback
            // never runs beside the replacement's.
            *outgoing = None;
        }
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

#[cfg(test)]
#[path = "handover_tests.rs"]
mod tests;
