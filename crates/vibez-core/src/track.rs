use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::effect::EffectInfo;
use crate::id::{ClipId, TrackId};
use crate::midi::{InstrumentKind, TrackKind};
use crate::perform::SwingOffset;

/// Credential-free identity retained after external media becomes project-owned.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MediaProvenance {
    Local {
        source_path: PathBuf,
    },
    Remote {
        provider: String,
        connection_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        connection_name: Option<String>,
        source_id: String,
        source_path: String,
        revision: Option<String>,
    },
}

impl MediaProvenance {
    pub fn display_label(&self) -> String {
        match self {
            Self::Local { source_path } => source_path.display().to_string(),
            Self::Remote {
                connection_id,
                connection_name,
                source_path,
                ..
            } => format!(
                "{} · {source_path}",
                connection_name.as_deref().unwrap_or(connection_id)
            ),
        }
    }
}

/// Canonical reference to media outside the project file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MediaSourceRef {
    LocalFile {
        path: PathBuf,
    },
    /// Project-owned bytes copied to a staging area before the first save.
    /// This reference must never survive in a committed project document.
    StagedProjectMedia {
        id: String,
        file_name: String,
        staging_path: PathBuf,
        source_path: PathBuf,
    },
    /// Remote bytes copied out of disposable Media Cache into managed staging.
    /// Provider identity is descriptive provenance, never a playback path.
    StagedRemoteProjectMedia {
        id: String,
        file_name: String,
        staging_path: PathBuf,
        provenance: Box<MediaProvenance>,
    },
    /// Playback-critical media embedded in a Project Format V1 container.
    ProjectMedia {
        id: String,
        file_name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provenance: Option<Box<MediaProvenance>>,
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
            MediaSourceRef::StagedProjectMedia { file_name, .. }
            | MediaSourceRef::StagedRemoteProjectMedia { file_name, .. }
            | MediaSourceRef::ProjectMedia { file_name, .. } => file_name.clone(),
            MediaSourceRef::DropboxFile { display_path, .. } => PathBuf::from(display_path)
                .file_name()
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or_else(|| display_path.clone()),
        }
    }

    pub fn provenance(&self) -> Option<&MediaProvenance> {
        match self {
            Self::StagedProjectMedia { .. } => None,
            Self::StagedRemoteProjectMedia { provenance, .. } => Some(provenance.as_ref()),
            Self::ProjectMedia { provenance, .. } => provenance.as_deref(),
            Self::LocalFile { .. } | Self::DropboxFile { .. } => None,
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
    SubtractiveSynth {
        params: Vec<f32>,
    },
    Sampler {
        params: Vec<f32>,
        source: Option<MediaSourceRef>,
    },
    DrumRack {
        pads: Vec<DrumPadState>,
    },
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
    /// Optional adjustment combined with the Project Swing amount for
    /// generated events on this Project Track.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub swing_offset: Option<SwingOffset>,
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
    /// Third-party plugin instrument on this track, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plugin_instrument: Option<crate::effect::PluginDeviceInfo>,
    /// Automation lanes on this track.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub automation: Vec<crate::automation::AutomationLane>,
    /// Post-fader send amounts into buses: `(bus id, 0..1)`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sends: Vec<(TrackId, f32)>,
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
            swing_offset: None,
            effects: Vec::new(),
            kind: TrackKind::default(),
            color_index: 0,
            instrument: None,
            native_instrument: None,
            plugin_instrument: None,
            automation: Vec::new(),
            sends: Vec::new(),
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
    /// Nominal BPM of the underlying sample, set either by BPM
    /// detection or manually. Drives warp ratio calculations and is
    /// independent of the project tempo.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub original_bpm: Option<f64>,
    /// Whether the clip's audio has been time-stretched to fit the
    /// project tempo.
    #[serde(default, skip_serializing_if = "skip_if_false")]
    pub warped: bool,
    /// Project BPM the current warped audio was stretched to. Used to
    /// flag staleness when the project tempo changes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warped_to_bpm: Option<f64>,
}

fn skip_if_false(b: &bool) -> bool {
    !b
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

    fn test_clip(position: u64, duration: u64) -> ClipInfo {
        ClipInfo {
            id: ClipId::new(),
            track_id: TrackId::new(),
            name: "test".into(),
            position,
            source_offset: 0,
            duration,
            source: Some(MediaSourceRef::LocalFile {
                path: PathBuf::from("test.wav"),
            }),
            file_path: Some(PathBuf::from("test.wav")),
            loop_enabled: false,
            loop_start: 0,
            loop_end: 0,
            original_bpm: None,
            warped: false,
            warped_to_bpm: None,
        }
    }

    #[test]
    fn clip_end_position() {
        let clip = test_clip(1000, 500);
        assert_eq!(clip.end_position(), 1500);
    }

    #[test]
    fn clip_end_position_saturates() {
        let clip = test_clip(u64::MAX - 10, 100);
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
        let mut clip = test_clip(44_100, 88_200);
        clip.name = "vocal.wav".into();
        clip.source_offset = 1_000;
        clip.source = Some(MediaSourceRef::LocalFile {
            path: PathBuf::from("/audio/vocal.wav"),
        });
        clip.file_path = Some(PathBuf::from("/audio/vocal.wav"));
        clip.original_bpm = Some(174.0);
        clip.warped = true;
        clip.warped_to_bpm = Some(140.0);
        let json = serde_json::to_string(&clip).unwrap();
        let deserialized: ClipInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(clip.id, deserialized.id);
        assert_eq!(clip.position, deserialized.position);
        assert_eq!(clip.source_offset, deserialized.source_offset);
        assert_eq!(clip.duration, deserialized.duration);
        assert_eq!(clip.file_path, deserialized.file_path);
        assert_eq!(clip.source, deserialized.source);
        assert_eq!(clip.original_bpm, deserialized.original_bpm);
        assert_eq!(clip.warped, deserialized.warped);
        assert_eq!(clip.warped_to_bpm, deserialized.warped_to_bpm);
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
