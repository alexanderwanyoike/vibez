use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::effect::EffectInfo;
use crate::id::{ClipId, TrackId};
use crate::midi::{InstrumentKind, TrackKind};

/// Canonical reference to media outside the project file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MediaSourceRef {
    LocalFile {
        path: PathBuf,
    },
    DropboxFile {
        path_lower: String,
        display_path: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        rev: Option<String>,
    },
}

impl MediaSourceRef {
    pub fn display_name(&self) -> String {
        match self {
            MediaSourceRef::LocalFile { path } => path
                .file_name()
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or_else(|| path.display().to_string()),
            MediaSourceRef::DropboxFile { display_path, .. } => PathBuf::from(display_path)
                .file_name()
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or_else(|| display_path.clone()),
        }
    }
}

/// Persisted state for a future native drum rack.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DrumPadState {
    pub source: Option<MediaSourceRef>,
    pub gain: f32,
    pub pan: f32,
    pub start: f32,
    pub end: f32,
    pub coarse_tune: i8,
    pub fine_tune: f32,
    pub one_shot: bool,
    pub choke_group: Option<u8>,
}

/// Persisted state for native instruments.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum InstrumentStateInfo {
    SubtractiveSynth { params: Vec<f32> },
    Sampler {
        params: Vec<f32>,
        source: Option<MediaSourceRef>,
    },
    DrumRack { pads: Vec<DrumPadState> },
}

/// Serializable track metadata shared between engine and UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackInfo {
    pub id: TrackId,
    pub name: String,
    pub gain: f32,
    pub pan: f32,
    pub mute: bool,
    pub solo: bool,
    #[serde(default)]
    pub effects: Vec<EffectInfo>,
    #[serde(default)]
    pub kind: TrackKind,
    #[serde(default)]
    pub color_index: u8,
    #[serde(default)]
    pub instrument: Option<InstrumentKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_instrument: Option<InstrumentStateInfo>,
}

impl TrackInfo {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            id: TrackId::new(),
            name: name.into(),
            gain: crate::constants::DEFAULT_TRACK_GAIN,
            pan: crate::constants::DEFAULT_TRACK_PAN,
            mute: false,
            solo: false,
            effects: Vec::new(),
            kind: TrackKind::default(),
            color_index: 0,
            instrument: None,
            native_instrument: None,
        }
    }
}

/// Serializable clip metadata shared between engine and UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClipInfo {
    pub id: ClipId,
    pub track_id: TrackId,
    pub name: String,
    /// Position on the timeline in samples.
    pub position: u64,
    /// Offset into the source audio in samples.
    pub source_offset: u64,
    /// Duration in samples.
    pub duration: u64,
    /// Canonical external media source reference.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<MediaSourceRef>,
    /// Legacy local path kept for backward compatibility with older projects.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_path: Option<PathBuf>,
    #[serde(default)]
    pub loop_enabled: bool,
    #[serde(default)]
    pub loop_start: u64,
    #[serde(default)]
    pub loop_end: u64,
}

impl ClipInfo {
    /// The end position of this clip on the timeline (position + duration).
    pub fn end_position(&self) -> u64 {
        self.position.saturating_add(self.duration)
    }

    pub fn resolved_source(&self) -> Option<&MediaSourceRef> {
        self.source.as_ref()
    }

    pub fn resolved_local_path(&self) -> Option<&PathBuf> {
        if let Some(MediaSourceRef::LocalFile { path }) = self.source.as_ref() {
            Some(path)
        } else {
            self.file_path.as_ref()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn track_info_defaults() {
        let track = TrackInfo::new("Track 1");
        assert_eq!(track.name, "Track 1");
        assert!((track.gain - 1.0).abs() < f32::EPSILON);
        assert!((track.pan - 0.5).abs() < f32::EPSILON);
        assert!(!track.mute);
        assert!(!track.solo);
    }

    #[test]
    fn clip_end_position() {
        let clip = ClipInfo {
            id: ClipId::new(),
            track_id: TrackId::new(),
            name: "test".into(),
            position: 1000,
            source_offset: 0,
            duration: 500,
            source: Some(MediaSourceRef::LocalFile {
                path: PathBuf::from("test.wav"),
            }),
            file_path: Some(PathBuf::from("test.wav")),
            loop_enabled: false,
            loop_start: 0,
            loop_end: 0,
        };
        assert_eq!(clip.end_position(), 1500);
    }

    #[test]
    fn clip_end_position_saturates() {
        let clip = ClipInfo {
            id: ClipId::new(),
            track_id: TrackId::new(),
            name: "test".into(),
            position: u64::MAX - 10,
            source_offset: 0,
            duration: 100,
            source: Some(MediaSourceRef::LocalFile {
                path: PathBuf::from("test.wav"),
            }),
            file_path: Some(PathBuf::from("test.wav")),
            loop_enabled: false,
            loop_start: 0,
            loop_end: 0,
        };
        assert_eq!(clip.end_position(), u64::MAX);
    }

    #[test]
    fn unique_ids() {
        let t1 = TrackInfo::new("A");
        let t2 = TrackInfo::new("B");
        assert_ne!(t1.id, t2.id);
    }

    #[test]
    fn serde_roundtrip_track() {
        let track = TrackInfo::new("Synth Lead");
        let json = serde_json::to_string(&track).unwrap();
        let deserialized: TrackInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(track.id, deserialized.id);
        assert_eq!(track.name, deserialized.name);
        assert!((track.gain - deserialized.gain).abs() < f32::EPSILON);
    }

    #[test]
    fn serde_roundtrip_clip() {
        let clip = ClipInfo {
            id: ClipId::new(),
            track_id: TrackId::new(),
            name: "vocal.wav".into(),
            position: 44100,
            source_offset: 1000,
            duration: 88200,
            source: Some(MediaSourceRef::LocalFile {
                path: PathBuf::from("/audio/vocal.wav"),
            }),
            file_path: Some(PathBuf::from("/audio/vocal.wav")),
            loop_enabled: false,
            loop_start: 0,
            loop_end: 0,
        };
        let json = serde_json::to_string(&clip).unwrap();
        let deserialized: ClipInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(clip.id, deserialized.id);
        assert_eq!(clip.position, deserialized.position);
        assert_eq!(clip.source_offset, deserialized.source_offset);
        assert_eq!(clip.duration, deserialized.duration);
        assert_eq!(clip.file_path, deserialized.file_path);
        assert_eq!(clip.source, deserialized.source);
    }

    #[test]
    fn clip_info_backward_compat_from_legacy_file_path() {
        let json = r#"{
            "id":0,
            "track_id":0,
            "name":"legacy.wav",
            "position":0,
            "source_offset":0,
            "duration":100,
            "file_path":"legacy.wav"
        }"#;
        let clip: ClipInfo = serde_json::from_str(json).unwrap();
        assert_eq!(clip.file_path, Some(PathBuf::from("legacy.wav")));
        assert!(clip.source.is_none());
    }
}
