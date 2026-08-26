//! Project save, load, source hydration, and load-error classification tasks.

use crate::message::{
    LoadedClipData, LoadedDrumRackPadData, LoadedSamplerData, ProjectLoadResult, ProjectSaveResult,
};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use vibez_audio_io::file_io;
use vibez_core::track::{ClipInfo, InstrumentStateInfo, MediaSourceRef};
use vibez_dropbox::{DropboxCache, DropboxClient, DropboxEntry};
use vibez_project::Project;

pub(in crate::app) async fn save_project_async(
    path: PathBuf,
    source_path: Option<PathBuf>,
    project: Project,
) -> Result<ProjectSaveResult, String> {
    tokio::task::spawn_blocking(move || {
        let is_v1_destination = path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("vzp"));
        if is_v1_destination {
            let v1_source = source_path.as_deref().filter(|source| {
                vibez_project::project_format_v1::detect_project_format(source).is_ok_and(
                    |format| format == vibez_project::project_format_v1::ProjectFileFormat::V1,
                )
            });
            let saved =
                vibez_project::project_format_v1::save_project_v1(&path, v1_source, project)
                    .map_err(|error| error.to_string())?;
            Ok(ProjectSaveResult {
                path,
                project: saved.project,
                observation: Some(saved.observation),
            })
        } else {
            // Legacy JSON has no Project Media table; transient staging
            // references must resolve back to durable Source Storage
            // identity or they dangle once the staging cache is swept.
            let mut project = project;
            vibez_project::project_format_v1::strip_staged_sources(&mut project);
            project
                .save_to_file(&path)
                .map_err(|error| error.to_string())?;
            Ok(ProjectSaveResult {
                path,
                project,
                observation: None,
            })
        }
    })
    .await
    .map_err(|err| format!("save task failed: {err}"))?
}

/// Finish a decoded clip for project load. The project file stores
/// the raw source reference, but a warped clip's geometry (duration /
/// offsets / loop bounds) is saved in warped-sample units, so the
/// deterministic stretch is re-applied here; otherwise every warped
/// clip reloads at its raw tempo and the whole project plays out of
/// sync. The stretch runs on a blocking thread (WSOLA over a whole
/// clip is CPU-heavy).
pub(in crate::app) async fn finish_loaded_clip(
    info: ClipInfo,
    raw: Arc<vibez_core::audio_buffer::DecodedAudio>,
) -> LoadedClipData {
    let transpose = info.transpose.semitones();
    if info.warped {
        if let (Some(clip_bpm), Some(warped_to_bpm)) = (info.original_bpm, info.warped_to_bpm) {
            let stretch_src = Arc::clone(&raw);
            let warped = tokio::task::spawn_blocking(move || {
                crate::warp::rewarp_for_load(&stretch_src, clip_bpm, warped_to_bpm, transpose)
            })
            .await
            .unwrap_or(None);
            if let Some(warped) = warped {
                return LoadedClipData {
                    info,
                    audio: warped,
                    original_audio: Some(raw),
                };
            }
        }
    }
    if transpose != 0 {
        let source = Arc::clone(&raw);
        let rendered = tokio::task::spawn_blocking(move || {
            Arc::new(
                vibez_dsp::time_stretch::pitch_preserving_stretch_transposed(
                    &source,
                    source.num_frames(),
                    f32::from(transpose),
                ),
            )
        })
        .await
        .unwrap_or_else(|_| Arc::clone(&raw));
        return LoadedClipData {
            info,
            audio: rendered,
            original_audio: Some(raw),
        };
    }
    LoadedClipData {
        info,
        audio: raw,
        original_audio: None,
    }
}

/// Run CPU-bound work on the tokio blocking pool, labelling join failures.
/// Every off-UI-thread hop in the app routes through here so the idiom (and
/// its error shape) exists exactly once.
/// One rule for what counts as a missing project: only a confirmed
/// `NotFound` io error prunes a Recent Projects entry; everything else is
/// treated as transient.
fn classify_load_error(
    io_error: Option<&std::io::Error>,
    message: String,
) -> crate::message::ProjectLoadError {
    if io_error.is_some_and(|error| error.kind() == std::io::ErrorKind::NotFound) {
        crate::message::ProjectLoadError::missing_project(message)
    } else {
        crate::message::ProjectLoadError::other(message)
    }
}

fn classify_project_format_load_error(
    error: vibez_project::project_format_v1::ProjectFormatError,
) -> crate::message::ProjectLoadError {
    let io_error = match &error {
        vibez_project::project_format_v1::ProjectFormatError::Io(io_error) => Some(io_error),
        _ => None,
    };
    classify_load_error(io_error, error.to_string())
}

fn classify_legacy_project_load_error(
    error: vibez_project::ProjectError,
) -> crate::message::ProjectLoadError {
    let io_error = match &error {
        vibez_project::ProjectError::Io(io_error) => Some(io_error),
        _ => None,
    };
    classify_load_error(io_error, error.to_string())
}

pub(in crate::app) async fn load_project_async(
    path: PathBuf,
    dropbox: Option<(Arc<DropboxClient>, DropboxCache)>,
) -> Result<ProjectLoadResult, crate::message::ProjectLoadError> {
    let load_path = path.clone();
    let (project, container_path) = tokio::task::spawn_blocking(move || {
        match vibez_project::project_format_v1::detect_project_format(&load_path)
            .map_err(classify_project_format_load_error)?
        {
            vibez_project::project_format_v1::ProjectFileFormat::V1 => {
                let container =
                    vibez_project::project_format_v1::ProjectContainer::open(&load_path)
                        .map_err(classify_project_format_load_error)?;
                let document = container
                    .load_document()
                    .map_err(classify_project_format_load_error)?;
                Ok((document.project, Some(load_path)))
            }
            vibez_project::project_format_v1::ProjectFileFormat::LegacyJson => {
                Project::load_from_file(&load_path)
                    .map(|project| (project, None))
                    .map_err(classify_legacy_project_load_error)
            }
        }
    })
    .await
    .map_err(|err| crate::message::ProjectLoadError::other(format!("load task failed: {err}")))??;

    let mut clips = Vec::new();
    let mut unresolved_clips = Vec::new();
    let mut sampler_samples = Vec::new();
    let mut drum_rack_pad_samples = Vec::new();
    let mut warnings = Vec::new();

    for (location, timeline) in project.timelines() {
        for clip in &timeline.clips {
            match clip.resolved_source().cloned() {
                Some(source) => match hydrate_saved_source(
                    container_path.as_ref(),
                    dropbox.as_ref(),
                    &source,
                    &clip.name,
                )
                .await
                {
                    Ok(audio) => clips.push(crate::message::LoadedTimelineClip {
                        location,
                        clip: finish_loaded_clip(clip.clone(), Arc::new(audio)).await,
                    }),
                    Err(err) => {
                        warnings.push(format!(
                            "Clip '{}' unavailable, kept for relink ({})",
                            clip.name, err
                        ));
                        unresolved_clips.push(crate::message::UnresolvedTimelineClip {
                            location,
                            info: clip.clone(),
                        });
                    }
                },
                None => warnings.push(format!(
                    "Skipped clip '{}' (missing source reference)",
                    clip.name
                )),
            }
        }
    }

    for track in &project.tracks {
        if let Some(native) = &track.native_instrument {
            match native {
                InstrumentStateInfo::Sampler {
                    source: Some(source),
                    ..
                } => match hydrate_saved_source(
                    container_path.as_ref(),
                    dropbox.as_ref(),
                    source,
                    &track.name,
                )
                .await
                {
                    Ok(audio) => sampler_samples.push(LoadedSamplerData {
                        track_id: track.id,
                        source: source.clone(),
                        audio: Arc::new(audio),
                        name: source.display_name(),
                    }),
                    Err(err) => warnings.push(format!(
                        "Skipped sampler source on '{}' ({})",
                        track.name, err
                    )),
                },
                InstrumentStateInfo::DrumRack { pads } => {
                    for (pad_index, pad) in pads.iter().enumerate() {
                        let Some(source) = &pad.source else {
                            continue;
                        };
                        let label = format!("drum pad {} on '{}'", pad_index + 1, track.name);
                        match hydrate_saved_source(
                            container_path.as_ref(),
                            dropbox.as_ref(),
                            source,
                            &label,
                        )
                        .await
                        {
                            Ok(audio) => drum_rack_pad_samples.push(LoadedDrumRackPadData {
                                track_id: track.id,
                                pad_index,
                                source: source.clone(),
                                audio: Arc::new(audio),
                                name: pad.name.clone().unwrap_or_else(|| source.display_name()),
                                state: pad.clone(),
                            }),
                            Err(err) => warnings.push(format!("Skipped {label} ({err})")),
                        }
                    }
                }
                _ => {}
            }
        }
    }

    Ok(ProjectLoadResult {
        path,
        project,
        clips,
        unresolved_clips,
        sampler_samples,
        drum_rack_pad_samples,
        warnings,
    })
}

pub(in crate::app) async fn hydrate_saved_source(
    container_path: Option<&PathBuf>,
    dropbox: Option<&(Arc<DropboxClient>, DropboxCache)>,
    source: &MediaSourceRef,
    label: &str,
) -> Result<vibez_core::audio_buffer::DecodedAudio, String> {
    match source {
        MediaSourceRef::LocalFile { path }
        | MediaSourceRef::StagedProjectMedia {
            staging_path: path, ..
        }
        | MediaSourceRef::StagedRemoteProjectMedia {
            staging_path: path, ..
        } => decode_blocking(path.clone()).await,
        MediaSourceRef::ProjectMedia { id, file_name, .. } => {
            let container_path = container_path
                .cloned()
                .ok_or_else(|| format!("{label} has Project Media without a V1 container"))?;
            let id = id.clone();
            let extension = Path::new(file_name)
                .extension()
                .map(|value| value.to_string_lossy().into_owned());
            tokio::task::spawn_blocking(move || {
                let container =
                    vibez_project::project_format_v1::ProjectContainer::open(container_path)
                        .map_err(|error| error.to_string())?;
                let bytes = container
                    .read_media(&id)
                    .map_err(|error| error.to_string())?;
                vibez_audio_io::file_io::decode_audio_cursor(
                    std::io::Cursor::new(bytes),
                    extension.as_deref(),
                )
                .map_err(|error| error.to_string())
            })
            .await
            .map_err(|error| format!("Project Media decode task failed: {error}"))?
        }
        MediaSourceRef::DropboxFile { .. } => hydrate_dropbox_source(dropbox, source, label).await,
    }
}

pub(in crate::app) async fn decode_blocking(
    path: PathBuf,
) -> Result<vibez_core::audio_buffer::DecodedAudio, String> {
    tokio::task::spawn_blocking(move || {
        file_io::decode_audio_file(&path).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("decode task failed: {e}"))?
}

pub(in crate::app) async fn hydrate_dropbox_source(
    dropbox: Option<&(Arc<DropboxClient>, DropboxCache)>,
    source: &MediaSourceRef,
    label: &str,
) -> Result<vibez_core::audio_buffer::DecodedAudio, String> {
    let MediaSourceRef::DropboxFile {
        path_lower,
        display_path,
        rev,
    } = source
    else {
        return Err(format!(
            "Skipped '{label}' (expected Dropbox source reference)"
        ));
    };
    let Some((client, cache)) = dropbox else {
        return Err(format!(
            "Skipped '{label}' (not connected to Dropbox - reconnect in Settings)"
        ));
    };
    let file_name = display_path
        .rsplit_once('/')
        .map(|(_, n)| n.to_string())
        .unwrap_or_else(|| display_path.clone());
    let entry = DropboxEntry {
        path_lower: path_lower.clone(),
        path_display: display_path.clone(),
        name: file_name,
        is_folder: false,
        rev: rev.clone(),
        size: None,
    };
    let local_path = client
        .download_to_cache(&entry, cache)
        .await
        .map_err(|e| format!("Skipped '{label}' ({e})"))?;
    decode_blocking(local_path)
        .await
        .map_err(|e| format!("Skipped '{label}' ({e})"))
}

#[cfg(test)]
mod project_load_error_tests {
    use super::*;

    #[tokio::test]
    async fn a_missing_top_level_project_is_classified_for_recent_path_pruning() {
        let directory = tempfile::tempdir().unwrap();
        let missing = directory.path().join("removed.vzp");

        let error = load_project_async(missing, None).await.unwrap_err();

        assert!(matches!(
            error,
            crate::message::ProjectLoadError::MissingProject(_)
        ));
    }

    #[tokio::test]
    async fn an_existing_but_invalid_project_is_not_classified_as_missing() {
        let directory = tempfile::tempdir().unwrap();
        let invalid = directory.path().join("invalid.vzp");
        std::fs::write(&invalid, "not a Vibez project").unwrap();

        let error = load_project_async(invalid, None).await.unwrap_err();

        assert!(matches!(error, crate::message::ProjectLoadError::Other(_)));
    }
}
