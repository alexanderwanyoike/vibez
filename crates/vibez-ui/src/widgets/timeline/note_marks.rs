//! Layout of a MIDI clip's notes as the marks drawn inside its lane body.

use iced::Rectangle;

/// One filled mark inside a note clip's body.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct NoteMark {
    pub rect: Rectangle,
    /// Belongs to a loop repeat rather than the clip's first pass.
    pub repeated: bool,
}

/// Lay out `notes` (pitch, clip-local start, duration, repeated) for a clip
/// whose beat 0 is at `origin_x`. `area` is the on-screen part of the clip
/// body below its title.
pub(super) fn note_marks(
    notes: &[(u8, f64, f64, bool)],
    origin_x: f32,
    area: Rectangle,
    pixels_per_beat: f32,
) -> Vec<NoteMark> {
    let Some(min_pitch) = notes.iter().map(|note| note.0).min() else {
        return Vec::new();
    };
    let max_pitch = notes.iter().map(|note| note.0).max().unwrap_or(min_pitch);
    let pitch_range = (max_pitch - min_pitch + 1).max(12) as f32;
    let right = area.x + area.width;
    notes
        .iter()
        .map(|&(pitch, start_beat, duration_beats, repeated)| {
            let note_x = origin_x + start_beat as f32 * pixels_per_beat;
            let note_w = duration_beats as f32 * pixels_per_beat;
            let note_y_frac = (max_pitch.saturating_sub(pitch)) as f32 / pitch_range;
            NoteMark {
                rect: Rectangle {
                    x: note_x,
                    y: area.y + 2.0 + note_y_frac * (area.height - 4.0),
                    width: note_w.max(2.0).min(right - note_x).max(0.0),
                    height: ((area.height - 4.0) / pitch_range).clamp(2.0, 6.0),
                },
                repeated,
            }
        })
        .collect()
}
