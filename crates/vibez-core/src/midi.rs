use serde::{Deserialize, Serialize};

use crate::clip_timeline::BeatClipTimeline;
use crate::id::{ClipId, TrackId};
use crate::perform::GrooveGrid;

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
    /// Initial playback position. Older projects start where their loop starts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_marker_beats: Option<f64>,
    #[serde(default)]
    pub loop_enabled: bool,
    #[serde(default)]
    pub loop_start_beats: f64,
    #[serde(default)]
    pub loop_end_beats: f64,
    #[serde(default)]
    pub groove_grid: GrooveGrid,
}

impl NoteClipInfo {
    /// Resolve the persisted Start marker into the playable clip window.
    /// Legacy projects begin at Loop Start, matching pre-marker playback.
    pub fn resolved_start_marker_beats(&self) -> f64 {
        let start = self.start_marker_beats.unwrap_or(self.loop_start_beats);
        BeatClipTimeline::new(
            start,
            self.loop_start_beats,
            self.loop_end_beats,
            self.duration_beats,
            self.loop_enabled,
        )
        .clamp_start(start, 0.0, self.duration_beats)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum InstrumentKind {
    SubtractiveSynth,
    Sampler,
    DrumRack,
}

impl InstrumentKind {
    pub fn name(self) -> &'static str {
        match self {
            InstrumentKind::SubtractiveSynth => "Subtractive Synth",
            InstrumentKind::Sampler => "Sampler",
            InstrumentKind::DrumRack => "Drum Rack",
        }
    }

    pub fn all() -> &'static [InstrumentKind] {
        &[
            InstrumentKind::SubtractiveSynth,
            InstrumentKind::Sampler,
            InstrumentKind::DrumRack,
        ]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum TrackKind {
    #[default]
    Audio,
    Instrument(InstrumentKind),
    Midi,
}

impl TrackKind {
    /// Returns true for both legacy Instrument and new Midi tracks.
    pub fn is_midi(&self) -> bool {
        matches!(self, TrackKind::Instrument(_) | TrackKind::Midi)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::perform::GrooveGrid;

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
            start_marker_beats: Some(1.0),
            loop_enabled: false,
            loop_start_beats: 0.0,
            loop_end_beats: 0.0,
            groove_grid: GrooveGrid::Sixteenth,
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
        assert_eq!(loaded.start_marker_beats, Some(1.0));
        assert_eq!(loaded.resolved_start_marker_beats(), 1.0);
        assert_eq!(loaded.groove_grid, GrooveGrid::Sixteenth);
    }

    #[test]
    fn legacy_note_clip_defaults_the_groove_grid_to_off() {
        let clip = NoteClipInfo {
            id: ClipId::new(),
            track_id: TrackId::new(),
            name: "Legacy".into(),
            position_beats: 0.0,
            duration_beats: 4.0,
            notes: Vec::new(),
            start_marker_beats: None,
            loop_enabled: false,
            loop_start_beats: 0.0,
            loop_end_beats: 0.0,
            groove_grid: GrooveGrid::Off,
        };
        let mut json = serde_json::to_value(clip).unwrap();
        json.as_object_mut().unwrap().remove("groove_grid");
        json.as_object_mut().unwrap().remove("start_marker_beats");
        let loaded: NoteClipInfo = serde_json::from_value(json).unwrap();
        assert_eq!(loaded.groove_grid, GrooveGrid::Off);
        assert_eq!(loaded.start_marker_beats, None);
        assert_eq!(loaded.resolved_start_marker_beats(), 0.0);
    }

    #[test]
    fn resolved_start_marker_is_clamped_before_loop_end() {
        let clip = NoteClipInfo {
            id: ClipId::new(),
            track_id: TrackId::new(),
            name: "Loop".into(),
            position_beats: 0.0,
            duration_beats: 8.0,
            notes: Vec::new(),
            start_marker_beats: Some(7.0),
            loop_enabled: true,
            loop_start_beats: 1.0,
            loop_end_beats: 4.0,
            groove_grid: GrooveGrid::Off,
        };
        assert_eq!(clip.resolved_start_marker_beats(), 3.99);
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

    #[test]
    fn track_kind_midi_serde_roundtrip() {
        let kind = TrackKind::Midi;
        let json = serde_json::to_string(&kind).unwrap();
        let loaded: TrackKind = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded, kind);
    }

    #[test]
    fn track_kind_is_midi() {
        assert!(!TrackKind::Audio.is_midi());
        assert!(TrackKind::Instrument(InstrumentKind::SubtractiveSynth).is_midi());
        assert!(TrackKind::Midi.is_midi());
    }

    #[test]
    fn instrument_kind_all() {
        let all = InstrumentKind::all();
        assert_eq!(all.len(), 3);
        assert_eq!(all[0], InstrumentKind::SubtractiveSynth);
        assert_eq!(all[1], InstrumentKind::Sampler);
        assert_eq!(all[2], InstrumentKind::DrumRack);
    }
}
