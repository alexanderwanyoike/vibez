//! A note clip's notes over every loop pass, merged where they would touch
//! on screen at the lane's zoom bucket.

/// One run of notes on one pitch, in clip-local beats.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NoteSpan {
    pub pitch: u8,
    pub start: f64,
    pub end: f64,
}

/// The note spans of one clip, sorted by pitch and then start.
#[derive(Debug, Default, PartialEq)]
pub struct NoteSummary {
    spans: Vec<NoteSpan>,
}

/// Zooms within one doubling of pixels per beat share a summary.
pub(super) fn zoom_bucket(pixels_per_beat: f32) -> i32 {
    pixels_per_beat.max(f32::MIN_POSITIVE).log2().ceil() as i32
}

impl NoteSummary {
    /// Summarise `notes` (pitch, clip-local start, duration) for a lane at
    /// `pixels_per_beat`.
    pub fn new(notes: impl IntoIterator<Item = (u8, f64, f64)>, pixels_per_beat: f32) -> Self {
        // Under a pixel apart at the bucket's closest zoom is under a pixel
        // apart at every zoom in it, and such notes always draw as one mark.
        let merge_gap = (-f64::from(zoom_bucket(pixels_per_beat))).exp2();
        let mut spans: Vec<NoteSpan> = notes
            .into_iter()
            .map(|(pitch, start, duration)| NoteSpan {
                pitch,
                start,
                end: start + duration,
            })
            .collect();
        spans.sort_unstable_by(|a, b| a.pitch.cmp(&b.pitch).then(a.start.total_cmp(&b.start)));
        spans.dedup_by(|next, kept| {
            let merge = next.pitch == kept.pitch && next.start - kept.end < merge_gap;
            if merge {
                kept.end = kept.end.max(next.end);
            }
            merge
        });
        Self { spans }
    }

    pub fn spans(&self) -> &[NoteSpan] {
        &self.spans
    }

    /// Lowest and highest pitch played.
    pub(super) fn pitch_range(&self) -> Option<(u8, u8)> {
        Some((self.spans.first()?.pitch, self.spans.last()?.pitch))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span(pitch: u8, start: f64, end: f64) -> NoteSpan {
        NoteSpan { pitch, start, end }
    }

    #[test]
    fn notes_closer_than_a_pixel_merge_and_wider_gaps_stay_apart() {
        // Four pixels per beat: a quarter of a beat is a pixel.
        let notes = [
            (60, 0.0, 1.0),
            (60, 1.2, 1.0),
            (60, 2.5, 1.0),
            (62, 0.5, 0.5),
        ];

        let summary = NoteSummary::new(notes, 4.0);

        assert_eq!(
            summary.spans(),
            [span(60, 0.0, 2.2), span(60, 2.5, 3.5), span(62, 0.5, 1.0)]
        );
        assert_eq!(summary.pitch_range(), Some((60, 62)));
    }

    #[test]
    fn zooms_within_one_doubling_share_a_bucket() {
        assert_eq!(zoom_bucket(3.0), zoom_bucket(4.0));
        assert_ne!(zoom_bucket(4.0), zoom_bucket(4.5));
    }
}
