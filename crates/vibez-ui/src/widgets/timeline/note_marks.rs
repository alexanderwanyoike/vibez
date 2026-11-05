//! Layout of a MIDI clip's notes as the marks drawn inside its lane body.

use iced::Rectangle;

use super::note_summary::NoteSummary;

/// Breathing room between the marks and the top and bottom of the body.
const PAD: f32 = 2.0;
const MIN_MARK_HEIGHT: f32 = 2.0;
/// The tallest a mark grows in a default height lane.
const MAX_MARK_HEIGHT: f32 = 6.0;
/// In a taller lane a mark may fill this share of the body, so notes grow
/// with the lane while a clip of one pitch still reads as a line of notes.
const MAX_MARK_SHARE: f32 = 0.1;

/// Lay out a clip's note summary for a clip whose beat 0 is at `origin_x`.
/// `area` is the on-screen part of the clip body below its title.
///
/// The clip's pitch range is fitted to the body, so one mark row per pitch.
/// Marks on one row that would touch on screen merge into one, which keeps
/// the mark count bounded by the visible width rather than the note count.
pub(super) fn note_marks(
    summary: &NoteSummary,
    origin_x: f32,
    area: Rectangle,
    pixels_per_beat: f32,
) -> Vec<Rectangle> {
    let Some((lowest, highest)) = summary.pitch_range() else {
        return Vec::new();
    };
    let rows = f32::from(highest - lowest) + 1.0;
    let inner = (area.height - 2.0 * PAD).max(0.0);
    let max_height = (inner * MAX_MARK_SHARE).max(MAX_MARK_HEIGHT);
    let height = (inner / rows).clamp(MIN_MARK_HEIGHT, max_height).min(inner);
    // A range too wide for MIN_MARK_HEIGHT rows lets neighbouring rows
    // overlap rather than spill out of the body.
    let step = if rows > 1.0 {
        ((inner - height) / (rows - 1.0)).min(height)
    } else {
        0.0
    };
    let top = area.y + PAD + (inner - (step * (rows - 1.0) + height)) / 2.0;
    let right = area.x + area.width;

    let mut marks: Vec<(u8, f32, f32)> = Vec::new();
    for span in summary.spans() {
        let start_x = origin_x + span.start as f32 * pixels_per_beat;
        let end_x = origin_x + span.end as f32 * pixels_per_beat;
        // Whole pixels, at least one wide, so a note narrower than a
        // pixel paints a solid column instead of a faint antialiased one.
        let x0 = start_x.floor().max(area.x);
        let x1 = end_x.ceil().max(start_x.floor() + 1.0).min(right);
        if x1 <= x0 {
            continue;
        }
        match marks.last_mut() {
            Some(kept) if kept.0 == span.pitch && x0 <= kept.2 => kept.2 = kept.2.max(x1),
            _ => marks.push((span.pitch, x0, x1)),
        }
    }
    marks
        .into_iter()
        .map(|(pitch, x0, x1)| Rectangle {
            x: x0,
            y: top + f32::from(highest - pitch) * step,
            width: x1 - x0,
            height,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::note_marks;
    use crate::widgets::timeline::NoteSummary;
    use iced::Rectangle;

    const AREA: Rectangle = Rectangle {
        x: 0.0,
        y: 22.0,
        width: 800.0,
        height: 44.0,
    };

    fn sixteenths(pitch: u8, count: usize) -> Vec<(u8, f64, f64)> {
        (0..count)
            .map(|index| (pitch, index as f64 * 0.25, 0.2))
            .collect()
    }

    #[test]
    fn a_single_pitch_clip_draws_its_notes_centred_in_the_body() {
        let marks = note_marks(&NoteSummary::new(sixteenths(36, 4), 20.0), 0.0, AREA, 20.0);

        let centre = marks[0].y + marks[0].height / 2.0;
        assert!(
            (centre - (AREA.y + AREA.height / 2.0)).abs() < 0.5,
            "centre {centre}"
        );
    }

    #[test]
    fn a_three_times_taller_lane_draws_its_notes_taller() {
        let tall = Rectangle {
            height: 3.0 * AREA.height,
            ..AREA
        };
        let bassline = NoteSummary::new((36..48).map(|pitch| (pitch, 0.0, 1.0)), 20.0);

        let short = note_marks(&bassline, 0.0, AREA, 20.0)[0].height;
        let taller = note_marks(&bassline, 0.0, tall, 20.0)[0].height;

        assert!(taller >= 2.5 * short, "{short} px became {taller} px");
    }

    #[test]
    fn dense_notes_on_one_pitch_merge_into_one_mark_when_zoomed_out() {
        let marks = note_marks(&NoteSummary::new(sixteenths(36, 64), 1.0), 0.0, AREA, 1.0);

        assert_eq!(marks.len(), 1);
        assert_eq!(marks[0].x, 0.0);
        assert_eq!(marks[0].width, 16.0);
    }

    #[test]
    fn notes_with_space_between_them_stay_separate_marks() {
        let marks = note_marks(&NoteSummary::new(sixteenths(36, 8), 20.0), 0.0, AREA, 20.0);

        assert_eq!(marks.len(), 8);
    }

    #[test]
    fn a_note_shorter_than_a_pixel_paints_one_whole_pixel() {
        let marks = note_marks(&NoteSummary::new([(60, 0.5, 0.1)], 1.0), 10.0, AREA, 1.0);

        assert_eq!(marks[0].x, 10.0);
        assert_eq!(marks[0].width, 1.0);
    }

    #[test]
    fn only_the_visible_part_of_a_scrolled_clip_makes_marks() {
        let notes = [
            (60, 0.0, 1.0),
            (60, 49.5, 1.0),
            (60, 55.0, 1.0),
            (60, 60.0, 1.0),
            (60, 200.0, 1.0),
        ];
        let visible = Rectangle {
            width: 200.0,
            ..AREA
        };

        let marks = note_marks(&NoteSummary::new(notes, 20.0), -1000.0, visible, 20.0);

        let spans: Vec<_> = marks.iter().map(|m| (m.x, m.width)).collect();
        assert_eq!(spans, [(0.0, 10.0), (100.0, 20.0)]);
    }
}
