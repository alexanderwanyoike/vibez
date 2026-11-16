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

#[cfg(test)]
mod tests {
    use super::super::test_support::RecordingEngine;
    use super::*;

    const C4: u8 = 60;

    /// The live notes sent, as (track, pitch, on).
    fn notes(engine: &RecordingEngine) -> Vec<(TrackId, u8, bool)> {
        engine
            .0
            .iter()
            .filter_map(|command| match command {
                EngineCommand::ExternalNoteOn {
                    track_id, pitch, ..
                } => Some((*track_id, *pitch, true)),
                EngineCommand::ExternalNoteOff { track_id, pitch } => {
                    Some((*track_id, *pitch, false))
                }
                _ => None,
            })
            .collect()
    }

    fn press(pitch: u8) -> MidiEvent {
        MidiEvent::NoteOn {
            pitch,
            velocity: 100,
        }
    }

    fn release(pitch: u8) -> MidiEvent {
        MidiEvent::NoteOff { pitch }
    }

    #[test]
    fn a_key_released_after_selecting_another_track_releases_where_it_was_pressed() {
        let (a, b) = (TrackId::new(), TrackId::new());
        let mut keys = HardwareKeys::default();
        let mut engine = RecordingEngine::default();

        keys.play(press(C4), Some(a), &mut engine);
        keys.play(release(C4), Some(b), &mut engine);

        assert_eq!(notes(&engine), [(a, C4, true), (a, C4, false)]);
    }

    #[test]
    fn a_key_released_with_no_target_left_still_releases() {
        let a = TrackId::new();
        let mut keys = HardwareKeys::default();
        let mut engine = RecordingEngine::default();

        keys.play(press(C4), Some(a), &mut engine);
        keys.play(release(C4), None, &mut engine);

        assert_eq!(notes(&engine), [(a, C4, true), (a, C4, false)]);
    }

    #[test]
    fn a_key_pressed_again_on_another_track_first_releases_the_first() {
        let (a, b) = (TrackId::new(), TrackId::new());
        let mut keys = HardwareKeys::default();
        let mut engine = RecordingEngine::default();

        keys.play(press(C4), Some(a), &mut engine);
        keys.play(press(C4), Some(b), &mut engine);
        keys.play(release(C4), Some(b), &mut engine);

        assert_eq!(
            notes(&engine),
            [(a, C4, true), (a, C4, false), (b, C4, true), (b, C4, false)]
        );
    }

    #[test]
    fn a_key_up_without_a_press_it_played_is_dropped() {
        let b = TrackId::new();
        let mut keys = HardwareKeys::default();
        let mut engine = RecordingEngine::default();

        keys.play(press(C4), None, &mut engine);
        keys.play(release(C4), Some(b), &mut engine);

        assert_eq!(notes(&engine), []);
    }
}
