//! Browser media decode, loop analysis, staging, audition, and library scanning tasks.

use super::super::audio_tasks::scan_sample_root;
use crate::message::SampleLibraryScanResult;
use std::path::PathBuf;
use std::sync::Arc;
use vibez_audio_io::file_io;
use vibez_core::track::MediaSourceRef;

pub(in crate::app) async fn decode_local_for_preview_async(
    path: PathBuf,
) -> Result<crate::message::AnalysedBrowserAudio, String> {
    tokio::task::spawn_blocking(move || {
        let name = path
            .file_name()
            .map(|value| value.to_string_lossy().into_owned())
            .unwrap_or_default();
        let audio = file_io::decode_audio_file(&path).map_err(|error| error.to_string())?;
        let loop_fit = crate::warp::analyse_loop_tempo(&audio, &name);
        Ok(crate::message::AnalysedBrowserAudio {
            audio: Arc::new(audio),
            loop_fit,
        })
    })
    .await
    .map_err(|error| format!("preview decode task failed: {error}"))?
}

pub(in crate::app) async fn decode_file_async(
    path: PathBuf,
) -> Result<vibez_core::audio_buffer::DecodedAudio, String> {
    tokio::task::spawn_blocking(move || {
        file_io::decode_audio_file(&path).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("decode task failed: {e}"))?
}

pub(in crate::app) async fn analyse_browser_audio_async(
    audio: vibez_core::audio_buffer::DecodedAudio,
    source_name: String,
) -> Result<crate::message::AnalysedBrowserAudio, String> {
    analyse_browser_audio_with_cached_metadata_async(audio, source_name, None).await
}

pub(in crate::app) async fn analyse_browser_audio_with_cached_metadata_async(
    audio: vibez_core::audio_buffer::DecodedAudio,
    source_name: String,
    cached_metadata: Option<vibez_dropbox::DerivedMetadata>,
) -> Result<crate::message::AnalysedBrowserAudio, String> {
    tokio::task::spawn_blocking(move || {
        let loop_fit = match cached_metadata {
            Some(metadata) => crate::warp::fit_loop_tempo(
                audio.num_frames(),
                audio.sample_rate as f64,
                &source_name,
                metadata.bpm,
            ),
            None => crate::warp::analyse_loop_tempo(&audio, &source_name),
        };
        crate::message::AnalysedBrowserAudio {
            audio: Arc::new(audio),
            loop_fit,
        }
    })
    .await
    .map_err(|error| format!("Browser analysis task failed: {error}"))
}

pub(in crate::app) async fn decode_and_analyse_async(
    path: PathBuf,
    source_name: String,
) -> Result<crate::message::AnalysedBrowserAudio, String> {
    let audio = decode_file_async(path).await?;
    analyse_browser_audio_async(audio, source_name).await
}

pub(in crate::app) async fn decode_analyse_and_stage_local_async(
    path: PathBuf,
) -> Result<(crate::message::AnalysedBrowserAudio, MediaSourceRef), String> {
    let name = path
        .file_name()
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_default();
    let (audio, source) = decode_and_stage_local_async(path).await?;
    let analysed = analyse_browser_audio_async(audio, name).await?;
    Ok((analysed, source))
}

pub(in crate::app) async fn decode_and_stage_local_async(
    path: PathBuf,
) -> Result<(vibez_core::audio_buffer::DecodedAudio, MediaSourceRef), String> {
    tokio::task::spawn_blocking(move || {
        // One read feeds both the decoder and the staging copy, so the
        // engine can never play different bytes than the project commits
        // (the file could be replaced between two independent reads).
        let content = std::fs::read(&path).map_err(|error| error.to_string())?;
        let extension = path
            .extension()
            .map(|value| value.to_string_lossy().into_owned());
        let audio = file_io::decode_audio_cursor(
            std::io::Cursor::new(content.clone()),
            extension.as_deref(),
        )
        .map_err(|error| error.to_string())?;
        let file_name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "project-media".to_string());
        let source =
            vibez_project::project_format_v1::stage_local_content(&path, &file_name, &content)
                .map_err(|error| error.to_string())?;
        Ok((audio, source))
    })
    .await
    .map_err(|error| format!("decode/stage task failed: {error}"))?
}

pub(in crate::app) async fn warp_browser_audition_async(
    audio: Arc<vibez_core::audio_buffer::DecodedAudio>,
    source_bpm: f64,
    project_bpm: f64,
) -> Result<Arc<vibez_core::audio_buffer::DecodedAudio>, String> {
    tokio::task::spawn_blocking(move || {
        crate::warp::rewarp_for_load(&audio, source_bpm, project_bpm, 0)
            .ok_or_else(|| "Could not create pitch-preserving WARP Audition".to_string())
    })
    .await
    .map_err(|error| format!("audition warp task failed: {error}"))?
}

pub(in crate::app) async fn prepare_browser_import_audio_async(
    target: crate::message::BrowserImportTarget,
    treatment: crate::state::AuditionImportInput,
    raw: Arc<vibez_core::audio_buffer::DecodedAudio>,
    source: MediaSourceRef,
    project_bpm: f64,
) -> Result<
    (
        Arc<vibez_core::audio_buffer::DecodedAudio>,
        Option<Arc<vibez_core::audio_buffer::DecodedAudio>>,
        MediaSourceRef,
    ),
    String,
> {
    if treatment.mode == crate::state::AuditionMode::Raw {
        return Ok((raw, None, source));
    }
    let source_bpm = treatment
        .source_bpm
        .filter(|bpm| bpm.is_finite() && *bpm > 0.0)
        .ok_or_else(|| "Confirm a positive source BPM before WARP import".to_string())?;
    let frames = raw.num_frames() as u64;
    let success = crate::warp::warp_clip_async(crate::warp::WarpClipInput {
        audio: Arc::clone(&raw),
        fields_frames: frames,
        source_offset: 0,
        start_marker: 0,
        duration: frames,
        loop_start: 0,
        loop_end: frames,
        clip_bpm: source_bpm,
        project_bpm,
        transpose_semitones: 0,
    })
    .await?;
    let device_target = matches!(
        target,
        crate::message::BrowserImportTarget::Sampler(_)
            | crate::message::BrowserImportTarget::DrumRackPad { .. }
    );
    if !device_target {
        return Ok((success.audio, Some(success.original_audio), source));
    }

    let rendered = Arc::clone(&success.audio);
    let staged = tokio::task::spawn_blocking(move || {
        let original_name = source.display_name();
        let stem = std::path::Path::new(&original_name)
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .filter(|stem| !stem.is_empty())
            .unwrap_or_else(|| "sample".into());
        let file_name = format!("{stem}-warped.wav");
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default();
        let temporary = std::env::temp_dir().join(format!(
            "vibez-warp-import-{}-{nonce}.wav",
            std::process::id()
        ));
        vibez_audio_io::file_io::write_wav_file(&temporary, &rendered)
            .map_err(|error| error.to_string())?;
        let content = std::fs::read(&temporary).map_err(|error| error.to_string())?;
        let _ = std::fs::remove_file(&temporary);
        match source {
            MediaSourceRef::StagedProjectMedia { source_path, .. }
            | MediaSourceRef::LocalFile { path: source_path } => {
                vibez_project::project_format_v1::stage_local_content(
                    &source_path,
                    &file_name,
                    &content,
                )
                .map_err(|error| error.to_string())
            }
            MediaSourceRef::StagedRemoteProjectMedia { provenance, .. } => match *provenance {
                vibez_core::track::MediaProvenance::Remote {
                    provider,
                    connection_id,
                    connection_name,
                    source_id,
                    source_path,
                    revision,
                } => vibez_project::project_format_v1::stage_remote_content(
                    &file_name,
                    &content,
                    vibez_core::track::MediaProvenance::Remote {
                        provider,
                        connection_id,
                        connection_name,
                        source_id,
                        source_path,
                        revision,
                    },
                )
                .map_err(|error| error.to_string()),
                vibez_core::track::MediaProvenance::Local { .. } => {
                    Err("Remote staging carried Local provenance".to_string())
                }
            },
            _ => Err("WARP device import requires materialized Project Media".to_string()),
        }
    })
    .await
    .map_err(|error| format!("WARP device staging task failed: {error}"))??;
    Ok((success.audio, None, staged))
}

pub(in crate::app) async fn scan_sample_root_async(
    root: PathBuf,
) -> Result<SampleLibraryScanResult, String> {
    tokio::task::spawn_blocking(move || scan_sample_root(&root))
        .await
        .map_err(|err| format!("scan task failed: {err}"))?
}
