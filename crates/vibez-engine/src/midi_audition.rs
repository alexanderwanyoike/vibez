//! A MIDI file's notes as the Browser auditions them, placed on the samples of each audio block.

use vibez_core::midi::MidiNote;
use vibez_core::note_output::NoteOutput;

/// One pass of a MIDI file's notes, `length_beats` long, played once or
/// looped from its start.
#[derive(Debug, Clone, PartialEq)]
pub struct MidiAuditionPattern {
    notes: Vec<MidiNote>,
    length_beats: f64,
}

impl MidiAuditionPattern {
    pub fn new(notes: Vec<MidiNote>, length_beats: f64) -> Self {
        Self {
            notes,
            length_beats,
        }
    }

    pub fn notes(&self) -> &[MidiNote] {
        &self.notes
    }

    pub fn length_beats(&self) -> f64 {
        self.length_beats
    }

    /// Fills `out` with the notes due in the block starting `pos` samples
    /// after the audition began.
    pub(crate) fn collect(
        &self,
        _pos: u64,
        _frames: usize,
        _samples_per_beat: f64,
        _looped: bool,
        out: &mut NoteOutput,
    ) {
        out.clear();
    }

    /// Whether an audition `pos` samples in has played all of its notes.
    pub(crate) fn ended(&self, _pos: u64, _samples_per_beat: f64, _looped: bool) -> bool {
        false
    }
}

#[cfg(test)]
#[path = "midi_audition_tests.rs"]
mod tests;
