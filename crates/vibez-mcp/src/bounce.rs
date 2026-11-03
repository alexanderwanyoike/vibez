//! Renders a session offline through the Vibez engine, exactly as the app's Export does, and writes a WAV.

use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::Serialize;

use crate::session::TrackSession;

/// Seconds rendered after the last clip so reverb and delay tails ring out.
pub const DEFAULT_TAIL_SECONDS: f64 = 2.0;

#[derive(Debug, thiserror::Error)]
pub enum BounceError {
    #[error("nothing to render: the project has no clips")]
    NothingToRender,
    #[error("invalid render range: {0}")]
    InvalidRange(String),
    #[error("could not load {label}: {reason}")]
    Media { label: String, reason: String },
    #[error("the render was incomplete: {0}")]
    Incomplete(String),
    #[error("could not write {path}: {reason}")]
    Write { path: PathBuf, reason: String },
}

#[derive(Debug, Clone, Default)]
pub struct BounceRange {
    pub start_beat: Option<f64>,
    pub end_beat: Option<f64>,
    pub tail_seconds: Option<f64>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct BounceReport {
    pub path: PathBuf,
    pub sample_rate: u32,
    pub seconds: f64,
    pub peak_dbfs: f32,
    pub rms_dbfs: f32,
}

pub fn bounce(
    session: &TrackSession,
    path: &Path,
    range: &BounceRange,
) -> Result<BounceReport, BounceError> {
    let _ = (session, path, range);
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device_params::Device;
    use crate::session::{NewAudioClip, NewNoteClip, NewTrack, Note};
    use vibez_core::audio_buffer::DecodedAudio;

    fn sine(dir: &Path, name: &str, seconds: f64, hz: f32) -> PathBuf {
        let path = dir.join(name);
        let frames = (seconds * 44_100.0) as usize;
        let wave: Vec<f32> = (0..frames)
            .map(|i| 0.5 * (std::f32::consts::TAU * hz * i as f32 / 44_100.0).sin())
            .collect();
        let audio = DecodedAudio {
            channels: vec![wave.clone(), wave],
            sample_rate: 44_100,
        };
        vibez_audio_io::file_io::write_wav_file(&path, &audio).unwrap();
        path
    }

    fn track(name: &str, instrument: Option<Device>, sample: Option<PathBuf>) -> NewTrack {
        NewTrack {
            name: name.into(),
            instrument,
            sample,
            gain_db: None,
            pan: None,
        }
    }

    fn note(pitch: u8) -> Note {
        Note {
            pitch,
            velocity: 100,
            start_beat: 0.0,
            length_beats: 2.0,
        }
    }

    fn rendered(dir: &Path, session: &TrackSession) -> (BounceReport, DecodedAudio) {
        let out = dir.join("out.wav");
        let report = bounce(session, &out, &BounceRange::default()).unwrap();
        let audio = vibez_audio_io::file_io::decode_audio_file(&out).unwrap();
        (report, audio)
    }

    #[test]
    fn an_empty_project_has_nothing_to_render() {
        let dir = tempfile::tempdir().unwrap();
        let session = TrackSession::new("empty", 120.0).unwrap();
        assert!(matches!(
            bounce(&session, &dir.path().join("x.wav"), &BounceRange::default()),
            Err(BounceError::NothingToRender)
        ));
    }

    #[test]
    fn a_looping_audio_clip_renders_for_its_full_length_plus_tail() {
        let dir = tempfile::tempdir().unwrap();
        let mut session = TrackSession::new("loop", 120.0).unwrap();
        let id = session.add_track(track("loop", None, None)).unwrap();
        session
            .add_audio_clip(NewAudioClip {
                track_id: id,
                sample: sine(dir.path(), "tone.wav", 2.5, 220.0),
                start_beat: 0.0,
                length_beats: Some(16.0),
                loop_beats: Some(4.0),
                gain_db: None,
            })
            .unwrap();
        let (report, audio) = rendered(dir.path(), &session);
        assert!((report.seconds - (8.0 + DEFAULT_TAIL_SECONDS)).abs() < 0.01);
        assert!(report.peak_dbfs > -12.0, "peak {}", report.peak_dbfs);
        let late = &audio.channels[0][6 * 44_100..6 * 44_100 + 4_410];
        assert!(late.iter().any(|s| s.abs() > 0.05), "the loop stopped");
    }

    #[test]
    fn native_instruments_play_their_notes_offline() {
        let dir = tempfile::tempdir().unwrap();
        let mut session = TrackSession::new("synths", 120.0).unwrap();
        let fm = session
            .add_track(track("fm", Some(Device::FmSynth), None))
            .unwrap();
        let sampler = session
            .add_track(track(
                "sampler",
                Some(Device::Sampler),
                Some(sine(dir.path(), "hit.wav", 0.5, 110.0)),
            ))
            .unwrap();
        for (id, start) in [(fm, 0.0), (sampler, 4.0)] {
            session
                .add_note_clip(NewNoteClip {
                    track_id: id,
                    start_beat: start,
                    length_beats: 4.0,
                    notes: vec![note(60)],
                })
                .unwrap();
        }
        let (_, audio) = rendered(dir.path(), &session);
        let window = |seconds: f64| {
            let at = (seconds * 44_100.0) as usize;
            audio.channels[0][at..at + 4_410]
                .iter()
                .fold(0f32, |m, s| m.max(s.abs()))
        };
        assert!(window(0.2) > 0.01, "the FM synth was silent");
        assert!(window(2.1) > 0.01, "the sampler was silent");
    }

    #[test]
    fn a_muted_track_renders_silence() {
        let dir = tempfile::tempdir().unwrap();
        let mut session = TrackSession::new("mute", 120.0).unwrap();
        let id = session.add_track(track("loop", None, None)).unwrap();
        session
            .add_audio_clip(NewAudioClip {
                track_id: id,
                sample: sine(dir.path(), "tone.wav", 1.0, 220.0),
                start_beat: 0.0,
                length_beats: None,
                loop_beats: None,
                gain_db: None,
            })
            .unwrap();
        session.set_track_mix(id, None, None, Some(true)).unwrap();
        let (report, _) = rendered(dir.path(), &session);
        assert!(report.peak_dbfs < -90.0, "peak {}", report.peak_dbfs);
    }

    #[test]
    fn a_range_renders_only_the_requested_beats() {
        let dir = tempfile::tempdir().unwrap();
        let mut session = TrackSession::new("range", 120.0).unwrap();
        let id = session
            .add_track(track("fm", Some(Device::FmSynth), None))
            .unwrap();
        session
            .add_note_clip(NewNoteClip {
                track_id: id,
                start_beat: 0.0,
                length_beats: 16.0,
                notes: vec![note(48)],
            })
            .unwrap();
        let out = dir.path().join("range.wav");
        let report = bounce(
            &session,
            &out,
            &BounceRange {
                start_beat: Some(4.0),
                end_beat: Some(8.0),
                tail_seconds: Some(0.0),
            },
        )
        .unwrap();
        assert!((report.seconds - 2.0).abs() < 0.01);
        assert!(matches!(
            bounce(
                &session,
                &out,
                &BounceRange {
                    start_beat: Some(8.0),
                    end_beat: Some(4.0),
                    tail_seconds: None,
                },
            ),
            Err(BounceError::InvalidRange(_))
        ));
    }
}
