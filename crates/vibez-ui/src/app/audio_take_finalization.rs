//! Recorded take staging and validation outside the live recording lifecycle.

use std::path::Path;
use std::sync::Arc;

use vibez_core::audio_buffer::DecodedAudio;
use vibez_core::id::TrackId;

use crate::domains::audio_recording::{AudioRecordingPhase, AudioRecordingSource};
use crate::message::AudioRecordingOutcome;

pub(super) fn recording_target_accepts_finalizer(
    phase: AudioRecordingPhase,
    armed_track: Option<TrackId>,
    outcome_track: TrackId,
    target_exists: bool,
) -> bool {
    phase == AudioRecordingPhase::Finalizing && armed_track == Some(outcome_track) && target_exists
}

pub(super) struct FinalizeAudioTake {
    pub track_id: TrackId,
    pub start_position_samples: u64,
    pub sample_rate: u32,
    pub frames: Vec<[f32; 2]>,
    pub recording_source: AudioRecordingSource,
    pub completion_label: String,
    pub truncated: bool,
    pub underrun_frames: u64,
}

pub(super) async fn finalize_audio_take(
    request: FinalizeAudioTake,
) -> Result<AudioRecordingOutcome, String> {
    let FinalizeAudioTake {
        track_id,
        start_position_samples,
        sample_rate,
        frames,
        recording_source,
        completion_label,
        truncated,
        underrun_frames,
    } = request;
    tokio::task::spawn_blocking(move || {
        if frames.is_empty() {
            return Err("the take contained no recorded audio frames".into());
        }
        let clip_name = match recording_source {
            AudioRecordingSource::HardwareInput => format!("Recording {start_position_samples}"),
            AudioRecordingSource::TrackOutput(_) => format!("Resample {start_position_samples}"),
        };
        let audio = Arc::new(DecodedAudio {
            channels: vec![
                frames.iter().map(|frame| frame[0]).collect(),
                frames.iter().map(|frame| frame[1]).collect(),
            ],
            sample_rate,
        });
        let bytes = vibez_audio_io::file_io::encode_wav(&audio);
        let source = vibez_project::project_format_v1::stage_local_content(
            Path::new("Recorded Audio"),
            &format!("{clip_name}.wav"),
            &bytes,
        )
        .map_err(|error| format!("Project Media staging failed: {error}"))?;
        let mut warnings = Vec::new();
        if truncated {
            warnings
                .push("capture overflow truncated the take to its valid captured frames".into());
        }
        if recording_source == AudioRecordingSource::HardwareInput && underrun_frames > 0 {
            warnings.push(format!(
                "{underrun_frames} input frame(s) were unavailable and replaced with silence"
            ));
        }
        Ok(AudioRecordingOutcome {
            track_id,
            start_position_samples,
            clip_name,
            audio,
            source,
            completion_label,
            quality_warning: (!warnings.is_empty()).then(|| warnings.join("; ")),
        })
    })
    .await
    .map_err(|error| format!("recording finalization task failed: {error}"))?
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::load_project_async;
    use vibez_core::id::ClipId;
    use vibez_core::track::MediaSourceRef;

    #[tokio::test]
    async fn completed_take_is_staged_before_it_can_become_project_content() {
        let outcome = finalize_audio_take(FinalizeAudioTake {
            track_id: TrackId::new(),
            start_position_samples: 9_600,
            sample_rate: 48_000,
            frames: vec![[0.25, -0.25]; 480],
            recording_source: AudioRecordingSource::HardwareInput,
            completion_label: "Recorded Audio Input".into(),
            truncated: false,
            underrun_frames: 0,
        })
        .await
        .unwrap();

        assert_eq!(outcome.start_position_samples, 9_600);
        assert_eq!(outcome.audio.num_frames(), 480);
        match outcome.source {
            MediaSourceRef::StagedProjectMedia {
                staging_path,
                file_name,
                ..
            } => {
                assert!(staging_path.is_file());
                assert!(file_name.ends_with(".wav"));
            }
            source => panic!("expected staged Project Media, got {source:?}"),
        }
    }

    #[test]
    fn a_stale_finalizer_cannot_land_in_a_new_project_or_ghost_track() {
        let target = TrackId::new();
        assert!(!recording_target_accepts_finalizer(
            AudioRecordingPhase::Idle,
            Some(target),
            target,
            true,
        ));
        assert!(!recording_target_accepts_finalizer(
            AudioRecordingPhase::Finalizing,
            Some(target),
            target,
            false,
        ));
        assert!(recording_target_accepts_finalizer(
            AudioRecordingPhase::Finalizing,
            Some(target),
            target,
            true,
        ));
    }

    #[tokio::test]
    async fn drift_damage_is_preserved_as_an_explicit_salvage_warning() {
        let outcome = finalize_audio_take(FinalizeAudioTake {
            track_id: TrackId::new(),
            start_position_samples: 0,
            sample_rate: 48_000,
            frames: vec![[0.1, -0.1]; 32],
            recording_source: AudioRecordingSource::HardwareInput,
            completion_label: "Recorded Audio Input".into(),
            truncated: true,
            underrun_frames: 7,
        })
        .await
        .unwrap();
        let warning = outcome.quality_warning.unwrap();
        assert!(warning.contains("overflow truncated"));
        assert!(warning.contains("7 input frame(s)"));
    }

    #[tokio::test]
    async fn resample_take_uses_recorded_audio_staging_without_hardware_drift_warnings() {
        let outcome = finalize_audio_take(FinalizeAudioTake {
            track_id: TrackId::new(),
            start_position_samples: 24_000,
            sample_rate: 48_000,
            frames: vec![[0.3, -0.2]; 128],
            recording_source: AudioRecordingSource::TrackOutput(TrackId::new()),
            completion_label: "Resampled MIDI 2".into(),
            truncated: false,
            underrun_frames: 99,
        })
        .await
        .unwrap();

        assert_eq!(outcome.clip_name, "Resample 24000");
        assert_eq!(outcome.completion_label, "Resampled MIDI 2");
        assert_eq!(outcome.audio.num_frames(), 128);
        assert!(outcome.quality_warning.is_none());
        assert!(matches!(
            outcome.source,
            MediaSourceRef::StagedProjectMedia { .. }
        ));
    }

    #[tokio::test]
    async fn recorded_take_reopens_from_project_media_at_the_same_position() {
        let directory = tempfile::tempdir().unwrap();
        let project_path = directory.path().join("recording-roundtrip.vzp");
        let track = vibez_core::track::TrackInfo::new("Vocal");
        let outcome = finalize_audio_take(FinalizeAudioTake {
            track_id: track.id,
            start_position_samples: 12_345,
            sample_rate: 48_000,
            frames: vec![[0.4, -0.2]; 960],
            recording_source: AudioRecordingSource::HardwareInput,
            completion_label: "Recorded Audio Input".into(),
            truncated: false,
            underrun_frames: 0,
        })
        .await
        .unwrap();
        let project = vibez_project::Project {
            tracks: vec![track.clone()],
            arrange: vibez_project::TimelineInfo {
                clips: vec![vibez_core::track::ClipInfo {
                    id: ClipId::new(),
                    track_id: track.id,
                    name: outcome.clip_name,
                    position: outcome.start_position_samples,
                    source_offset: 0,
                    start_marker: None,
                    duration: outcome.audio.num_frames() as u64,
                    source: Some(outcome.source),
                    file_path: None,
                    loop_enabled: false,
                    loop_start: 0,
                    loop_end: outcome.audio.num_frames() as u64,
                    gain_db: Default::default(),
                    fades: Default::default(),
                    playback_direction: Default::default(),
                    transient_markers: Default::default(),
                    transpose: Default::default(),
                    original_bpm: None,
                    warped: false,
                    warped_to_bpm: None,
                }],
                ..Default::default()
            },
            ..Default::default()
        };
        vibez_project::project_format_v1::save_project_v1(&project_path, None, project).unwrap();

        let reopened = load_project_async(project_path, None).await.unwrap();
        assert_eq!(reopened.project.arrange.clips[0].position, 12_345);
        assert_eq!(reopened.clips[0].audio.num_frames(), 960);
        assert!(matches!(
            reopened.project.arrange.clips[0].source,
            Some(MediaSourceRef::ProjectMedia { .. })
        ));
    }
}
