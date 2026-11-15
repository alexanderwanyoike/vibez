//! The notes MIDI From routes into a track, admitted so each note-on gets exactly one note-off.

use vibez_core::note_output::{BlockNote, NoteOutput};

/// A retrigger adds a note-off before its note-on, so a block can double.
const BLOCK_CAPACITY: usize = NoteOutput::CAPACITY * 2;

#[derive(Debug)]
pub(crate) struct RoutedNotes {
    incoming: Vec<BlockNote>,
    block: Vec<BlockNote>,
    sounding: u128,
    release_pending: bool,
}

impl Default for RoutedNotes {
    fn default() -> Self {
        Self {
            incoming: Vec::with_capacity(NoteOutput::CAPACITY),
            block: Vec::with_capacity(BLOCK_CAPACITY),
            sounding: 0,
            release_pending: false,
        }
    }
}

impl RoutedNotes {
    /// Replaces the block's notes with `incoming`, in frame order, after any
    /// pending release.
    pub(crate) fn admit(&mut self, _incoming: &[BlockNote]) {}

    /// Releases every routed note still sounding at the start of the next block.
    pub(crate) fn release(&mut self) {}

    /// The notes to play this block, in frame order.
    pub(crate) fn notes(&self) -> &[BlockNote] {
        &self.block
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn admitted(routed: &mut RoutedNotes, incoming: &[BlockNote]) -> Vec<BlockNote> {
        routed.admit(incoming);
        routed.notes().to_vec()
    }

    #[test]
    fn notes_play_in_frame_order_with_note_offs_first_at_one_frame() {
        let mut routed = RoutedNotes::default();
        admitted(&mut routed, &[BlockNote::on(0, 60, 90)]);

        assert_eq!(
            admitted(
                &mut routed,
                &[
                    BlockNote::on(9, 64, 80),
                    BlockNote::on(4, 62, 70),
                    BlockNote::off(4, 60),
                ]
            ),
            [
                BlockNote::off(4, 60),
                BlockNote::on(4, 62, 70),
                BlockNote::on(9, 64, 80)
            ]
        );
    }

    #[test]
    fn a_note_on_for_a_held_pitch_releases_it_first() {
        let mut routed = RoutedNotes::default();

        assert_eq!(
            admitted(
                &mut routed,
                &[BlockNote::on(2, 60, 90), BlockNote::on(7, 60, 100)]
            ),
            [
                BlockNote::on(2, 60, 90),
                BlockNote::off(7, 60),
                BlockNote::on(7, 60, 100)
            ]
        );
    }

    #[test]
    fn a_note_off_for_a_pitch_that_is_not_held_is_dropped() {
        let mut routed = RoutedNotes::default();

        assert!(admitted(&mut routed, &[BlockNote::off(3, 60)]).is_empty());
    }

    #[test]
    fn a_release_ends_held_notes_at_the_next_block_start_exactly_once() {
        let mut routed = RoutedNotes::default();
        admitted(
            &mut routed,
            &[BlockNote::on(5, 60, 90), BlockNote::on(5, 67, 90)],
        );
        routed.release();

        assert_eq!(
            admitted(&mut routed, &[BlockNote::on(3, 72, 90)]),
            [
                BlockNote::off(0, 60),
                BlockNote::off(0, 67),
                BlockNote::on(3, 72, 90)
            ]
        );
        assert!(admitted(&mut routed, &[BlockNote::off(1, 60)]).is_empty());
    }

    #[test]
    fn a_release_with_nothing_held_plays_nothing() {
        let mut routed = RoutedNotes::default();
        routed.release();

        assert!(admitted(&mut routed, &[]).is_empty());
    }
}
