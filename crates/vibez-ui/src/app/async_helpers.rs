//! Async decode/save/load/dropbox/export task helpers.
//! Split from app/mod.rs; re-exported into the app module.

use super::*;

pub(super) async fn decode_local_for_preview_async(
    path: PathBuf,
) -> Result<Arc<vibez_core::audio_buffer::DecodedAudio>, String> {
    let audio = decode_file_async(path).await?;
    Ok(Arc::new(audio))
}

pub(super) async fn decode_file_async(
    path: PathBuf,
) -> Result<vibez_core::audio_buffer::DecodedAudio, String> {
    tokio::task::spawn_blocking(move || {
        file_io::decode_audio_file(&path).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("decode task failed: {e}"))?
}

pub(super) async fn decode_and_stage_local_async(
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

pub(super) async fn save_project_async(
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

pub(super) async fn quantize_audio_clip_async(
    input: QuantizeInput,
) -> Result<crate::message::AudioQuantizeSuccess, String> {
    tokio::task::spawn_blocking(move || compute_audio_quantize(input))
        .await
        .map_err(|e| format!("quantize task failed: {e}"))?
}

pub(super) async fn detect_clip_bpm_async(
    audio: Arc<vibez_core::audio_buffer::DecodedAudio>,
    sample_rate: u32,
) -> Option<vibez_core::onset::BpmEstimate> {
    tokio::task::spawn_blocking(move || vibez_core::onset::detect_bpm(&audio, sample_rate))
        .await
        .unwrap_or(None)
}

pub(super) async fn warp_browser_audition_async(
    audio: Arc<vibez_core::audio_buffer::DecodedAudio>,
    source_bpm: f64,
    project_bpm: f64,
) -> Result<Arc<vibez_core::audio_buffer::DecodedAudio>, String> {
    tokio::task::spawn_blocking(move || {
        crate::warp::rewarp_for_load(&audio, source_bpm, project_bpm)
            .ok_or_else(|| "Could not create pitch-preserving WARP Audition".to_string())
    })
    .await
    .map_err(|error| format!("audition warp task failed: {error}"))?
}

pub(super) async fn prepare_browser_import_audio_async(
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
        duration: frames,
        loop_start: 0,
        loop_end: frames,
        clip_bpm: source_bpm,
        project_bpm,
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

pub(super) async fn auto_warp_clip_async(input: AutoWarpInput) -> crate::message::AutoWarpOutcome {
    use crate::message::AutoWarpOutcome;
    let audio_for_detect = Arc::clone(&input.audio);
    let sample_rate = input.sample_rate;
    let estimate = tokio::task::spawn_blocking(move || {
        vibez_core::onset::detect_bpm(&audio_for_detect, sample_rate)
    })
    .await
    .unwrap_or(None);
    let Some(est) = estimate else {
        return AutoWarpOutcome::NotDetected;
    };
    if est.confidence < input.confidence_threshold || est.bpm <= 0.0 {
        return AutoWarpOutcome::DetectedOnly {
            bpm: est.bpm,
            confidence: est.confidence,
        };
    }
    let num_frames = input.audio.num_frames();
    let warp_input = crate::warp::WarpClipInput {
        audio: input.audio,
        fields_frames: num_frames as u64,
        source_offset: 0,
        duration: num_frames as u64,
        loop_start: 0,
        loop_end: 0,
        clip_bpm: est.bpm,
        project_bpm: input.project_bpm,
    };
    match crate::warp::warp_clip_async(warp_input).await {
        Ok(success) => AutoWarpOutcome::Warped {
            confidence: est.confidence,
            success,
        },
        Err(_) => AutoWarpOutcome::DetectedOnly {
            bpm: est.bpm,
            confidence: est.confidence,
        },
    }
}

pub(super) async fn connect_dropbox_async(
    app_key: String,
) -> Result<(vibez_dropbox::AccountInfo, vibez_dropbox::Tokens), String> {
    let opener: Arc<dyn vibez_dropbox::BrowserOpener> =
        Arc::new(vibez_dropbox::SystemBrowserOpener);
    let tokens = vibez_dropbox::run_oauth_flow(&app_key, opener)
        .await
        .map_err(|e| e.to_string())?;
    let client = DropboxClient::new(app_key, tokens);
    let info = client.current_account().await.map_err(|e| e.to_string())?;
    let tokens = client.tokens().await;
    Ok((info, tokens))
}

/// Commit downloaded bytes to the Media Cache on a blocking thread; the
/// write can be multi-MB and must not stall the async executor.
pub(super) async fn write_cache_blocking(
    cache: &DropboxCache,
    entry: &DropboxEntry,
    bytes: Vec<u8>,
) -> Result<PathBuf, String> {
    let cache = cache.clone();
    let path_lower = entry.path_lower.clone();
    let revision = entry.rev.clone();
    tokio::task::spawn_blocking(move || cache.write(&path_lower, revision.as_deref(), &bytes))
        .await
        .map_err(|error| format!("Media Cache write task failed: {error}"))?
        .map_err(|error| format!("Media Cache write failed: {error}"))
}

pub(super) async fn fetch_dropbox_sample_async(
    client: Option<Arc<DropboxClient>>,
    cache: DropboxCache,
    entry: DropboxEntry,
) -> Result<
    (
        Arc<vibez_core::audio_buffer::DecodedAudio>,
        String,
        MediaSourceRef,
    ),
    String,
> {
    let _lease = cache.protect(&entry.path_lower, entry.rev.as_deref());
    let local = match cache
        .lookup(&entry.path_lower, entry.rev.as_deref())
        .map_err(|error| format!("Media Cache lookup failed: {error}"))?
    {
        Some(path) => path,
        None => {
            let client = client.ok_or_else(|| {
                "Reconnect Required · uncached Remote media cannot be imported".to_string()
            })?;
            let bytes = client.download(&entry.path_lower).await.map_err(|error| {
                format!("Remote materialization failed for {}: {error}", entry.name)
            })?;
            write_cache_blocking(&cache, &entry, bytes).await?
        }
    };
    let decode_path = local.clone();
    let decoded = tokio::task::spawn_blocking(move || {
        vibez_audio_io::file_io::decode_audio_file(&decode_path).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("decode task failed: {e}"))??;
    let staging_entry = entry.clone();
    let source = tokio::task::spawn_blocking(move || {
        vibez_project::project_format_v1::stage_remote_file(
            &local,
            &staging_entry.name,
            vibez_core::track::MediaProvenance::Remote {
                provider: crate::remote_provider::DROPBOX_PROVIDER_ID.into(),
                connection_id: crate::remote_provider::DROPBOX_CONNECTION_ID.into(),
                connection_name: Some(crate::remote_provider::DROPBOX_CONNECTION_NAME.into()),
                source_id: staging_entry.path_lower,
                source_path: staging_entry.path_display,
                revision: staging_entry.rev,
            },
        )
        .map_err(|error| format!("Remote Project Media staging failed: {error}"))
    })
    .await
    .map_err(|error| format!("Remote Project Media staging task failed: {error}"))??;
    Ok((Arc::new(decoded), entry.name, source))
}

pub(super) async fn materialize_remote_sample_async(
    client: Option<Arc<DropboxClient>>,
    cache: DropboxCache,
    entry: DropboxEntry,
    lease: vibez_dropbox::CacheLease,
    debounce: bool,
) -> Result<crate::message::RemoteMaterializedSample, String> {
    if debounce
        && cache
            .lookup(&entry.path_lower, entry.rev.as_deref())
            .map_err(|error| format!("Media Cache lookup failed: {error}"))?
            .is_none()
    {
        tokio::time::sleep(dropbox_io::REMOTE_SELECTION_DEBOUNCE).await;
    }

    let local = match cache
        .lookup(&entry.path_lower, entry.rev.as_deref())
        .map_err(|error| format!("Media Cache lookup failed: {error}"))?
    {
        Some(path) => path,
        None => {
            let client = client.ok_or_else(|| {
                "Reconnect Required · uncached Remote media cannot be materialized".to_string()
            })?;
            let bytes = client.download(&entry.path_lower).await.map_err(|error| {
                format!("Remote materialization failed for {}: {error}", entry.name)
            })?;
            write_cache_blocking(&cache, &entry, bytes).await?
        }
    };

    let revision = entry.rev.clone();
    let (decoded, metadata) = tokio::task::spawn_blocking(move || {
        let decoded = vibez_audio_io::file_io::decode_audio_file(&local)
            .map_err(|error| error.to_string())?;
        let estimate = vibez_core::onset::detect_bpm(&decoded, decoded.sample_rate);
        let bucket_count = 64usize;
        let frames_per_bucket = decoded.num_frames().max(1).div_ceil(bucket_count);
        let waveform_peaks = (0..bucket_count)
            .map(|bucket| {
                let start = bucket * frames_per_bucket;
                let end = (start + frames_per_bucket).min(decoded.num_frames());
                (0..decoded.num_channels())
                    .map(|channel| {
                        let (min, max) = decoded.peak_in_range(channel, start, end);
                        min.abs().max(max.abs())
                    })
                    .fold(0.0_f32, f32::max)
            })
            .collect();
        let metadata = vibez_dropbox::DerivedMetadata {
            provider_revision: revision,
            duration_seconds: decoded.duration_seconds(),
            channels: decoded.num_channels().try_into().unwrap_or(u16::MAX),
            sample_rate: decoded.sample_rate,
            bpm: estimate.map(|value| value.bpm),
            bpm_confidence: estimate.map(|value| value.confidence),
            waveform_peaks,
        };
        Ok::<_, String>((Arc::new(decoded), metadata))
    })
    .await
    .map_err(|error| format!("decode task failed: {error}"))??;
    cache
        .store_derived_metadata(&entry.path_lower, entry.rev.as_deref(), metadata.clone())
        .map_err(|error| format!("Derived Metadata save failed: {error}"))?;
    let source = MediaSourceRef::DropboxFile {
        path_lower: entry.path_lower,
        display_path: entry.path_display,
        rev: entry.rev,
    };
    Ok(crate::message::RemoteMaterializedSample {
        audio: decoded,
        name: entry.name,
        source,
        lease,
        metadata,
    })
}

pub(super) async fn export_async(
    request: vibez_engine::render::BounceRequest,
    wav_path: PathBuf,
) -> Result<PathBuf, String> {
    tokio::task::spawn_blocking(move || {
        let result = vibez_engine::render::render_offline(&request);
        vibez_audio_io::file_io::write_wav_file(&wav_path, &result.audio)
            .map_err(|e| format!("WAV write error: {e}"))?;
        Ok(wav_path)
    })
    .await
    .map_err(|err| format!("export task failed: {err}"))?
}

pub(super) async fn bounce_async(
    request: vibez_engine::render::BounceRequest,
    wav_path: PathBuf,
    clip_name: String,
    insert_position_samples: u64,
) -> Result<crate::message::BounceOutcome, String> {
    tokio::task::spawn_blocking(move || {
        let result = vibez_engine::render::render_offline(&request);
        vibez_audio_io::file_io::write_wav_file(&wav_path, &result.audio)
            .map_err(|e| format!("WAV write error: {e}"))?;
        Ok(crate::message::BounceOutcome {
            audio: Arc::new(result.audio),
            source: MediaSourceRef::LocalFile {
                path: wav_path.clone(),
            },
            path: wav_path,
            clip_name,
            insert_position_samples,
            warnings: result.warnings,
        })
    })
    .await
    .map_err(|err| format!("bounce task failed: {err}"))?
}

/// Finish a decoded clip for project load. The project file stores
/// the raw source reference, but a warped clip's geometry (duration /
/// offsets / loop bounds) is saved in warped-sample units, so the
/// deterministic stretch is re-applied here; otherwise every warped
/// clip reloads at its raw tempo and the whole project plays out of
/// sync. The stretch runs on a blocking thread (WSOLA over a whole
/// clip is CPU-heavy).
pub(super) async fn finish_loaded_clip(
    info: ClipInfo,
    raw: Arc<vibez_core::audio_buffer::DecodedAudio>,
) -> LoadedClipData {
    if info.warped {
        if let (Some(clip_bpm), Some(warped_to_bpm)) = (info.original_bpm, info.warped_to_bpm) {
            let stretch_src = Arc::clone(&raw);
            let warped = tokio::task::spawn_blocking(move || {
                crate::warp::rewarp_for_load(&stretch_src, clip_bpm, warped_to_bpm)
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
    LoadedClipData {
        info,
        audio: raw,
        original_audio: None,
    }
}

pub(super) async fn load_project_async(
    path: PathBuf,
    dropbox: Option<(Arc<DropboxClient>, DropboxCache)>,
) -> Result<ProjectLoadResult, String> {
    let load_path = path.clone();
    let (project, container_path) = tokio::task::spawn_blocking(move || {
        match vibez_project::project_format_v1::detect_project_format(&load_path)
            .map_err(|error| error.to_string())?
        {
            vibez_project::project_format_v1::ProjectFileFormat::V1 => {
                let container =
                    vibez_project::project_format_v1::ProjectContainer::open(&load_path)
                        .map_err(|error| error.to_string())?;
                let document = container
                    .load_document()
                    .map_err(|error| error.to_string())?;
                Ok((document.project, Some(load_path)))
            }
            vibez_project::project_format_v1::ProjectFileFormat::LegacyJson => {
                Project::load_from_file(&load_path)
                    .map(|project| (project, None))
                    .map_err(|error| error.to_string())
            }
        }
    })
    .await
    .map_err(|err| format!("load task failed: {err}"))??;

    let mut clips = Vec::new();
    let mut unresolved_clips = Vec::new();
    let mut sampler_samples = Vec::new();
    let mut drum_rack_pad_samples = Vec::new();
    let mut warnings = Vec::new();

    for clip in &project.clips {
        match clip.resolved_source().cloned() {
            Some(source) => match hydrate_saved_source(
                container_path.as_ref(),
                dropbox.as_ref(),
                &source,
                &clip.name,
            )
            .await
            {
                Ok(audio) => clips.push(finish_loaded_clip(clip.clone(), Arc::new(audio)).await),
                Err(err) => {
                    // The clip cannot play this session, but dropping it
                    // would also drop its source reference from the next
                    // save. Keep it so the media stays relinkable.
                    warnings.push(format!(
                        "Clip '{}' unavailable, kept for relink ({})",
                        clip.name, err
                    ));
                    unresolved_clips.push(clip.clone());
                }
            },
            None => warnings.push(format!(
                "Skipped clip '{}' (missing source reference)",
                clip.name
            )),
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
                                name: source.display_name(),
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

pub(super) async fn hydrate_saved_source(
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

pub(super) async fn decode_blocking(
    path: PathBuf,
) -> Result<vibez_core::audio_buffer::DecodedAudio, String> {
    tokio::task::spawn_blocking(move || {
        file_io::decode_audio_file(&path).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("decode task failed: {e}"))?
}

pub(super) async fn hydrate_dropbox_source(
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

pub(super) async fn scan_sample_root_async(
    root: PathBuf,
) -> Result<SampleLibraryScanResult, String> {
    tokio::task::spawn_blocking(move || scan_sample_root(&root))
        .await
        .map_err(|err| format!("scan task failed: {err}"))?
}
