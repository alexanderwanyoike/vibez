//! Moves the engine callback from the outgoing output stream to its replacement.

use cpal::traits::StreamTrait;

use super::AudioStreamError;

/// A replacement that could not start.
#[derive(Debug)]
pub(super) struct HandoverFailure {
    pub error: AudioStreamError,
    /// Whether the previous output is driving the engine again.
    pub previous_running: bool,
}

/// Stop `previous` and start `replacement`, resuming `previous` when the
/// replacement cannot start. A previous output whose stream cannot be stopped
/// is dropped whole, leaving `previous` empty.
pub(super) fn hand_over<O, S: StreamTrait>(
    previous: &mut Option<O>,
    stream_of: impl Fn(&O) -> &S,
    replacement: &S,
) -> Result<(), HandoverFailure> {
    if let Some(output) = previous.as_ref() {
        if stream_of(output).pause().is_err() {
            // A stream that cannot be paused has usually lost its device.
            // Dropping it is the only way left to guarantee its callback
            // never runs beside the replacement's.
            *previous = None;
        }
    }
    replacement.play().map_err(|error| HandoverFailure {
        error: error.into(),
        previous_running: previous
            .as_ref()
            .is_some_and(|output| stream_of(output).play().is_ok()),
    })
}

#[cfg(test)]
#[path = "handover_tests.rs"]
mod tests;
