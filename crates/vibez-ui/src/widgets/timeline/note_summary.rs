//! A note clip's notes as its lane draws them: every loop pass, merged
//! where they would touch on screen, laid out once per clip and zoom bucket.

use std::cell::RefCell;
use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::Arc;

use vibez_core::id::ClipId;

use crate::state::UiNoteClip;

/// Summaries kept before the cache starts over.
const CACHE_SUMMARIES: usize = 4096;

struct CachedSummary {
    /// The notes and bounds the summary was laid out from.
    fingerprint: u64,
    summary: Arc<NoteSummary>,
}

thread_local! {
    static SUMMARIES: RefCell<HashMap<(ClipId, i32), CachedSummary>> =
        RefCell::new(HashMap::new());
}

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

/// The summary of `clip` for a lane at `pixels_per_beat`, laid out again
/// only when the clip's notes or bounds change or the zoom leaves its
/// bucket.
pub(super) fn note_clip_summary(clip: &UiNoteClip, pixels_per_beat: f32) -> Arc<NoteSummary> {
    let key = (clip.id, zoom_bucket(pixels_per_beat));
    let fingerprint = fingerprint(clip);
    if let Some(summary) = SUMMARIES.with(|summaries| {
        summaries
            .borrow()
            .get(&key)
            .filter(|cached| cached.fingerprint == fingerprint)
            .map(|cached| Arc::clone(&cached.summary))
    }) {
        return summary;
    }

    let summary = Arc::new(NoteSummary::new(
        clip.notes.iter().flat_map(|note| {
            clip.note_occurrences(note.start_beat)
                .into_iter()
                .map(move |occurrence| (note.pitch, occurrence, note.duration_beats))
        }),
        pixels_per_beat,
    ));
    SUMMARIES.with(|summaries| {
        let mut summaries = summaries.borrow_mut();
        if summaries.len() >= CACHE_SUMMARIES {
            summaries.clear();
        }
        summaries.insert(
            key,
            CachedSummary {
                fingerprint,
                summary: Arc::clone(&summary),
            },
        );
    });
    summary
}

fn fingerprint(clip: &UiNoteClip) -> u64 {
    let mut hasher = DefaultHasher::new();
    for note in &clip.notes {
        (
            note.pitch,
            note.start_beat.to_bits(),
            note.duration_beats.to_bits(),
        )
            .hash(&mut hasher);
    }
    (
        clip.start_marker_beats.to_bits(),
        clip.loop_enabled,
        clip.loop_start_beats.to_bits(),
        clip.loop_end_beats.to_bits(),
        clip.duration_beats.to_bits(),
    )
        .hash(&mut hasher);
    hasher.finish()
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
