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

impl NoteOutput {
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            notes: Vec::with_capacity(capacity),
            dropped: 0,
        }
    }

    pub fn clear(&mut self) {}

    pub fn push(&mut self, _note: BlockNote) {}

    pub fn notes(&self) -> &[BlockNote] {
        &[]
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
        let mut output = NoteOutput::with_capacity(4);
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
        let mut output = NoteOutput::with_capacity(2);
        for frame in 0..5 {
            output.push(BlockNote::on(frame, 60, 100));
        }

        assert_eq!(output.notes().len(), 2);
        assert_eq!(output.notes()[1].frame, 1);
        assert_eq!(output.dropped(), 3);
        output.clear();
        output.push(BlockNote::off(0, 60));
        assert_eq!(output.notes(), &[BlockNote::off(0, 60)]);
        assert_eq!(output.dropped(), 3);
    }
}
