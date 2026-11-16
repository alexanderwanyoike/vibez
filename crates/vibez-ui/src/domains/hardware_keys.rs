//! Keys on the hardware MIDI input play the configured MIDI target Track.

use vibez_audio_io::midi_input::MidiEvent;
use vibez_core::id::TrackId;
use vibez_engine::commands::EngineCommand;

use super::EngineHandle;

/// The keys of the open hardware MIDI input.
#[derive(Debug, Default)]
pub struct HardwareKeys;

impl HardwareKeys {
    /// Plays `event` on `target`, the configured MIDI target Track now, and
    /// drops it when there is none.
    pub fn play(
        &mut self,
        event: MidiEvent,
        target: Option<TrackId>,
        engine: &mut impl EngineHandle,
    ) {
        let Some(track_id) = target else {
            return;
        };
        match event {
            MidiEvent::NoteOn { pitch, velocity } => {
                engine.send(EngineCommand::ExternalNoteOn {
                    track_id,
                    pitch,
                    velocity,
                });
            }
            MidiEvent::NoteOff { pitch } => {
                engine.send(EngineCommand::ExternalNoteOff { track_id, pitch });
            }
            MidiEvent::ControlChange { .. } => {
                // CC mapping not wired yet.
            }
        }
    }
}
