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

#[cfg(test)]
mod tests {
    use super::note_marks;
    use iced::Rectangle;

    const AREA: Rectangle = Rectangle {
        x: 0.0,
        y: 22.0,
        width: 800.0,
        height: 44.0,
    };

    fn sixteenths(pitch: u8, count: usize) -> Vec<(u8, f64, f64, bool)> {
        (0..count)
            .map(|index| (pitch, index as f64 * 0.25, 0.2, false))
            .collect()
    }

    #[test]
    fn a_single_pitch_clip_draws_its_notes_centred_in_the_body() {
        let marks = note_marks(&sixteenths(36, 4), 0.0, AREA, 20.0);

        let centre = marks[0].rect.y + marks[0].rect.height / 2.0;
        assert!(
            (centre - (AREA.y + AREA.height / 2.0)).abs() < 0.5,
            "centre {centre}"
        );
    }

    #[test]
    fn dense_notes_on_one_pitch_merge_into_one_mark_when_zoomed_out() {
        let marks = note_marks(&sixteenths(36, 64), 0.0, AREA, 1.0);

        assert_eq!(marks.len(), 1);
        assert_eq!(marks[0].rect.x, 0.0);
        assert_eq!(marks[0].rect.width, 16.0);
    }

    #[test]
    fn notes_with_space_between_them_stay_separate_marks() {
        let marks = note_marks(&sixteenths(36, 8), 0.0, AREA, 20.0);

        assert_eq!(marks.len(), 8);
    }

    #[test]
    fn a_note_shorter_than_a_pixel_paints_one_whole_pixel() {
        let marks = note_marks(&[(60, 0.5, 0.1, false)], 10.0, AREA, 1.0);

        assert_eq!(marks[0].rect.x, 10.0);
        assert_eq!(marks[0].rect.width, 1.0);
    }

    #[test]
    fn only_the_visible_part_of_a_scrolled_clip_makes_marks() {
        let notes = [
            (60, 0.0, 1.0, false),
            (60, 49.5, 1.0, false),
            (60, 55.0, 1.0, false),
            (60, 60.0, 1.0, false),
            (60, 200.0, 1.0, false),
        ];
        let visible = Rectangle {
            width: 200.0,
            ..AREA
        };

        let marks = note_marks(&notes, -1000.0, visible, 20.0);

        let spans: Vec<_> = marks.iter().map(|m| (m.rect.x, m.rect.width)).collect();
        assert_eq!(spans, [(0.0, 10.0), (100.0, 20.0)]);
    }
}
