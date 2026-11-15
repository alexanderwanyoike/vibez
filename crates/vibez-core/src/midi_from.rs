//! A MIDI track's choice of another track whose played notes it also plays, as Ableton's MIDI From.

use serde::{Deserialize, Serialize};

use crate::id::TrackId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MidiFrom {
    source: TrackId,
}

impl MidiFrom {
    /// `None` when a track would take its own notes.
    pub fn new(receiver: TrackId, source: TrackId) -> Option<Self> {
        (receiver != source).then_some(Self { source })
    }

    pub fn source(self) -> TrackId {
        self.source
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_track_cannot_take_its_own_notes() {
        let (track, other) = (TrackId::new(), TrackId::new());

        assert_eq!(MidiFrom::new(track, track), None);
        assert_eq!(
            MidiFrom::new(track, other).map(MidiFrom::source),
            Some(other)
        );
    }
}
