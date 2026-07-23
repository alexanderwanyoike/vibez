//! Shared event-read boundary for non-destructive MIDI clip Groove.

use super::*;
use vibez_core::midi::MidiNote;
use vibez_core::perform::GrooveGrid;

#[inline]
pub(super) fn crossed_beat(previous: f64, current: f64, event: f64) -> bool {
    previous < event && event <= current
}

#[inline]
pub(super) fn mapped_note_start(clip: &EngineNoteClip, note: &MidiNote, swing: SwingAmount) -> f64 {
    clip.groove_grid.map_beat(note.start_beat, swing)
}

pub(super) fn mapped_note_end(clip: &EngineNoteClip, note_index: usize, swing: SwingAmount) -> f64 {
    let note = &clip.notes[note_index];
    let mapped_end = clip.groove_grid.map_beat(note.end_beat(), swing);
    if clip.groove_grid == GrooveGrid::Off {
        return mapped_end;
    }
    let next_same_pitch = clip
        .next_same_pitch_start_beat(note_index)
        .map(|start_beat| clip.groove_grid.map_beat(start_beat, swing));
    next_same_pitch.map_or(mapped_end, |next_start| mapped_end.min(next_start))
}

impl EngineTrack {
    pub(super) fn reset_groove_latches(&self) {
        for clip in self
            .playback_source
            .note_clips
            .iter()
            .chain(self.section_playback_source.note_clips.iter())
        {
            clip.reset_groove_latch();
        }
    }
}
