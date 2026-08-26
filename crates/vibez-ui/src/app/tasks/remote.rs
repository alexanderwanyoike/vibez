//! Remote provider connection, caching, and sample materialization tasks.

use super::super::dropbox_io;
use super::browser::analyse_browser_audio_with_cached_metadata_async;
use std::path::PathBuf;
use std::sync::Arc;
use vibez_core::track::MediaSourceRef;
use vibez_dropbox::{DropboxCache, DropboxClient, DropboxEntry};

pub(in crate::app) async fn connect_dropbox_async(
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

/// Hand a URL to the system browser on a blocking thread.
///
/// Reuses the opener vibez-dropbox already owns for its OAuth flow rather
/// than taking a second browser dependency. `BrowserOpener::open` waits for
/// the platform launcher to return, which must never happen on the update
/// loop: on a cold browser start that stall would freeze the whole UI.
pub(in crate::app) async fn open_url_async(url: &'static str) -> Result<(), String> {
    let opener: Arc<dyn vibez_dropbox::BrowserOpener> =
        Arc::new(vibez_dropbox::SystemBrowserOpener);
    tokio::task::spawn_blocking(move || opener.open(url))
        .await
        .map_err(|error| format!("Browser task failed: {error}"))?
        .map_err(|error| error.to_string())
}

/// Commit downloaded bytes to the Media Cache on a blocking thread; the
/// write can be multi-MB and must not stall the async executor.
pub(in crate::app) async fn write_cache_blocking(
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

pub(in crate::app) async fn fetch_dropbox_sample_async(
    client: Option<Arc<DropboxClient>>,
    cache: DropboxCache,
    entry: DropboxEntry,
) -> Result<(crate::message::AnalysedBrowserAudio, String, MediaSourceRef), String> {
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
    let metadata_cache = cache.clone();
    let metadata_path = entry.path_lower.clone();
    let metadata_revision = entry.rev.clone();
    let cached_metadata = tokio::task::spawn_blocking(move || {
        metadata_cache.derived_metadata(&metadata_path, metadata_revision.as_deref())
    })
    .await
    .map_err(|error| format!("Derived Metadata lookup task failed: {error}"))?
    .map_err(|error| format!("Derived Metadata lookup failed: {error}"))?;
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
    let analysed = analyse_browser_audio_with_cached_metadata_async(
        decoded,
        entry.name.clone(),
        cached_metadata,
    )
    .await?;
    Ok((analysed, entry.name, source))
}

pub(in crate::app) async fn materialize_remote_sample_async(
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
