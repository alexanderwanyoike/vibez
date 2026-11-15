//! Converts a note a plugin outputs, in either format's terms, into a block note.

use vibez_core::note_output::BlockNote;

/// Room for a dense arpeggio or drum fill in one block; more is dropped and counted.
pub(crate) const NOTE_OUTPUT_CAPACITY: usize = 1024;

pub(crate) fn note_on(_frame: u32, _frames: u32, _key: i32, _velocity: f64) -> Option<BlockNote> {
    None
}

pub(crate) fn note_off(_frame: u32, _frames: u32, _key: i32) -> Option<BlockNote> {
    None
}

pub(crate) fn midi(_frame: u32, _frames: u32, _data: [u8; 3]) -> Option<BlockNote> {
    None
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
