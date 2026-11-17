//! Notes a track plays from outside its own clips, admitted so each note-on gets exactly one note-off.

use vibez_core::note_output::{BlockNote, NoteOutput};
use vibez_instruments::Instrument;

use super::pitch_mask::pitches;

/// A release adds a note-off per held pitch, and a retrigger adds one before
/// its note-on, so the block never outgrows this.
const BLOCK_CAPACITY: usize = 128 + NoteOutput::CAPACITY * 2;

/// The track, and which of its instruments, that played a block's routed notes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct NoteSource {
    pub(crate) track: vibez_core::id::TrackId,
    /// Changes whenever the track's instrument does.
    pub(crate) instrument: u64,
}

/// `S` names whoever plays the notes, so a change of player releases what
/// the old one started.
#[derive(Debug)]
pub(crate) struct RoutedNotes<S = NoteSource> {
    incoming: Vec<BlockNote>,
    block: Vec<BlockNote>,
    sounding: u128,
    /// Note-offs due at the next block's first frame.
    deferred: u128,
    /// The frame each held pitch last started on in this block.
    started: [u32; 128],
    source: Option<S>,
    release_pending: bool,
}

impl<S> Default for RoutedNotes<S> {
    fn default() -> Self {
        Self {
            incoming: Vec::with_capacity(NoteOutput::CAPACITY),
            block: Vec::with_capacity(BLOCK_CAPACITY),
            sounding: 0,
            deferred: 0,
            started: [u32::MAX; 128],
            source: None,
            release_pending: false,
        }
    }
}

impl<S: Copy + PartialEq> RoutedNotes<S> {
    /// Replaces the block's notes with `incoming` from `source`, in frame
    /// order, after any pending release. `source` is `None` when no source
    /// rendered this block. A changed source releases what the old one
    /// started, since it will never end those notes.
    pub(crate) fn admit(&mut self, source: Option<S>, incoming: &[BlockNote], frames: u32) {
        self.block.clear();
        if source != self.source {
            self.source = source;
            self.release_pending = true;
        }
        if std::mem::take(&mut self.release_pending) {
            self.deferred |= std::mem::take(&mut self.sounding);
        }
        for pitch in pitches(std::mem::take(&mut self.deferred)) {
            self.block.push(BlockNote::off(0, pitch));
        }
        let last = frames.saturating_sub(1);
        self.incoming.clear();
        for &note in incoming.iter().filter(|note| note.pitch < 128) {
            if self.incoming.len() == self.incoming.capacity() {
                break;
            }
            self.incoming.push(BlockNote {
                frame: note.frame.min(last),
                ..note
            });
        }
        sort_by_frame(&mut self.incoming);
        self.started = [u32::MAX; 128];
        for &note in &self.incoming {
            let bit = 1u128 << note.pitch;
            let held = self.sounding & bit != 0;
            if note.on {
                if held {
                    self.block.push(BlockNote::off(note.frame, note.pitch));
                }
                self.sounding |= bit;
                self.started[note.pitch as usize] = note.frame;
                self.block.push(note);
            } else if held {
                self.sounding &= !bit;
                // A receiving plugin plays note-offs before note-ons at one
                // frame, so an off sharing its note-on's frame moves later.
                if self.started[note.pitch as usize] != note.frame {
                    self.block.push(note);
                } else if note.frame < last {
                    self.block.push(BlockNote::off(note.frame + 1, note.pitch));
                } else {
                    self.deferred |= bit;
                }
            }
        }
        sort_by_frame(&mut self.block);
    }

    /// Releases every routed note still sounding at the start of the next block.
    pub(crate) fn release(&mut self) {
        self.release_pending = true;
    }

    /// The notes to play this block, in frame order.
    pub(crate) fn notes(&self) -> &[BlockNote] {
        &self.block
    }

    /// Hands a batch instrument every note with its frame offset.
    pub(crate) fn schedule(&self, instrument: &mut dyn Instrument) {
        for note in &self.block {
            if note.on {
                instrument.note_on_at(note.pitch, note.velocity, note.frame);
            } else {
                instrument.note_off_at(note.pitch, note.frame);
            }
        }
    }

    /// Plays the notes due at `frame` on an instrument rendered frame by frame;
    /// `cursor` starts at zero each block.
    pub(crate) fn play_due(&self, frame: u32, cursor: &mut usize, instrument: &mut dyn Instrument) {
        while let Some(note) = self.block.get(*cursor).filter(|note| note.frame == frame) {
            if note.on {
                instrument.note_on(note.pitch, note.velocity);
            } else {
                instrument.note_off(note.pitch);
            }
            *cursor += 1;
        }
    }
}

/// Stable, so notes at one frame keep the source's order, and allocation-free.
fn sort_by_frame(notes: &mut [BlockNote]) {
    for index in 1..notes.len() {
        let mut slot = index;
        while slot > 0 && notes[slot - 1].frame > notes[slot].frame {
            notes.swap(slot - 1, slot);
            slot -= 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FRAMES: u32 = 64;

    fn source() -> Option<NoteSource> {
        thread_local! { static SOURCE: vibez_core::id::TrackId = vibez_core::id::TrackId::new(); }
        Some(NoteSource {
            track: SOURCE.with(|id| *id),
            instrument: 0,
        })
    }

    fn admitted(routed: &mut RoutedNotes, incoming: &[BlockNote]) -> Vec<BlockNote> {
        routed.admit(source(), incoming, FRAMES);
        routed.notes().to_vec()
    }

    #[test]
    fn notes_play_in_frame_order_keeping_the_sources_order_inside_a_frame() {
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
                BlockNote::on(4, 62, 70),
                BlockNote::off(4, 60),
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

    #[test]
    fn a_note_on_and_its_note_off_at_one_frame_release_on_the_next_frame() {
        let mut routed = RoutedNotes::default();

        assert_eq!(
            admitted(
                &mut routed,
                &[BlockNote::on(5, 60, 90), BlockNote::off(5, 60)]
            ),
            [BlockNote::on(5, 60, 90), BlockNote::off(6, 60)]
        );
        assert_eq!(
            admitted(
                &mut routed,
                &[
                    BlockNote::on(FRAMES - 1, 62, 90),
                    BlockNote::off(FRAMES - 1, 62)
                ]
            ),
            [BlockNote::on(FRAMES - 1, 62, 90)]
        );
        assert_eq!(admitted(&mut routed, &[]), [BlockNote::off(0, 62)]);
    }

    #[test]
    fn notes_outside_the_block_or_midi_are_kept_in_range() {
        let mut routed = RoutedNotes::default();

        assert_eq!(
            admitted(
                &mut routed,
                &[BlockNote::on(FRAMES + 9, 60, 90), BlockNote::on(3, 200, 90)]
            ),
            [BlockNote::on(FRAMES - 1, 60, 90)]
        );
    }

    #[test]
    fn a_changed_or_missing_source_ends_held_notes_at_the_next_block_start() {
        let other = vibez_core::id::TrackId::new();
        for next in [
            Some(NoteSource {
                track: other,
                instrument: 0,
            }),
            Some(NoteSource {
                instrument: 1,
                ..source().unwrap()
            }),
            None,
        ] {
            let mut routed = RoutedNotes::default();
            admitted(&mut routed, &[BlockNote::on(5, 60, 90)]);

            routed.admit(next, &[], FRAMES);

            assert_eq!(routed.notes(), [BlockNote::off(0, 60)], "{next:?}");
        }
    }
}
