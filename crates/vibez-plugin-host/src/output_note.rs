//! Converts a note a plugin outputs, in either format's terms, into a block note.

use vibez_core::note_output::BlockNote;

/// Room for a dense arpeggio or drum fill in one block; more is dropped and counted.
pub(crate) const NOTE_OUTPUT_CAPACITY: usize = 1024;

/// `key` and `velocity` (0..=1) arrive as the plugin wrote them, so a key
/// outside MIDI or a frame past the block is the plugin's error to absorb.
pub(crate) fn note_on(frame: u32, frames: u32, key: i32, velocity: f64) -> Option<BlockNote> {
    let velocity = (velocity.clamp(0.0, 1.0) * 127.0).round().max(1.0) as u8;
    Some(BlockNote::on(within(frame, frames), pitch(key)?, velocity))
}

pub(crate) fn note_off(frame: u32, frames: u32, key: i32) -> Option<BlockNote> {
    Some(BlockNote::off(within(frame, frames), pitch(key)?))
}

/// Raw MIDI 1.0 note messages on any channel; a note-on at velocity 0 is a note-off.
pub(crate) fn midi(frame: u32, frames: u32, data: [u8; 3]) -> Option<BlockNote> {
    match (data[0] & 0xF0, data[2]) {
        (0x90, 0) | (0x80, _) => note_off(frame, frames, i32::from(data[1])),
        (0x90, velocity) => note_on(
            frame,
            frames,
            i32::from(data[1]),
            f64::from(velocity) / 127.0,
        ),
        _ => None,
    }
}

fn pitch(key: i32) -> Option<u8> {
    u8::try_from(key).ok().filter(|pitch| *pitch < 128)
}

fn within(frame: u32, frames: u32) -> u32 {
    frame.min(frames.saturating_sub(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn velocity_rounds_to_midi_and_a_sounding_note_never_has_velocity_zero() {
        assert_eq!(note_on(0, 64, 60, 100.0 / 127.0).unwrap().velocity, 100);
        assert_eq!(note_on(0, 64, 60, 0.0).unwrap().velocity, 1);
        assert_eq!(note_on(0, 64, 60, 2.0).unwrap().velocity, 127);
    }

    #[test]
    fn keys_outside_midi_are_ignored_and_late_frames_land_on_the_last_frame() {
        assert_eq!(note_on(0, 64, -1, 1.0), None);
        assert_eq!(note_off(0, 64, 128), None);
        assert_eq!(note_off(90, 64, 60), Some(BlockNote::off(63, 60)));
    }

    #[test]
    fn raw_midi_note_messages_become_block_notes() {
        assert_eq!(
            midi(4, 64, [0x93, 62, 100]),
            Some(BlockNote::on(4, 62, 100))
        );
        assert_eq!(midi(4, 64, [0x90, 62, 0]), Some(BlockNote::off(4, 62)));
        assert_eq!(midi(4, 64, [0x81, 62, 64]), Some(BlockNote::off(4, 62)));
        assert_eq!(midi(4, 64, [0xB0, 1, 64]), None);
    }
}
