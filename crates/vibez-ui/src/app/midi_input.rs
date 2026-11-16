//! MIDI delivery and timing eligibility share the existing configured destination.

use super::*;

pub(super) fn configured_target(state: &crate::state::AppState) -> Option<vibez_core::id::TrackId> {
    state.arrangement.selected_track.filter(|id| {
        state
            .find_track(*id)
            .is_some_and(|track| track.is_playable_midi_target())
    })
}

impl App {
    /// Drain pending MIDI events from the external input port and play
    /// them on the configured MIDI target. Called on every UI tick.
    pub(super) fn poll_midi_input(&mut self) {
        let Some(handle) = self.midi_input.as_ref() else {
            return;
        };
        let mut events = Vec::new();
        while let Ok(event) = handle.rx.try_recv() {
            events.push(event);
        }
        if events.is_empty() {
            return;
        }
        let target = configured_target(&self.state);
        for event in events {
            self.hardware_keys.play(event, target, &mut self.cmd_tx);
        }
    }
}
