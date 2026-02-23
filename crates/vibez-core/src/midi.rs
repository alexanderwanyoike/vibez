use serde::{Deserialize, Serialize};

use crate::id::{ClipId, TrackId};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct MidiNote {
    pub pitch: u8,
    pub velocity: u8,
    pub start_beat: f64,
    pub duration_beats: f64,
}

impl MidiNote {
    pub fn frequency(&self) -> f64 {
        440.0 * 2.0_f64.powf((self.pitch as f64 - 69.0) / 12.0)
    }

    pub fn end_beat(&self) -> f64 {
        self.start_beat + self.duration_beats
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NoteClipInfo {
    pub id: ClipId,
    pub track_id: TrackId,
    pub name: String,
    pub position_beats: f64,
    pub duration_beats: f64,
    pub notes: Vec<MidiNote>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum InstrumentKind {
    SubtractiveSynth,
}

impl InstrumentKind {
    pub fn name(self) -> &'static str {
        match self {
            InstrumentKind::SubtractiveSynth => "Subtractive Synth",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum TrackKind {
    #[default]
    Audio,
    Instrument(InstrumentKind),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn midi_note_frequency() {
        let note = MidiNote {
            pitch: 69,
            velocity: 100,
            start_beat: 0.0,
            duration_beats: 1.0,
        };
        assert!((note.frequency() - 440.0).abs() < 0.01);
    }

    #[test]
    fn midi_note_middle_c() {
        let note = MidiNote {
            pitch: 60,
            velocity: 100,
            start_beat: 0.0,
            duration_beats: 1.0,
        };
        assert!((note.frequency() - 261.63).abs() < 0.1);
    }

    #[test]
    fn midi_note_end_beat() {
        let note = MidiNote {
            pitch: 60,
            velocity: 100,
            start_beat: 2.0,
            duration_beats: 1.5,
        };
        assert!((note.end_beat() - 3.5).abs() < f64::EPSILON);
    }

    #[test]
    fn track_kind_default_is_audio() {
        assert_eq!(TrackKind::default(), TrackKind::Audio);
    }

    #[test]
    fn note_clip_serde_roundtrip() {
        let clip = NoteClipInfo {
            id: ClipId::new(),
            track_id: TrackId::new(),
            name: "Pattern 1".into(),
            position_beats: 0.0,
            duration_beats: 4.0,
            notes: vec![
                MidiNote {
                    pitch: 60,
                    velocity: 100,
                    start_beat: 0.0,
                    duration_beats: 1.0,
                },
                MidiNote {
                    pitch: 64,
                    velocity: 80,
                    start_beat: 1.0,
                    duration_beats: 0.5,
                },
            ],
        };
        let json = serde_json::to_string(&clip).unwrap();
        let loaded: NoteClipInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded.name, "Pattern 1");
        assert_eq!(loaded.notes.len(), 2);
        assert_eq!(loaded.notes[0].pitch, 60);
    }

    #[test]
    fn instrument_kind_name() {
        assert_eq!(InstrumentKind::SubtractiveSynth.name(), "Subtractive Synth");
    }

    #[test]
    fn track_kind_serde_roundtrip() {
        let kind = TrackKind::Instrument(InstrumentKind::SubtractiveSynth);
        let json = serde_json::to_string(&kind).unwrap();
        let loaded: TrackKind = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded, kind);
    }
}
