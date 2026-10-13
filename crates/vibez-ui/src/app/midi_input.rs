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
    /// Drain pending MIDI events from the external input port and
    /// forward them to the engine. Events route to the currently-
    /// selected track's instrument; if nothing is selected or the
    /// track has no instrument attached, events are dropped (no
    /// passthrough). Called on every UI tick.
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
        let Some(track_id) = configured_target(&self.state) else {
            return;
        };
        for event in events {
            match event {
                vibez_audio_io::midi_input::MidiEvent::NoteOn { pitch, velocity } => {
                    self.send_command(EngineCommand::ExternalNoteOn {
                        track_id,
                        pitch,
                        velocity,
                    });
                }
                vibez_audio_io::midi_input::MidiEvent::NoteOff { pitch } => {
                    self.send_command(EngineCommand::ExternalNoteOff { track_id, pitch });
                }
                vibez_audio_io::midi_input::MidiEvent::ControlChange { .. } => {
                    // CC mapping not wired yet.
                }
            }
        }
    }
}
