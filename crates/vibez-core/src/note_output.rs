//! The notes a device played during one process call, held in fixed capacity for the audio thread.

/// One note edge at a frame offset inside the block that produced it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockNote {
    pub frame: u32,
    pub pitch: u8,
    pub velocity: u8,
    pub on: bool,
}

impl BlockNote {
    pub fn on(frame: u32, pitch: u8, velocity: u8) -> Self {
        Self {
            frame,
            pitch,
            velocity,
            on: true,
        }
    }

    pub fn off(frame: u32, pitch: u8) -> Self {
        Self {
            frame,
            pitch,
            velocity: 0,
            on: false,
        }
    }
}

/// Storage is reserved up front, so a full buffer drops and counts further
/// notes instead of growing on the audio thread.
#[derive(Debug)]
pub struct NoteOutput {
    notes: Vec<BlockNote>,
    dropped: u64,
}

impl Default for NoteOutput {
    fn default() -> Self {
        Self {
            notes: Vec::with_capacity(Self::CAPACITY),
            dropped: 0,
        }
    }
}

impl NoteOutput {
    /// Room for a dense arpeggio or drum fill in one block.
    pub const CAPACITY: usize = 1024;

    pub fn clear(&mut self) {
        self.notes.clear();
    }

    /// False when the note was dropped.
    pub fn push(&mut self, note: BlockNote) -> bool {
        if self.notes.len() < Self::CAPACITY {
            self.notes.push(note);
        } else {
            self.dropped += 1;
        }
        true
    }

    pub fn notes(&self) -> &[BlockNote] {
        &self.notes
    }

    /// Notes lost to a full buffer over this output's lifetime.
    pub fn dropped(&self) -> u64 {
        self.dropped
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_notes_in_push_order_until_cleared() {
        let mut output = NoteOutput::default();
        output.push(BlockNote::on(3, 60, 100));
        output.push(BlockNote::off(9, 60));

        assert_eq!(
            output.notes(),
            &[BlockNote::on(3, 60, 100), BlockNote::off(9, 60)]
        );
        output.clear();
        assert!(output.notes().is_empty());
    }

    #[test]
    fn a_full_output_drops_and_counts_notes_without_growing() {
        let mut output = NoteOutput::default();
        for _ in 0..NoteOutput::CAPACITY + 3 {
            output.push(BlockNote::off(1, 60));
        }

        assert_eq!(output.notes().len(), NoteOutput::CAPACITY);
        assert_eq!(output.dropped(), 3);
        output.clear();
        output.push(BlockNote::off(0, 60));
        assert_eq!(output.notes(), &[BlockNote::off(0, 60)]);
        assert_eq!(output.dropped(), 3);
    }

    #[test]
    fn note_ons_leave_room_to_release_every_pitch() {
        let mut output = NoteOutput::default();
        let accepted_ons = (0..NoteOutput::CAPACITY)
            .filter(|_| output.push(BlockNote::on(0, 60, 100)))
            .count();

        assert_eq!(accepted_ons, NoteOutput::CAPACITY - 128);
        assert!((0..128).all(|pitch| output.push(BlockNote::off(1, pitch))));
        assert!(!output.push(BlockNote::off(1, 0)));
        assert_eq!(output.dropped(), 128 + 1);
    }
}
