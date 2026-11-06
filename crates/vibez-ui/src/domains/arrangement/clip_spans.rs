//! Where a timeline's Clips and selection sit, in beats.

use crate::state::{ArrangementSelection, ClipSpan, TimelineContent, TimelineEditorState};

impl TimelineContent {
    /// Where the Clip `selection` names sits, in beats.
    pub fn clip_span(
        &self,
        selection: ArrangementSelection,
        samples_per_beat: f64,
    ) -> Option<ClipSpan> {
        let lane = self.get(selection.track_id())?;
        match selection {
            ArrangementSelection::AudioClip { clip_id, .. } if samples_per_beat > 0.0 => lane
                .clips
                .iter()
                .find(|clip| clip.id == clip_id)
                .map(|clip| clip.span(samples_per_beat)),
            ArrangementSelection::AudioClip { .. } => None,
            ArrangementSelection::NoteClip { clip_id, .. } => lane
                .note_clips
                .iter()
                .find(|clip| clip.id == clip_id)
                .map(|clip| clip.span()),
        }
    }
}

impl TimelineEditorState {
    /// The time selection when there is one, otherwise the span from the
    /// earliest selected Clip start to the latest selected Clip end.
    pub fn selection_span_beats(&self, samples_per_beat: f64) -> Option<(f64, f64)> {
        if self.time_selection_active && self.selection_end_beats > self.selection_start_beats {
            return Some((self.selection_start_beats, self.selection_end_beats));
        }
        self.selected_clips
            .iter()
            .filter_map(|selection| self.clip_span(*selection, samples_per_beat))
            .map(|span| (span.start_beats, span.end_beats()))
            .reduce(|(start, end), (clip_start, clip_end)| {
                (start.min(clip_start), end.max(clip_end))
            })
    }
}
