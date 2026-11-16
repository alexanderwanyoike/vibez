//! Where a small note thumbnail draws each pitch, centred on the notes' range with headroom, and how tall and strong each note is.

/// Fewer pitches than this still spread over an octave, so a one-note
/// rhythm does not fill the thumbnail.
const MIN_SPAN: f32 = 12.0;
/// Room left above and below the highest and lowest notes.
const HEADROOM: f32 = 5.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NoteRows {
    middle: f32,
    span: f32,
}

impl NoteRows {
    pub fn of(pitches: impl IntoIterator<Item = u8>) -> Self {
        let (low, high) = pitches
            .into_iter()
            .fold(None, |range: Option<(u8, u8)>, pitch| {
                Some(range.map_or((pitch, pitch), |(low, high)| {
                    (low.min(pitch), high.max(pitch))
                }))
            })
            .unwrap_or((60, 60));
        let (low, high) = (f32::from(low), f32::from(high));
        Self {
            middle: (low + high) / 2.0,
            span: (high - low + HEADROOM).max(MIN_SPAN),
        }
    }

    /// How far down the thumbnail `pitch` sits, from 0 at the top to 1 at
    /// the bottom.
    pub fn depth(&self, pitch: u8) -> f32 {
        0.5 - (f32::from(pitch) - self.middle) / self.span
    }

    /// A note's height in a thumbnail `height` tall.
    pub fn note_height(&self, height: f32) -> f32 {
        (height / self.span).clamp(2.0, 4.0)
    }
}

/// Louder notes are drawn stronger.
pub fn note_alpha(velocity: u8) -> f32 {
    0.55 + f32::from(velocity) / 127.0 * 0.45
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_range_is_centred_with_higher_pitches_nearer_the_top() {
        let rows = NoteRows::of([36, 48]);

        assert_eq!(rows.depth(42), 0.5);
        assert!(rows.depth(48) < rows.depth(36));
        assert!(rows.depth(48) > 0.0 && rows.depth(36) < 1.0);
    }

    #[test]
    fn a_narrow_range_spreads_over_an_octave_and_notes_stay_drawable() {
        let rows = NoteRows::of([40]);

        assert_eq!(rows.depth(40), 0.5);
        assert_eq!(rows.depth(46), 0.0);
        assert_eq!(rows.note_height(12.0), 2.0);
        assert_eq!(rows.note_height(100.0), 4.0);
    }

    #[test]
    fn louder_notes_are_drawn_stronger() {
        assert!(note_alpha(127) > note_alpha(40));
        assert_eq!(note_alpha(127), 1.0);
    }
}
