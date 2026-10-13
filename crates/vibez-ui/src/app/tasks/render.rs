//! Offline export and bounce render tasks.

use std::path::PathBuf;
use std::sync::Arc;
use vibez_core::track::MediaSourceRef;

pub(in crate::app) async fn export_async(
    request: vibez_engine::render::BounceRequest,
    plugins: vibez_engine::render::OfflinePlugins,
    wav_path: PathBuf,
    progress: Arc<std::sync::atomic::AtomicU8>,
    plugin_return: std::sync::mpsc::Sender<vibez_engine::render::OfflinePlugins>,
) -> Result<PathBuf, String> {
    render_async(request, plugins, wav_path.clone(), progress, plugin_return).await?;
    Ok(wav_path)
}

async fn render_async(
    request: vibez_engine::render::BounceRequest,
    mut plugins: vibez_engine::render::OfflinePlugins,
    wav_path: PathBuf,
    progress: Arc<std::sync::atomic::AtomicU8>,
    plugin_return: std::sync::mpsc::Sender<vibez_engine::render::OfflinePlugins>,
) -> Result<vibez_engine::render::BounceResult, String> {
    tokio::task::spawn_blocking(move || {
        use std::sync::atomic::Ordering;

        let outcome = (|| {
            let result = vibez_engine::render::render_offline_with_plugins(
                &request,
                &mut plugins,
                |percent| {
                    // Plugin preflight occupies 0–10%; audio rendering occupies
                    // 10–99%, with 100 reserved for the committed destination.
                    let scaled = 10 + ((percent as u16 * 89) / 100) as u8;
                    progress.store(scaled.min(99), Ordering::Relaxed);
                },
            )?;
            if !result.warnings.is_empty() {
                return Err(format!(
                    "render was incomplete: {}",
                    result.warnings.join("; ")
                ));
            }
            let temporary = temporary_export_path(&wav_path);
            if let Err(error) = vibez_audio_io::file_io::write_wav_file(&temporary, &result.audio) {
                let _ = std::fs::remove_file(&temporary);
                return Err(format!("WAV write error: {error}"));
            }
            if let Err(error) = std::fs::rename(&temporary, &wav_path) {
                let _ = std::fs::remove_file(&temporary);
                return Err(format!("could not commit destination WAV: {error}"));
            }
            progress.store(100, Ordering::Relaxed);
            Ok(result)
        })();
        // Plugin teardown is main-thread-affine for JUCE-based CLAP/VST3
        // devices. Return the instances to App; dropping them here would
        // violate the plugin contract after an otherwise successful export.
        let _ = plugin_return.send(plugins);
        outcome
    })
    .await
    .map_err(|err| format!("export task failed: {err}"))?
}

fn temporary_export_path(destination: &std::path::Path) -> PathBuf {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let file_name = destination
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "vibez-export.wav".to_string());
    destination.with_file_name(format!(".{file_name}.{nonce}.part"))
}

pub(in crate::app) async fn bounce_async(
    request: vibez_engine::render::BounceRequest,
    plugins: vibez_engine::render::OfflinePlugins,
    wav_path: PathBuf,
    clip_name: String,
    insert_position_samples: u64,
    progress: Arc<std::sync::atomic::AtomicU8>,
    plugin_return: std::sync::mpsc::Sender<vibez_engine::render::OfflinePlugins>,
) -> Result<crate::message::BounceOutcome, String> {
    let result = render_async(request, plugins, wav_path.clone(), progress, plugin_return).await?;
    Ok(crate::message::BounceOutcome {
        audio: Arc::new(result.audio),
        source: MediaSourceRef::LocalFile {
            path: wav_path.clone(),
        },
        path: wav_path,
        clip_name,
        insert_position_samples,
    })
}

#[cfg(test)]
mod export_tests {
    use super::*;
    use vibez_core::constants::{DEFAULT_TRACK_GAIN, DEFAULT_TRACK_PAN};
    use vibez_core::effect::PluginDeviceInfo;
    use vibez_core::id::TrackId;
    use vibez_core::midi::TrackKind;
    use vibez_core::track::TrackInfo;

    #[tokio::test]
    async fn failed_plugin_render_never_creates_the_destination_wav() {
        let track_id = TrackId::new();
        let track = TrackInfo {
            id: track_id,
            name: "Bass".into(),
            gain: DEFAULT_TRACK_GAIN,
            pan: DEFAULT_TRACK_PAN,
            mute: false,
            solo: false,
            audio_input_route: Default::default(),
            input_monitoring: Default::default(),
            swing_offset: None,
            effects: Vec::new(),
            kind: TrackKind::Midi,
            color_index: 0,
            instrument: None,
            native_instrument: None,
            plugin_instrument: Some(PluginDeviceInfo {
                format: "clap".into(),
                uid: "org.example.bass".into(),
                path: "/missing/Bass.clap".into(),
                name: "Bass Plugin".into(),
                state_b64: None,
            }),
            automation: Vec::new(),
            sends: Vec::new(),
        };
        let request = vibez_engine::render::BounceRequest {
            tracks: vec![track],
            master: None,
            buses: Vec::new(),
            audio_clips: Vec::new(),
            note_clips: Vec::new(),
            clip_audio: Default::default(),
            sampler_audio: Default::default(),
            drum_pad_audio: Default::default(),
            mode: vibez_engine::render::BounceMode::Master,
            range_samples: (0, 512),
            bpm: 120.0,
            sample_rate: 44_100,
            swing: vibez_core::perform::SwingAmount::STRAIGHT,
        };
        let directory = tempfile::tempdir().unwrap();
        let destination = directory.path().join("must-not-exist.wav");
        let progress = Arc::new(std::sync::atomic::AtomicU8::new(0));
        let (plugin_return, returned_plugins) = std::sync::mpsc::channel();

        let error = export_async(
            request,
            vibez_engine::render::OfflinePlugins::default(),
            destination.clone(),
            progress,
            plugin_return,
        )
        .await
        .unwrap_err();
        drop(returned_plugins.recv().unwrap());

        assert!(error.contains("Bass Plugin"));
        assert!(!destination.exists());
        assert_eq!(
            std::fs::read_dir(directory.path()).unwrap().count(),
            0,
            "failed export must clean up every temporary file"
        );
    }
    #[tokio::test]
    async fn missing_detector_media_fails_bounce_without_committing_audio() {
        use vibez_core::{
            effect::{EffectInfo, EffectType},
            id::{ClipId, EffectId},
            routing::{ExternalInputId, SidechainAssignment, SourceTap},
        };
        let kick = TrackId::new();
        let bass = TrackId::new();
        let channel = |id, name: &str| TrackInfo {
            id,
            name: name.into(),
            gain: DEFAULT_TRACK_GAIN,
            pan: DEFAULT_TRACK_PAN,
            mute: false,
            solo: false,
            audio_input_route: Default::default(),
            input_monitoring: Default::default(),
            swing_offset: None,
            effects: vec![],
            kind: TrackKind::Audio,
            color_index: 0,
            instrument: None,
            native_instrument: None,
            plugin_instrument: None,
            automation: vec![],
            sends: vec![],
        };
        let mut receiver = channel(bass, "Bass");
        receiver.effects.push(EffectInfo {
            id: EffectId::new(),
            effect_type: EffectType::Compressor,
            bypass: false,
            params: vec![],
            plugin: None,
            inactive_sidechains: vec![],
            sidechains: vec![SidechainAssignment {
                input_id: ExternalInputId(0),
                input_name: "Sidechain".into(),
                source: kick,
                source_name: "Kick".into(),
                tap: SourceTap::BeforeEffects,
            }],
        });
        let clip = serde_json::from_value(
            serde_json::json!({"id": ClipId::new(), "track_id": kick, "name": "Missing ghost kick",
            "position": 0, "source_offset": 0, "duration": 128, "file_path": "/missing/kick.wav"}),
        )
        .unwrap();
        let request = vibez_engine::render::BounceRequest {
            tracks: vec![channel(kick, "Kick"), receiver],
            master: None,
            buses: vec![],
            audio_clips: vec![clip],
            note_clips: vec![],
            clip_audio: Default::default(),
            sampler_audio: Default::default(),
            drum_pad_audio: Default::default(),
            mode: vibez_engine::render::BounceMode::Track(bass),
            range_samples: (0, 128),
            bpm: 120.0,
            sample_rate: 44_100,
            swing: vibez_core::perform::SwingAmount::STRAIGHT,
        };
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("missing-detector.wav");
        let (returned, rx) = std::sync::mpsc::channel();
        let error = bounce_async(
            request,
            Default::default(),
            path.clone(),
            "Bass".into(),
            0,
            Arc::new(std::sync::atomic::AtomicU8::new(0)),
            returned,
        )
        .await
        .unwrap_err();
        drop(rx.recv().unwrap());
        assert!(error.contains("Missing ghost kick"), "{error}");
        assert!(!path.exists());
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 0);
    }
}
