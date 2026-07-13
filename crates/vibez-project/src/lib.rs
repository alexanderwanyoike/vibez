use std::path::Path;

use serde::{Deserialize, Serialize};
use vibez_core::midi::NoteClipInfo;
use vibez_core::track::{ClipInfo, TrackInfo};

pub mod project_format_v1_proof;

/// A serializable project containing tracks and clips.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub name: String,
    pub bpm: f64,
    pub sample_rate: u32,
    pub tracks: Vec<TrackInfo>,
    pub clips: Vec<ClipInfo>,
    #[serde(default)]
    pub note_clips: Vec<NoteClipInfo>,
    /// The master bus channel (gain + effect chain). Absent in
    /// projects saved before the master was a real channel.
    #[serde(default)]
    pub master: Option<TrackInfo>,
    /// Return buses (mixer-only channels fed by track sends).
    #[serde(default)]
    pub buses: Vec<TrackInfo>,
}

impl Default for Project {
    fn default() -> Self {
        Self {
            name: "Untitled".to_string(),
            bpm: vibez_core::constants::DEFAULT_BPM,
            sample_rate: vibez_core::constants::DEFAULT_SAMPLE_RATE,
            tracks: Vec::new(),
            clips: Vec::new(),
            note_clips: Vec::new(),
            master: None,
            buses: Vec::new(),
        }
    }
}

/// Errors that can occur during project save/load.
#[derive(Debug)]
pub enum ProjectError {
    Io(std::io::Error),
    Json(serde_json::Error),
}

impl std::fmt::Display for ProjectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProjectError::Io(e) => write!(f, "I/O error: {e}"),
            ProjectError::Json(e) => write!(f, "JSON error: {e}"),
        }
    }
}

impl std::error::Error for ProjectError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ProjectError::Io(e) => Some(e),
            ProjectError::Json(e) => Some(e),
        }
    }
}

impl From<std::io::Error> for ProjectError {
    fn from(e: std::io::Error) -> Self {
        ProjectError::Io(e)
    }
}

impl From<serde_json::Error> for ProjectError {
    fn from(e: serde_json::Error) -> Self {
        ProjectError::Json(e)
    }
}

impl Project {
    /// Save the project to a JSON file.
    pub fn save_to_file(&self, path: &Path) -> Result<(), ProjectError> {
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(path, json)?;
        Ok(())
    }

    /// Load a project from a JSON file.
    pub fn load_from_file(path: &Path) -> Result<Self, ProjectError> {
        let json = std::fs::read_to_string(path)?;
        let project = serde_json::from_str(&json)?;
        Ok(project)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error;
    use std::path::PathBuf;
    use vibez_core::id::{ClipId, TrackId};
    use vibez_core::midi::{MidiNote, NoteClipInfo};
    use vibez_core::track::{InstrumentStateInfo, MediaSourceRef};

    #[test]
    fn plugin_devices_roundtrip() {
        use vibez_core::effect::{EffectInfo, EffectType, PluginDeviceInfo};
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("plugins.vibez");

        let dev = PluginDeviceInfo {
            format: "vst3".to_string(),
            uid: "ABCD1234-0000-0000-0000-000000000000".to_string(),
            path: PathBuf::from("/plugins/ZL Equalizer 2.vst3"),
            name: "ZL Equalizer 2".to_string(),
            state_b64: Some("dmliZXogc3RhdGU=".to_string()),
        };
        let mut track = TrackInfo::new("FX");
        track.effects.push(EffectInfo {
            id: vibez_core::id::EffectId::new(),
            effect_type: EffectType::Gain,
            bypass: false,
            params: Vec::new(),
            plugin: Some(dev.clone()),
        });
        track.plugin_instrument = Some(PluginDeviceInfo {
            format: "clap".to_string(),
            uid: "com.surge-synth-team.surge-xt".to_string(),
            path: PathBuf::from("/plugins/Surge XT.clap"),
            name: "Surge XT".to_string(),
            state_b64: None,
        });

        let mut project = Project::default();
        project.tracks.push(track);
        project.save_to_file(&path).unwrap();
        let loaded = Project::load_from_file(&path).unwrap();

        assert_eq!(loaded.tracks[0].effects[0].plugin.as_ref(), Some(&dev));
        assert_eq!(
            loaded.tracks[0]
                .plugin_instrument
                .as_ref()
                .map(|d| d.name.as_str()),
            Some("Surge XT")
        );
    }

    #[test]
    fn pre_plugin_schema_still_loads() {
        // A track serialized before the plugin fields existed must
        // deserialize with them defaulted.
        let json = r#"{
            "id": 7, "name": "Old", "gain": 1.0, "pan": 0.0,
            "mute": false, "solo": false,
            "effects": [{ "id": 9, "effect_type": "Delay",
                          "bypass": false, "params": [1.0] }]
        }"#;
        let track: TrackInfo = serde_json::from_str(json).unwrap();
        assert!(track.effects[0].plugin.is_none());
        assert!(track.plugin_instrument.is_none());
    }

    #[test]
    fn buses_and_sends_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("buses.vzp");

        let bus = TrackInfo::new("A Return");
        let mut track = TrackInfo::new("Drums");
        track.sends.push((bus.id, 0.65));

        let mut project = Project::default();
        let bus_id = bus.id;
        project.tracks.push(track);
        project.buses.push(bus);
        project.save_to_file(&path).unwrap();

        let loaded = Project::load_from_file(&path).unwrap();
        assert_eq!(loaded.buses.len(), 1);
        assert_eq!(loaded.buses[0].name, "A Return");
        assert_eq!(loaded.tracks[0].sends, vec![(bus_id, 0.65)]);
    }

    #[test]
    fn pre_bus_schema_still_loads() {
        // Files saved before buses existed must load with empty
        // buses and sends.
        let json = r#"{
            "name": "Old", "bpm": 120.0, "sample_rate": 44100,
            "tracks": [{ "id": 1, "name": "T", "gain": 1.0,
                         "pan": 0.5, "mute": false, "solo": false }],
            "clips": []
        }"#;
        let project: Project = serde_json::from_str(json).unwrap();
        assert!(project.buses.is_empty());
        assert!(project.tracks[0].sends.is_empty());
    }

    #[test]
    fn project_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.vibez");

        let project = Project {
            master: None,
            buses: Vec::new(),
            name: "Test Project".into(),
            bpm: 140.0,
            sample_rate: 48_000,
            tracks: vec![TrackInfo::new("Synth"), TrackInfo::new("Bass")],
            clips: vec![ClipInfo {
                id: ClipId::new(),
                track_id: TrackId::new(),
                name: "loop.wav".into(),
                position: 0,
                source_offset: 0,
                duration: 44100,
                source: Some(MediaSourceRef::LocalFile {
                    path: PathBuf::from("audio/loop.wav"),
                }),
                file_path: Some(PathBuf::from("audio/loop.wav")),
                loop_enabled: false,
                loop_start: 0,
                loop_end: 0,
                original_bpm: None,
                warped: false,
                warped_to_bpm: None,
            }],
            note_clips: Vec::new(),
        };

        project.save_to_file(&path).unwrap();
        let loaded = Project::load_from_file(&path).unwrap();

        assert_eq!(loaded.name, "Test Project");
        assert!((loaded.bpm - 140.0).abs() < f64::EPSILON);
        assert_eq!(loaded.sample_rate, 48_000);
        assert_eq!(loaded.tracks.len(), 2);
        assert_eq!(loaded.tracks[0].name, "Synth");
        assert_eq!(loaded.clips.len(), 1);
        assert_eq!(loaded.clips[0].name, "loop.wav");
    }

    #[test]
    fn empty_project_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("empty.vibez");

        let project = Project::default();
        project.save_to_file(&path).unwrap();
        let loaded = Project::load_from_file(&path).unwrap();

        assert_eq!(loaded.name, "Untitled");
        assert!(loaded.tracks.is_empty());
        assert!(loaded.clips.is_empty());
    }

    #[test]
    fn load_bad_path() {
        let result = Project::load_from_file(Path::new("/nonexistent/path.vibez"));
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, ProjectError::Io(_)));
        assert!(err.to_string().contains("I/O error"));
    }

    #[test]
    fn load_corrupt_json() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad.vibez");
        std::fs::write(&path, "not valid json {{{").unwrap();

        let result = Project::load_from_file(&path);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ProjectError::Json(_)));
    }

    #[test]
    fn error_implements_display_and_error() {
        let io_err = ProjectError::Io(std::io::Error::new(std::io::ErrorKind::NotFound, "test"));
        assert!(io_err.to_string().contains("I/O error"));
        assert!(io_err.source().is_some());

        let json_str = "not json";
        let json_err: Result<Project, _> = serde_json::from_str(json_str);
        let project_err = ProjectError::Json(json_err.unwrap_err());
        assert!(project_err.to_string().contains("JSON error"));
    }

    #[test]
    fn backward_compat_no_note_clips() {
        // Simulate a project saved before note_clips existed
        let json = r#"{
            "name": "Old Project",
            "bpm": 120.0,
            "sample_rate": 44100,
            "tracks": [],
            "clips": []
        }"#;
        let project: Project = serde_json::from_str(json).unwrap();
        assert_eq!(project.name, "Old Project");
        assert!(project.note_clips.is_empty());
    }

    #[test]
    fn note_clips_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("notes.vibez");

        let tid = TrackId::new();
        let project = Project {
            master: None,
            buses: Vec::new(),
            name: "Note Test".into(),
            bpm: 128.0,
            sample_rate: 44_100,
            tracks: vec![],
            clips: vec![],
            note_clips: vec![NoteClipInfo {
                id: ClipId::new(),
                track_id: tid,
                name: "Pattern 1".into(),
                position_beats: 0.0,
                duration_beats: 4.0,
                loop_enabled: false,
                loop_start_beats: 0.0,
                loop_end_beats: 0.0,
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
            }],
        };

        project.save_to_file(&path).unwrap();
        let loaded = Project::load_from_file(&path).unwrap();

        assert_eq!(loaded.note_clips.len(), 1);
        assert_eq!(loaded.note_clips[0].name, "Pattern 1");
        assert_eq!(loaded.note_clips[0].notes.len(), 2);
        assert_eq!(loaded.note_clips[0].notes[0].pitch, 60);
        assert_eq!(loaded.note_clips[0].notes[1].pitch, 64);
    }

    #[test]
    fn track_effects_roundtrip() {
        use vibez_core::effect::{EffectInfo, EffectType};
        use vibez_core::id::EffectId;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("fx.vibez");

        let mut track = TrackInfo::new("FX Track");
        track.effects.push(EffectInfo {
            id: EffectId::new(),
            effect_type: EffectType::Delay,
            bypass: false,
            params: vec![500.0, 0.5, 0.3],
            plugin: None,
        });
        track.native_instrument = Some(InstrumentStateInfo::SubtractiveSynth {
            params: vec![0.05, 0.2, 0.8, 0.4],
        });

        let project = Project {
            master: None,
            buses: Vec::new(),
            name: "FX Test".into(),
            bpm: 120.0,
            sample_rate: 44_100,
            tracks: vec![track],
            clips: vec![],
            note_clips: vec![],
        };

        project.save_to_file(&path).unwrap();
        let loaded = Project::load_from_file(&path).unwrap();

        assert_eq!(loaded.tracks.len(), 1);
        assert_eq!(loaded.tracks[0].effects.len(), 1);
        assert_eq!(loaded.tracks[0].effects[0].effect_type, EffectType::Delay);
        assert_eq!(loaded.tracks[0].effects[0].params.len(), 3);
        assert!(matches!(
            loaded.tracks[0].native_instrument,
            Some(InstrumentStateInfo::SubtractiveSynth { .. })
        ));
    }
}

#[cfg(test)]
mod automation_persistence_tests {
    use super::*;
    use vibez_core::automation::{AutomationLane, AutomationPoint, AutomationTarget};
    use vibez_core::track::TrackInfo;

    #[test]
    fn lanes_survive_a_save_load_roundtrip() {
        let mut track = TrackInfo::new("T1");
        let mut lane = AutomationLane::new(AutomationTarget::TrackGain);
        lane.insert_point(AutomationPoint {
            beat: 0.0,
            value: 1.0,
            curve: 0.0,
        });
        lane.insert_point(AutomationPoint {
            beat: 8.0,
            value: 0.25,
            curve: 0.5,
        });
        track.automation.push(lane.clone());

        let project = Project {
            master: None,
            buses: Vec::new(),
            name: "roundtrip".to_string(),
            tracks: vec![track],
            ..Default::default()
        };

        let dir = std::env::temp_dir().join("vibez-lane-roundtrip-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("p.vibez");
        project.save_to_file(&path).unwrap();
        let loaded = Project::load_from_file(&path).unwrap();
        std::fs::remove_dir_all(&dir).ok();

        assert_eq!(loaded.tracks[0].automation, vec![lane]);
    }

    #[test]
    fn projects_without_lanes_still_load() {
        // Backcompat: pre-automation files have no `automation` key.
        let json = r#"{"name":"old","bpm":120.0,"sample_rate":48000,
            "tracks":[{"id":1,"name":"T","gain":1.0,"pan":0.5,
            "mute":false,"solo":false}],"clips":[]}"#;
        let project: Project = serde_json::from_str(json).unwrap();
        assert!(project.tracks[0].automation.is_empty());
    }
}
