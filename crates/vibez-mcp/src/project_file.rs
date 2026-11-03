//! Saves sessions as self-contained `.vzp` projects and opens existing Vibez projects for editing.

use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::Serialize;

use crate::session::TrackSession;

#[derive(Debug, thiserror::Error)]
pub enum ProjectFileError {
    #[error("projects are saved as .vzp files, got {0}")]
    NotVzp(PathBuf),
    #[error("could not save {path}: {reason}")]
    Save { path: PathBuf, reason: String },
    #[error("could not open {path}: {reason}")]
    Open { path: PathBuf, reason: String },
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct SaveReport {
    pub path: PathBuf,
    pub embedded_media: usize,
    pub bytes: u64,
}

/// Saves to a `.vzp`, embedding every referenced sample. The session then
/// refers to the embedded copies, so later saves and bounces need no source files.
pub fn save(session: &mut TrackSession, path: &Path) -> Result<SaveReport, ProjectFileError> {
    let _ = (session, path);
    todo!()
}

pub fn open(path: &Path) -> Result<TrackSession, ProjectFileError> {
    let _ = path;
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bounce::{bounce, BounceRange};
    use crate::session::{NewAudioClip, NewTrack};
    use vibez_core::audio_buffer::DecodedAudio;
    use vibez_core::track::MediaSourceRef;

    fn session_with_loop(dir: &Path) -> (TrackSession, PathBuf) {
        let sample = dir.join("loop.wav");
        let wave: Vec<f32> = (0..88_200).map(|i| 0.4 * (i as f32 * 0.03).sin()).collect();
        vibez_audio_io::file_io::write_wav_file(
            &sample,
            &DecodedAudio {
                channels: vec![wave.clone(), wave],
                sample_rate: 44_100,
            },
        )
        .unwrap();
        let mut session = TrackSession::new("saved", 100.0).unwrap();
        let id = session
            .add_track(NewTrack {
                name: "loop".into(),
                instrument: None,
                sample: None,
                gain_db: None,
                pan: None,
            })
            .unwrap();
        session
            .add_audio_clip(NewAudioClip {
                track_id: id,
                sample: sample.clone(),
                start_beat: 0.0,
                length_beats: Some(8.0),
                loop_beats: Some(2.0),
                gain_db: None,
            })
            .unwrap();
        (session, sample)
    }

    #[test]
    fn saving_embeds_samples_and_points_the_session_at_them() {
        let dir = tempfile::tempdir().unwrap();
        let (mut session, sample) = session_with_loop(dir.path());
        let target = dir.path().join("song.vzp");
        let report = save(&mut session, &target).unwrap();
        assert_eq!(report.embedded_media, 1);
        assert_eq!(session.container(), Some(target.as_path()));
        assert!(matches!(
            session.project().arrange.clips[0].source,
            Some(MediaSourceRef::ProjectMedia { .. })
        ));
        std::fs::remove_file(sample).unwrap();
        let out = dir.path().join("after-save.wav");
        let rendered = bounce(&session, &out, &BounceRange::default()).unwrap();
        assert!(rendered.peak_dbfs > -20.0);
    }

    #[test]
    fn an_opened_project_round_trips_tracks_and_clips() {
        let dir = tempfile::tempdir().unwrap();
        let (mut session, _) = session_with_loop(dir.path());
        let target = dir.path().join("song.vzp");
        save(&mut session, &target).unwrap();
        let reopened = open(&target).unwrap();
        assert_eq!(reopened.project().bpm, 100.0);
        assert_eq!(reopened.project().tracks.len(), 1);
        assert_eq!(reopened.project().arrange.clips.len(), 1);
        assert_eq!(reopened.container(), Some(target.as_path()));
    }

    #[test]
    fn only_vzp_destinations_are_accepted() {
        let dir = tempfile::tempdir().unwrap();
        let (mut session, _) = session_with_loop(dir.path());
        assert!(matches!(
            save(&mut session, &dir.path().join("song.json")),
            Err(ProjectFileError::NotVzp(_))
        ));
    }

    #[test]
    fn a_missing_project_reports_where_it_looked() {
        let error = open(Path::new("/nonexistent/song.vzp")).unwrap_err();
        assert!(matches!(error, ProjectFileError::Open { .. }));
    }
}
