//! Offline preparation for faithful Audio Clip to Drum Rack conversion.

use std::sync::Arc;

use vibez_core::track::{MediaProvenance, MediaSourceRef};

pub(super) async fn prepare_drum_rack_audio_async(
    clip: crate::state::UiClip,
) -> Result<crate::message::PreparedDrumRackAudio, String> {
    tokio::task::spawn_blocking(move || {
        let source = clip
            .source
            .clone()
            .ok_or_else(|| "Slice to Drum Rack needs available Source Media".to_string())?;
        let frame_count = usize::try_from(clip.duration)
            .map_err(|_| "Audio Clip is too long to prepare as Drum Rack media".to_string())?;
        if frame_count == 0 || clip.audio.num_channels() == 0 {
            return Err("Slice to Drum Rack needs available audio".to_string());
        }

        let channels = (0..clip.audio.num_channels())
            .map(|channel| {
                (0..clip.duration)
                    .map(|clip_frame| {
                        let source_frame = clip.source_frame_position_at(clip_frame);
                        let fade_gain = clip.fades.gain_at(clip_frame, clip.duration);
                        clip.audio.sample_linear(channel, source_frame) * fade_gain
                    })
                    .collect::<Vec<_>>()
            })
            .collect();
        let rendered = Arc::new(vibez_core::audio_buffer::DecodedAudio {
            channels,
            sample_rate: clip.audio.sample_rate,
        });
        debug_assert_eq!(rendered.num_frames(), frame_count);

        let source_name = source.display_name();
        let stem = std::path::Path::new(&source_name)
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .filter(|stem| !stem.is_empty())
            .unwrap_or_else(|| "audio-clip".into());
        let file_name = format!("{stem}-slices.wav");
        let bytes = vibez_audio_io::file_io::encode_wav(&rendered);
        let staged = stage_rendered_audio(source, &file_name, &bytes)?;
        let persisted =
            vibez_audio_io::file_io::decode_audio_cursor(std::io::Cursor::new(bytes), Some("wav"))
                .map_err(|error| format!("Drum Rack media verification failed: {error}"))?;
        debug_assert_eq!(persisted.num_frames(), frame_count);

        Ok(crate::message::PreparedDrumRackAudio {
            source: staged,
            audio: Arc::new(persisted),
        })
    })
    .await
    .map_err(|error| format!("Drum Rack preparation task failed: {error}"))?
}

fn stage_rendered_audio(
    source: MediaSourceRef,
    file_name: &str,
    bytes: &[u8],
) -> Result<MediaSourceRef, String> {
    let staged = match source {
        MediaSourceRef::StagedRemoteProjectMedia { provenance, .. } => {
            vibez_project::project_format_v1::stage_remote_content(file_name, bytes, *provenance)
        }
        MediaSourceRef::ProjectMedia {
            provenance: Some(provenance),
            ..
        } if matches!(provenance.as_ref(), MediaProvenance::Remote { .. }) => {
            vibez_project::project_format_v1::stage_remote_content(file_name, bytes, *provenance)
        }
        MediaSourceRef::LocalFile { path }
        | MediaSourceRef::StagedProjectMedia {
            source_path: path, ..
        } => vibez_project::project_format_v1::stage_local_content(&path, file_name, bytes),
        MediaSourceRef::ProjectMedia { provenance, .. } => {
            let source_path = provenance
                .and_then(|provenance| match *provenance {
                    MediaProvenance::Local { source_path } => Some(source_path),
                    MediaProvenance::Remote { .. } => None,
                })
                .unwrap_or_else(|| std::path::PathBuf::from("Project Media"));
            vibez_project::project_format_v1::stage_local_content(&source_path, file_name, bytes)
        }
        MediaSourceRef::DropboxFile { .. } => {
            return Err("Slice to Drum Rack needs materialized Project Media".into());
        }
    };
    staged.map_err(|error| format!("Drum Rack media staging failed: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use vibez_core::track::{ClipFades, ClipGainDb, ClipPlaybackDirection};

    #[tokio::test]
    async fn preparation_flattens_reverse_loop_gain_and_fades_into_project_media() {
        let directory = tempfile::tempdir().unwrap();
        let source_path = directory.path().join("loop.wav");
        let audio = Arc::new(vibez_core::audio_buffer::DecodedAudio {
            channels: vec![(0..8).map(|frame| frame as f32).collect()],
            sample_rate: 48_000,
        });
        let clip = crate::state::UiClip {
            id: vibez_core::id::ClipId::new(),
            name: "Loop".into(),
            audio,
            source: Some(MediaSourceRef::LocalFile {
                path: source_path.clone(),
            }),
            position: 0,
            source_offset: 0,
            start_marker: 2,
            duration: 6,
            loop_enabled: true,
            loop_start: 2,
            loop_end: 4,
            gain_db: ClipGainDb::new(12.0).unwrap(),
            fades: ClipFades::new(1, 1, 6),
            playback_direction: ClipPlaybackDirection::Reverse,
            transient_markers: Default::default(),
            warp_markers: Default::default(),
            transpose: Default::default(),
            original_bpm: None,
            warped: false,
            warped_to_bpm: None,
            original_audio: None,
        };

        let prepared = prepare_drum_rack_audio_async(clip).await.unwrap();

        assert_eq!(prepared.audio.num_frames(), 6);
        let expected = [0.0, 1.0, 1.0, 1.0, 1.0, 0.0];
        for (actual, expected) in prepared.audio.channels[0].iter().zip(expected) {
            assert!((actual - expected).abs() < 1e-4);
        }
        assert!(matches!(
            prepared.source,
            MediaSourceRef::StagedProjectMedia { source_path: path, .. } if path == source_path
        ));
    }

    #[tokio::test]
    async fn prepared_warped_audio_and_pad_name_survive_save_and_reopen() {
        let directory = tempfile::tempdir().unwrap();
        let source_path = directory.path().join("warped.wav");
        let project_path = directory.path().join("warped-slices.vzp");
        let mut clip = crate::state::UiClip {
            id: vibez_core::id::ClipId::new(),
            name: "Warped Loop".into(),
            audio: Arc::new(vibez_core::audio_buffer::DecodedAudio {
                channels: vec![(0..8).map(|frame| frame as f32 / 10.0).collect()],
                sample_rate: 48_000,
            }),
            source: Some(MediaSourceRef::LocalFile { path: source_path }),
            position: 0,
            source_offset: 0,
            start_marker: 0,
            duration: 6,
            loop_enabled: false,
            loop_start: 0,
            loop_end: 6,
            gain_db: Default::default(),
            fades: Default::default(),
            playback_direction: ClipPlaybackDirection::Reverse,
            transient_markers: Default::default(),
            warp_markers: Default::default(),
            transpose: vibez_core::track::ClipTranspose::new(7),
            original_bpm: Some(120.0),
            warped: true,
            warped_to_bpm: Some(140.0),
            original_audio: None,
        };
        assert!(clip.warp_markers.add(4, 3, 0, 8, 6));
        let prepared = prepare_drum_rack_audio_async(clip).await.unwrap();
        let expected = Arc::clone(&prepared.audio);

        let mut track = vibez_core::track::TrackInfo::new("Slices 1");
        track.kind = vibez_core::midi::TrackKind::Midi;
        track.instrument = Some(vibez_core::midi::InstrumentKind::DrumRack);
        track.native_instrument = Some(vibez_core::track::InstrumentStateInfo::DrumRack {
            pads: vec![vibez_core::track::DrumPadState {
                name: Some("Warped Loop Slice 1".into()),
                source: Some(prepared.source),
                gain: 1.0,
                pan: 0.0,
                start: 0.0,
                end: 1.0,
                coarse_tune: 0,
                fine_tune: 0.0,
                one_shot: true,
                choke_group: None,
            }],
        });
        let project = vibez_project::Project {
            tracks: vec![track],
            ..vibez_project::Project::default()
        };
        vibez_project::project_format_v1::save_project_v1(&project_path, None, project).unwrap();

        let reopened = crate::app::async_helpers::load_project_async(project_path, None)
            .await
            .unwrap();
        assert!(reopened.warnings.is_empty());
        assert_eq!(reopened.drum_rack_pad_samples.len(), 1);
        let pad = &reopened.drum_rack_pad_samples[0];
        assert_eq!(pad.state.name.as_deref(), Some("Warped Loop Slice 1"));
        assert_eq!(pad.name, "Warped Loop Slice 1");
        assert_eq!(pad.audio.num_frames(), expected.num_frames());
        for (actual, expected) in pad.audio.channels[0].iter().zip(&expected.channels[0]) {
            assert!((actual - expected).abs() < 1e-4);
        }
    }
}
