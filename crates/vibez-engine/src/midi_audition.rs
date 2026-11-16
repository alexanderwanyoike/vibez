//! A MIDI file's notes as the Browser auditions them, placed on the samples of each audio block.

use vibez_core::midi::MidiNote;
use vibez_core::note_output::{BlockNote, NoteOutput};

use crate::mixer::frame_for_beat;

/// One pass of a MIDI file's notes, `length_beats` long, played once or
/// looped from its start.
#[derive(Debug, Clone, PartialEq)]
pub struct MidiAuditionPattern {
    notes: Vec<MidiNote>,
    length_beats: f64,
}

impl MidiAuditionPattern {
    /// Notes starting at or after `length_beats` never play, so they are
    /// left out.
    pub fn new(mut notes: Vec<MidiNote>, length_beats: f64) -> Self {
        notes.retain(|note| note.start_beat < length_beats);
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
    /// after the audition began. A note held past the length ends there.
    /// Note-offs come first, so a note ending where the next pass starts it
    /// again is released before it retriggers.
    pub(crate) fn collect(
        &self,
        pos: u64,
        frames: usize,
        samples_per_beat: f64,
        looped: bool,
        out: &mut NoteOutput,
    ) {
        out.clear();
        let length = self.length_beats;
        if frames == 0 || samples_per_beat <= 0.0 || length <= 0.0 {
            return;
        }
        let first_beat = pos as f64 / samples_per_beat;
        let last_beat = pos.saturating_add(frames as u64) as f64 / samples_per_beat;
        // Every pass that could put `beat` inside the block; the exact
        // sample test below keeps only those that do. Unlike
        // `BeatClipTimeline::occurrences_of`, which walks every pass from the
        // first, this costs the same however long a loop has been playing.
        let passes = |beat: f64| {
            if looped {
                let first = ((first_beat - beat) / length).floor().max(0.0) as u64;
                let last = ((last_beat - beat) / length).ceil().max(0.0) as u64;
                first..=last
            } else {
                0..=0
            }
        };
        let frame_of = |beat: f64, pass: u64| {
            frame_for_beat(beat + pass as f64 * length, pos, frames, samples_per_beat)
        };
        // A note shorter than a sample starts and ends on one frame; its
        // note-off follows its note-on there, or it would never be released.
        let offs = |starting_too: bool, out: &mut NoteOutput| {
            for note in &self.notes {
                let end = note.end_beat().min(length);
                for pass in passes(end) {
                    let Some(frame) = frame_of(end, pass) else {
                        continue;
                    };
                    if (frame_of(note.start_beat, pass) == Some(frame)) == starting_too {
                        out.push(BlockNote::off(frame, note.pitch));
                    }
                }
            }
        };
        offs(false, out);
        for note in &self.notes {
            for frame in passes(note.start_beat).filter_map(|pass| frame_of(note.start_beat, pass))
            {
                out.push(BlockNote::on(frame, note.pitch, note.velocity));
            }
        }
        offs(true, out);
    }

    /// Whether an audition `pos` samples in has played all of its notes,
    /// the note-offs on its last sample included.
    pub(crate) fn ended(&self, pos: u64, samples_per_beat: f64, looped: bool) -> bool {
        !looped && pos as f64 > (self.length_beats * samples_per_beat).ceil()
    }
}

#[cfg(test)]
#[path = "midi_audition_tests.rs"]
mod tests;
