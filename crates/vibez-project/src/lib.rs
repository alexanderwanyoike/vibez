use std::path::Path;

use serde::{Deserialize, Serialize};
use vibez_core::track::{ClipInfo, TrackInfo};

/// A serializable project containing tracks and clips.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub name: String,
    pub bpm: f64,
    pub sample_rate: u32,
    pub tracks: Vec<TrackInfo>,
    pub clips: Vec<ClipInfo>,
}

impl Default for Project {
    fn default() -> Self {
        Self {
            name: "Untitled".to_string(),
            bpm: vibez_core::constants::DEFAULT_BPM,
            sample_rate: vibez_core::constants::DEFAULT_SAMPLE_RATE,
            tracks: Vec::new(),
            clips: Vec::new(),
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

    #[test]
    fn project_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.vibez");

        let project = Project {
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
                file_path: PathBuf::from("audio/loop.wav"),
            }],
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
}
