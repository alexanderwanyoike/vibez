//! Standard MIDI Files read into beat-timed notes, one entry per track that plays.

use std::path::Path;

use crate::midi::MidiNote;

/// Extensions a file must carry to be offered as a MIDI file.
pub const MIDI_FILE_EXTENSIONS: &[&str] = &["mid", "midi"];

/// The General MIDI percussion channel (channel 10), zero-based.
const DRUM_CHANNEL: u8 = 9;

/// A MIDI file's notes, timed in beats from its own quarter-note resolution.
#[derive(Debug, Clone, PartialEq)]
pub struct MidiFile {
    /// The tracks that play notes, in file order.
    pub tracks: Vec<MidiFileTrack>,
    /// Where the file ends: its last End of Track, never before a note ends.
    pub length_beats: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MidiFileTrack {
    pub name: Option<String>,
    /// Sorted by start, then pitch.
    pub notes: Vec<MidiNote>,
    /// Every note sits on the General MIDI percussion channel.
    pub on_drum_channel: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MidiFileError {
    /// The bytes are not a Standard MIDI File.
    Malformed(String),
    /// Timed in SMPTE frames rather than beats.
    TimecodeTiming,
    /// A type 2 file, whose tracks are independent patterns.
    SequentialPatterns,
    NoNotes,
}

impl std::fmt::Display for MidiFileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Malformed(reason) => write!(f, "not a readable MIDI file ({reason})"),
            Self::TimecodeTiming => f.write_str("SMPTE-timed MIDI files are not supported"),
            Self::SequentialPatterns => {
                f.write_str("type 2 MIDI files of separate patterns are not supported")
            }
            Self::NoNotes => f.write_str("the MIDI file has no notes"),
        }
    }
}

impl std::error::Error for MidiFileError {}

pub fn is_midi_file_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            MIDI_FILE_EXTENSIONS
                .iter()
                .any(|candidate| candidate.eq_ignore_ascii_case(extension))
        })
}

impl MidiFile {
    pub fn read(bytes: &[u8]) -> Result<Self, MidiFileError> {
        let smf = midly::Smf::parse(bytes)
            .map_err(|error| MidiFileError::Malformed(error.to_string()))?;
        let ticks_per_beat = match smf.header.timing {
            midly::Timing::Metrical(ticks) => f64::from(ticks.as_int().max(1)),
            midly::Timing::Timecode(..) => return Err(MidiFileError::TimecodeTiming),
        };
        if smf.header.format == midly::Format::Sequential {
            return Err(MidiFileError::SequentialPatterns);
        }
        let beats = |tick: u64| tick as f64 / ticks_per_beat;
        let mut end_tick = 0;
        let mut tracks = Vec::new();
        for events in &smf.tracks {
            let track = TrackReader::read(events);
            end_tick = end_tick.max(track.end_tick);
            if track.notes.is_empty() {
                continue;
            }
            let mut notes: Vec<_> = track
                .notes
                .iter()
                .map(|note| MidiNote {
                    pitch: note.pitch,
                    velocity: note.velocity,
                    start_beat: beats(note.start),
                    // A note ended on its own tick still sounds for one.
                    duration_beats: beats((note.end - note.start).max(1)),
                })
                .collect();
            MidiNote::sort(&mut notes);
            tracks.push(MidiFileTrack {
                name: track.name,
                on_drum_channel: track.notes.iter().all(|note| note.channel == DRUM_CHANNEL),
                notes,
            });
        }
        if tracks.is_empty() {
            return Err(MidiFileError::NoNotes);
        }
        let last_note_end = tracks
            .iter()
            .flat_map(|track| &track.notes)
            .map(MidiNote::end_beat)
            .fold(0.0, f64::max);
        Ok(Self {
            tracks,
            length_beats: beats(end_tick).max(last_note_end),
        })
    }

    /// Every track's notes as one sorted list, for a single Clip.
    pub fn merged_notes(&self) -> Vec<MidiNote> {
        let mut notes: Vec<_> = self
            .tracks
            .iter()
            .flat_map(|track| track.notes.iter().copied())
            .collect();
        MidiNote::sort(&mut notes);
        notes
    }

    /// Every note sits on the General MIDI percussion channel.
    pub fn on_drum_channel(&self) -> bool {
        self.tracks.iter().all(|track| track.on_drum_channel)
    }
}

struct TickNote {
    channel: u8,
    pitch: u8,
    velocity: u8,
    start: u64,
    end: u64,
}

/// One track's notes in ticks, paired from its note-on and note-off events.
struct TrackReader {
    name: Option<String>,
    notes: Vec<TickNote>,
    end_tick: u64,
}

impl TrackReader {
    fn read(events: &[midly::TrackEvent<'_>]) -> Self {
        let mut reader = Self {
            name: None,
            notes: Vec::new(),
            end_tick: 0,
        };
        // Indices into `notes` still sounding, oldest first per channel and
        // key, so overlapping notes on one key end first in, first out.
        let mut sounding: Vec<usize> = Vec::new();
        let mut tick = 0;
        for event in events {
            tick += u64::from(event.delta.as_int());
            match event.kind {
                midly::TrackEventKind::Midi { channel, message } => {
                    let channel = channel.as_int();
                    match message {
                        midly::MidiMessage::NoteOn { key, vel } if vel.as_int() > 0 => {
                            sounding.push(reader.notes.len());
                            reader.notes.push(TickNote {
                                channel,
                                pitch: key.as_int(),
                                velocity: vel.as_int(),
                                start: tick,
                                end: tick,
                            });
                        }
                        midly::MidiMessage::NoteOn { key, .. }
                        | midly::MidiMessage::NoteOff { key, .. } => {
                            let notes = &mut reader.notes;
                            if let Some(slot) = sounding.iter().position(|&index| {
                                notes[index].channel == channel
                                    && notes[index].pitch == key.as_int()
                            }) {
                                notes[sounding.remove(slot)].end = tick;
                            }
                        }
                        _ => {}
                    }
                }
                // midly reads on to the end of the chunk; whatever follows
                // End of Track is not part of the track.
                midly::TrackEventKind::Meta(midly::MetaMessage::EndOfTrack) => break,
                midly::TrackEventKind::Meta(midly::MetaMessage::TrackName(name)) => {
                    reader.name = Some(String::from_utf8_lossy(name).trim().to_string())
                        .filter(|name| !name.is_empty());
                }
                _ => {}
            }
        }
        reader.end_tick = tick;
        for index in sounding {
            reader.notes[index].end = tick;
        }
        reader
    }
}

#[cfg(test)]
#[path = "midi_file_tests.rs"]
mod tests;
