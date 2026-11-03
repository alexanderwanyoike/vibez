//! Renders a session offline through the Vibez engine, exactly as the app's Export does, and writes a WAV.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use schemars::JsonSchema;
use serde::Serialize;
use vibez_core::audio_buffer::DecodedAudio;
use vibez_core::track::{InstrumentStateInfo, MediaSourceRef};
use vibez_engine::render::{BounceMode, BounceRequest};

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
    let project = session.project();
    let content_end = session.length_beats();
    if content_end <= 0.0 {
        return Err(BounceError::NothingToRender);
    }
    let start_beat = range.start_beat.unwrap_or(0.0);
    let end_beat = range.end_beat.unwrap_or(content_end);
    let tail = range.tail_seconds.unwrap_or(DEFAULT_TAIL_SECONDS);
    let valid_beats = start_beat.is_finite()
        && end_beat.is_finite()
        && start_beat >= 0.0
        && end_beat > start_beat;
    let valid_tail = tail.is_finite() && tail >= 0.0;
    if !(valid_beats && valid_tail) {
        return Err(BounceError::InvalidRange(format!(
            "beats {start_beat}..{end_beat} with a {tail}s tail"
        )));
    }
    let sample_rate = project.sample_rate;
    let start = session.beats_to_frames(start_beat);
    let end = session.beats_to_frames(end_beat) + (tail * f64::from(sample_rate)).round() as u64;

    let media = MediaResolver::new(session.container(), sample_rate);
    let mut clip_audio = HashMap::new();
    for clip in &project.arrange.clips {
        let source = clip.resolved_source().ok_or_else(|| BounceError::Media {
            label: clip.name.clone(),
            reason: "the clip has no media source".into(),
        })?;
        clip_audio.insert(clip.id, media.load(source, &clip.name)?);
    }
    let mut sampler_audio = HashMap::new();
    let mut drum_pad_audio = HashMap::new();
    for track in &project.tracks {
        match &track.native_instrument {
            Some(InstrumentStateInfo::Sampler {
                source: Some(source),
                ..
            }) => {
                let audio = media.load(source, &track.name)?;
                sampler_audio.insert(track.id, (audio, source_label(source)));
            }
            Some(InstrumentStateInfo::DrumRack { pads }) => {
                for (index, pad) in pads.iter().enumerate() {
                    if let Some(source) = &pad.source {
                        let label = format!("{} pad {}", track.name, index + 1);
                        let audio = media.load(source, &label)?;
                        drum_pad_audio.insert(
                            (track.id, index),
                            (audio, pad.name.clone().unwrap_or(label)),
                        );
                    }
                }
            }
            _ => {}
        }
    }

    let request = BounceRequest {
        tracks: project.tracks.clone(),
        master: project.master.clone(),
        buses: project.buses.clone(),
        audio_clips: project.arrange.clips.clone(),
        note_clips: project.arrange.note_clips.clone(),
        clip_audio,
        sampler_audio,
        drum_pad_audio,
        mode: BounceMode::Master,
        range_samples: (start, end),
        bpm: project.bpm,
        sample_rate,
        swing: project.swing,
    };
    let result = vibez_engine::render::render_offline(&request);
    if !result.warnings.is_empty() {
        return Err(BounceError::Incomplete(result.warnings.join("; ")));
    }
    vibez_audio_io::file_io::write_wav_file(path, &result.audio).map_err(|error| {
        BounceError::Write {
            path: path.to_path_buf(),
            reason: error.to_string(),
        }
    })?;
    let (peak, rms) = levels(&result.audio);
    Ok(BounceReport {
        path: path.to_path_buf(),
        sample_rate,
        seconds: result.audio.duration_seconds(),
        peak_dbfs: to_dbfs(peak),
        rms_dbfs: to_dbfs(rms),
    })
}

/// Loads media the way the app hydrates a saved project, then resamples to the
/// project rate as an import would.
struct MediaResolver<'a> {
    container: Option<&'a Path>,
    sample_rate: u32,
}

impl<'a> MediaResolver<'a> {
    fn new(container: Option<&'a Path>, sample_rate: u32) -> Self {
        Self {
            container,
            sample_rate,
        }
    }

    fn load(&self, source: &MediaSourceRef, label: &str) -> Result<Arc<DecodedAudio>, BounceError> {
        let fail = |reason: String| BounceError::Media {
            label: label.to_string(),
            reason,
        };
        let decoded = match source {
            MediaSourceRef::LocalFile { path }
            | MediaSourceRef::StagedProjectMedia {
                staging_path: path, ..
            }
            | MediaSourceRef::StagedRemoteProjectMedia {
                staging_path: path, ..
            } => {
                vibez_audio_io::file_io::decode_audio_file(path).map_err(|e| fail(e.to_string()))?
            }
            MediaSourceRef::ProjectMedia { id, file_name, .. } => {
                let container = self
                    .container
                    .ok_or_else(|| fail("Project Media needs the .vzp it came from".into()))?;
                let bytes = vibez_project::project_format_v1::ProjectContainer::open(container)
                    .and_then(|container| container.read_media(id))
                    .map_err(|e| fail(e.to_string()))?;
                let extension = Path::new(file_name)
                    .extension()
                    .map(|value| value.to_string_lossy().into_owned());
                vibez_audio_io::file_io::decode_audio_cursor(
                    std::io::Cursor::new(bytes),
                    extension.as_deref(),
                )
                .map_err(|e| fail(e.to_string()))?
            }
            MediaSourceRef::DropboxFile {
                path_lower, rev, ..
            } => {
                let cache = vibez_dropbox::DropboxCache::new();
                if !cache.is_cached(path_lower, rev.as_deref()) {
                    return Err(fail(
                        "the Dropbox file is not downloaded yet; fetch it first".into(),
                    ));
                }
                let path = cache.path_for(path_lower, rev.as_deref());
                vibez_audio_io::file_io::decode_audio_file(&path)
                    .map_err(|e| fail(e.to_string()))?
            }
        };
        let audio = vibez_audio_io::file_io::resample_audio(&decoded, self.sample_rate)
            .map_err(|e| fail(e.to_string()))?;
        Ok(Arc::new(audio))
    }
}

fn source_label(source: &MediaSourceRef) -> String {
    match source {
        MediaSourceRef::LocalFile { path } => path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default(),
        MediaSourceRef::StagedProjectMedia { file_name, .. }
        | MediaSourceRef::StagedRemoteProjectMedia { file_name, .. }
        | MediaSourceRef::ProjectMedia { file_name, .. } => file_name.clone(),
        MediaSourceRef::DropboxFile { display_path, .. } => display_path
            .rsplit('/')
            .next()
            .unwrap_or(display_path)
            .to_string(),
    }
}

fn levels(audio: &DecodedAudio) -> (f32, f32) {
    let mut peak = 0f32;
    let mut sum = 0f64;
    let mut count = 0usize;
    for channel in &audio.channels {
        for &sample in channel {
            peak = peak.max(sample.abs());
            sum += f64::from(sample) * f64::from(sample);
            count += 1;
        }
    }
    let rms = if count == 0 {
        0.0
    } else {
        (sum / count as f64).sqrt() as f32
    };
    (peak, rms)
}

fn to_dbfs(level: f32) -> f32 {
    if level <= 0.0 {
        -120.0
    } else {
        (20.0 * level.log10()).max(-120.0)
    }
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
