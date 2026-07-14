use std::path::{Path, PathBuf};
use std::sync::Arc;

use iced::{Subscription, Task, Theme};

use crate::domains::browser::BrowserMsg;
use crate::domains::view::ViewMsg;
use rtrb::{Consumer, Producer};
use vibez_audio_io::audio_stream::AudioOutputStream;
use vibez_audio_io::file_io;
use vibez_core::constants::UI_TICK_MS;
use vibez_core::track::{ClipInfo, InstrumentStateInfo, MediaSourceRef};
use vibez_dropbox::{
    load_app_key_with_env_override, DropboxCache, DropboxClient, DropboxEntry, DropboxSettings,
};
use vibez_engine::commands::EngineCommand;
use vibez_engine::engine::AudioEngine;
use vibez_engine::events::EngineEvent;
use vibez_plugin_host::gui::PluginGuiKey;

use crate::services::plugin_loader::{PluginInstrumentLoadResult, PluginLoadResult};
use vibez_project::Project;

use crate::icons;
use crate::message::{
    LoadedClipData, LoadedDrumRackPadData, LoadedSamplerData, Message, ProjectLoadResult,
    ProjectSaveResult, SampleLibraryScanResult,
};
use crate::plugin_window::{PluginRawPtr, PluginWindowManager};
use crate::state::AppState;
use crate::theme as th;
use crate::ui_settings::UiSettings;

struct App {
    state: AppState,
    cmd_tx: Option<Producer<EngineCommand>>,
    event_rx: Option<Consumer<EngineEvent>>,
    /// Post-effects mono samples from the engine's spectrum tap,
    /// feeding the EQ analyser.
    spectrum_rx: Option<Consumer<f32>>,
    /// Track the engine tap currently points at, so tick can retarget
    /// it when the selection moves.
    spectrum_tap: Option<vibez_core::id::TrackId>,
    _stream: Option<AudioOutputStream>,
    // Channels for receiving loaded plugins from background threads
    plugin_effect_rx: std::sync::mpsc::Receiver<PluginLoadResult>,
    plugin_effect_tx: std::sync::mpsc::Sender<PluginLoadResult>,
    plugin_instrument_rx: std::sync::mpsc::Receiver<PluginInstrumentLoadResult>,
    plugin_instrument_tx: std::sync::mpsc::Sender<PluginInstrumentLoadResult>,
    // Plugin GUI support
    plugin_window_manager: Option<PluginWindowManager>,
    plugin_gui_raw_ptrs: std::collections::HashMap<PluginGuiKey, PluginRawPtr>,
    /// Raw pointers for live state capture at save time, keyed like
    /// the GUI pointers. Entries live exactly as long as the device.
    plugin_state_ptrs: std::collections::HashMap<PluginGuiKey, vibez_plugin_host::PluginStatePtr>,

    // Dropbox
    dropbox_settings: DropboxSettings,
    dropbox_cache: DropboxCache,
    dropbox_client: Option<Arc<DropboxClient>>,
    remote_materialization_abort: Option<iced::task::Handle>,
    remote_import_abort: Option<iced::task::Handle>,
    remote_import_request_id: u64,
    remote_materialization_request_id: u64,
    remote_audition_cache_lease: Option<vibez_dropbox::CacheLease>,
    remote_import_in_flight: bool,
    pending_remote_audition: Option<DropboxEntry>,

    // External MIDI input (USB keyboard, Ableton Push, virtual cable...).
    // Dropping the handle closes the port.
    midi_input: Option<vibez_audio_io::midi_input::MidiInputHandle>,
    /// Cached list of port names last seen by `list_midi_input_ports`.
    /// Populated when the user opens the MIDI picker; used so the UI
    /// can show a dropdown without re-scanning on every frame.
    midi_input_ports: Vec<String>,
    // Undo / redo
}

pub fn run() -> iced::Result {
    // Raw 64x64 RGBA (assets/icon/vibez-64.rgba, generated from
    // assets/icon/vibez.svg) so the window icon needs no image
    // decoder in the dependency tree.
    let icon = iced::window::icon::from_rgba(
        include_bytes!("../../../../assets/icon/vibez-64.rgba").to_vec(),
        64,
        64,
    )
    .ok();
    iced::application("vibez", App::update, App::view)
        .theme(App::theme)
        .antialiasing(true)
        .subscription(App::subscription)
        .window({
            #[allow(unused_mut)]
            let mut settings = iced::window::Settings {
                icon,
                min_size: Some(iced::Size::new(900.0, 600.0)),
                ..Default::default()
            };
            // WM_CLASS / app_id: lets docks and taskbars match the
            // window to a vibez.desktop entry instead of guessing.
            // The field only exists in the Linux settings variant.
            #[cfg(target_os = "linux")]
            {
                settings.platform_specific.application_id = "vibez".to_string();
            }
            settings
        })
        .window_size((1400.0, 900.0))
        .font(icons::ICON_FONT_BYTES)
        .run_with(App::new)
}

mod actions;
mod audio_tasks;
mod bounce;
mod keyboard;
mod local_watcher;

pub(crate) use audio_tasks::*;
pub(crate) use keyboard::*;
mod dropbox_io;
mod media;
mod plugins;
mod project_io;
mod update;
mod views_browser;
mod views_detail;
mod views_devices;
mod views_overlays;
mod views_settings;
mod views_shell;

impl App {
    fn new() -> (Self, Task<Message>) {
        let (mut engine, cmd_tx, event_rx) = AudioEngine::new();
        let spectrum_rx = engine.take_spectrum_consumer();
        let ui_settings = UiSettings::load();

        let (stream, sample_rate) = match AudioOutputStream::open(engine, Some(512)) {
            Ok(s) => {
                let sr = s.sample_rate();
                if let Err(e) = s.play() {
                    eprintln!("vibez: failed to start audio stream: {e}");
                }
                (Some(s), sr)
            }
            Err(e) => {
                eprintln!("vibez: failed to open audio stream: {e}");
                (None, 44_100)
            }
        };

        let dropbox_settings = DropboxSettings::load();
        let dropbox_cache = DropboxCache::with_policy(vibez_dropbox::MediaCachePolicy {
            budget_bytes: ui_settings.media_cache_budget_bytes,
            automatic_eviction: ui_settings.media_cache_automatic_eviction,
        });
        let cache_usage = dropbox_cache.usage().unwrap_or_default();
        let resolved_key = load_app_key_with_env_override(&dropbox_settings);
        let dropbox_client = match (&resolved_key, &dropbox_settings.tokens) {
            (Some(key), Some(tokens)) => {
                Some(Arc::new(DropboxClient::new(key.clone(), tokens.clone())))
            }
            _ => None,
        };
        let catalog_store = crate::remote_provider::RemoteCatalogStore::for_dropbox();
        let (remote_catalog, mut remote_catalog_state) = match catalog_store.load() {
            Ok(catalog) => (catalog, crate::state::RemoteCatalogState::Ready),
            Err(error) => (
                crate::remote_provider::RemoteCatalogSnapshot::default(),
                crate::state::RemoteCatalogState::Stale { error },
            ),
        };
        if dropbox_client.is_none()
            && matches!(
                remote_catalog_state,
                crate::state::RemoteCatalogState::Ready
            )
        {
            remote_catalog_state = crate::state::RemoteCatalogState::AuthenticationRequired {
                error: "Sign in to refresh; showing the last saved Remote catalog".into(),
            };
        }
        let mut remote_ui_state = crate::state::RemoteUiState {
            connected: dropbox_client.is_some(),
            account_email: dropbox_settings.account_email.clone(),
            app_key_input: dropbox_settings.app_key.clone().unwrap_or_default(),
            has_app_key: resolved_key.is_some(),
            catalog: remote_catalog,
            catalog_state: remote_catalog_state,
            cache_usage_bytes: cache_usage.bytes,
            cache_entries: cache_usage.entries,
            cache_budget_bytes: ui_settings.media_cache_budget_bytes,
            cache_automatic_eviction: ui_settings.media_cache_automatic_eviction,
            ..Default::default()
        };
        remote_ui_state.rebuild_catalog_children();

        let mut state = AppState {
            transport: crate::state::TransportState {
                sample_rate,
                ..Default::default()
            },
            auto_warp_on_import: ui_settings.auto_warp_on_import,
            warp_confidence_threshold: ui_settings.warp_confidence_threshold,
            browser: crate::state::BrowserState {
                open: ui_settings.sample_browser_open,
                dock_width: ui_settings.sample_browser_width.clamp(
                    crate::state::BROWSER_DOCK_MIN_WIDTH,
                    crate::state::BROWSER_DOCK_MAX_WIDTH,
                ),
                audition_enabled: ui_settings.audition_enabled,
                audition_gain: ui_settings.audition_gain.clamp(0.0, 2.0),
                audition_loop: ui_settings.audition_loop,
                roots: ui_settings.sample_library_roots,
                remote: remote_ui_state,
                ..Default::default()
            },
            ..Default::default()
        };

        // Themes: scan the user's .vzt collection, then restore the
        // saved selection (built-in name or user theme name).
        let (user_themes, theme_warnings) = crate::themes::scan_user_themes();
        for warning in theme_warnings {
            eprintln!("vibez: theme scan: {warning}");
        }
        state.user_themes = user_themes;
        if let Some(name) = &ui_settings.theme {
            let palette = crate::themes::builtin_by_name(name).or_else(|| {
                state
                    .user_themes
                    .iter()
                    .find(|t| t.palette.name == *name)
                    .map(|t| t.palette.clone())
            });
            if let Some(palette) = palette {
                th::set_theme(palette);
                state.current_theme_name = name.clone();
            }
        }

        let (plugin_effect_tx, plugin_effect_rx) = std::sync::mpsc::channel();
        let (plugin_instrument_tx, plugin_instrument_rx) = std::sync::mpsc::channel();

        // Register the UI thread (process main thread) as the CLAP "main thread".
        // JUCE-based CLAP plugins require GUI calls on this thread.
        vibez_plugin_host::set_clap_main_thread();

        let plugin_window_manager = PluginWindowManager::new();

        // Auto-connect to the preferred MIDI input if the saved name
        // matches one currently visible, else to the first available
        // port. Silent on failure: the user can still work without
        // MIDI, and Settings → Audio lets them pick manually later.
        let midi_input = auto_open_midi_input(ui_settings.preferred_midi_input.as_deref());

        let mut app = Self {
            state,
            cmd_tx: Some(cmd_tx),
            event_rx: Some(event_rx),
            spectrum_rx,
            spectrum_tap: None,
            _stream: stream,
            plugin_effect_rx,
            plugin_effect_tx,
            plugin_instrument_rx,
            plugin_instrument_tx,
            plugin_window_manager,
            plugin_gui_raw_ptrs: std::collections::HashMap::new(),
            plugin_state_ptrs: std::collections::HashMap::new(),
            dropbox_settings,
            dropbox_cache,
            dropbox_client,
            remote_materialization_abort: None,
            remote_import_abort: None,
            remote_import_request_id: 0,
            remote_materialization_request_id: 0,
            remote_audition_cache_lease: None,
            remote_import_in_flight: false,
            pending_remote_audition: None,
            midi_input,
            midi_input_ports: Vec::new(),
        };

        // Inform the engine of the actual sample rate
        app.send_command(EngineCommand::SetBpm(app.state.transport.bpm));
        app.send_command(EngineCommand::SetAuditionGain(
            app.state.browser.audition_gain,
        ));
        app.send_command(EngineCommand::SetAuditionLoop(
            app.state.browser.audition_loop,
        ));

        // Console model: the master bus carries its channel EQ from
        // the first frame.
        app.ensure_master_eq();

        let roots = app.state.browser.roots.clone();
        let local_startup_task = Task::batch(roots.into_iter().map(|root| {
            let revision = app.state.browser.begin_root_scan(&root, false);
            Task::perform(scan_sample_root_async(root.clone()), move |result| {
                Message::Browser(BrowserMsg::LocalRootCatalogReconciled {
                    root: root.clone(),
                    revision,
                    result,
                })
            })
        }));
        let remote_startup_task = if app.dropbox_client.is_some() {
            Task::done(Message::RefreshRemoteConnection)
        } else {
            Task::none()
        };

        // `vibez <project.vzp>` opens a project straight from the
        // command line (also how file-manager associations launch
        // us). Legacy `.vibez` files load the same way.
        let open_task = std::env::args()
            .nth(1)
            .map(std::path::PathBuf::from)
            .filter(|p| p.is_file())
            .map(|p| Task::done(Message::ProjectOpenPathSelected(Some(p))))
            .unwrap_or_else(Task::none);

        (
            app,
            Task::batch([local_startup_task, remote_startup_task, open_task]),
        )
    }

    /// Find a theme by name: built-ins first, then the scanned user
    /// collection.
    pub(crate) fn resolve_theme(&self, name: &str) -> Option<crate::theme::ThemePalette> {
        crate::themes::builtin_by_name(name).or_else(|| {
            self.state
                .user_themes
                .iter()
                .find(|t| t.palette.name == name)
                .map(|t| t.palette.clone())
        })
    }

    fn send_command(&mut self, cmd: EngineCommand) {
        if let Some(ref mut tx) = self.cmd_tx {
            let _ = tx.push(cmd);
        }
    }

    fn mark_project_dirty(&mut self) {
        self.state.project.dirty = true;
    }

    pub(super) fn active_editor_pixels_per_beat(&self) -> f32 {
        if let Some((track_id, clip_id)) = self.state.arrangement.selected_note_clip {
            if let Some(duration) = self.state.find_track(track_id).and_then(|track| {
                track
                    .note_clips
                    .iter()
                    .find(|clip| clip.id == clip_id)
                    .map(|clip| clip.duration_beats)
            }) {
                return (self.state.view.window_width - 52.0).max(1.0) / duration.max(1.0) as f32;
            }
        }
        20.0 * self.state.view.zoom_level
    }

    /// Walk `next_track_number` forward past any names already in use so
    /// that `format!("{prefix} {n}")` is unique. Prevents e.g. two lanes
    /// both named "Track 2" when numbering gets out of sync after loads,
    /// deletes, or undo chains.
    fn next_unique_track_number(&mut self, prefix: &str) -> u32 {
        loop {
            let candidate = self.state.arrangement.next_track_number;
            let name = format!("{prefix} {candidate}");
            let clash = self.state.arrangement.tracks.iter().any(|t| t.name == name);
            if !clash {
                return candidate;
            }
            self.state.arrangement.next_track_number += 1;
        }
    }

    /// Auto-scroll the arrangement when a clip's right edge nears the visible boundary.
    /// Called from resize/move handlers so the view follows the drag.
    fn auto_scroll_to_beat(&mut self, clip_end_beat: f64) {
        let ppb = 20.0 * self.state.view.zoom_level as f64;
        // Conservative estimate of canvas width (window minus track headers)
        let canvas_width = 1400.0_f64;
        let visible_beats = canvas_width / ppb;
        let visible_end = self.state.view.scroll_offset_beats + visible_beats;
        let margin = 2.0_f64;

        if clip_end_beat > visible_end - margin {
            let delta = clip_end_beat - visible_end + margin * 2.0;
            let total = self.state.total_beats();
            self.state.view.scroll_offset_beats =
                (self.state.view.scroll_offset_beats + delta).clamp(0.0, total);
        }
        // Also scroll left when dragging toward the left edge
        if clip_end_beat < self.state.view.scroll_offset_beats + margin
            && self.state.view.scroll_offset_beats > 0.0
        {
            let delta = self.state.view.scroll_offset_beats + margin - clip_end_beat;
            self.state.view.scroll_offset_beats =
                (self.state.view.scroll_offset_beats - delta).max(0.0);
        }
    }

    fn theme(&self) -> Theme {
        th::vibez_theme()
    }

    fn subscription(&self) -> Subscription<Message> {
        Subscription::batch([
            iced::time::every(std::time::Duration::from_millis(UI_TICK_MS)).map(|_| Message::Tick),
            local_watcher::subscription(self.state.browser.roots.clone()),
            iced::keyboard::on_key_press(global_key_handler),
            iced::event::listen_with(|event, _status, _id| match event {
                iced::Event::Mouse(iced::mouse::Event::CursorMoved { position }) => {
                    Some(Message::View(ViewMsg::CursorMoved(position.x, position.y)))
                }
                iced::Event::Mouse(iced::mouse::Event::ButtonReleased(
                    iced::mouse::Button::Left,
                )) => Some(Message::View(ViewMsg::MouseReleased)),
                iced::Event::Window(iced::window::Event::Resized(size)) => Some(Message::View(
                    ViewMsg::WindowResized(size.width, size.height),
                )),
                _ => None,
            }),
        ])
    }
}

async fn decode_local_for_preview_async(
    path: PathBuf,
) -> Result<Arc<vibez_core::audio_buffer::DecodedAudio>, String> {
    let audio = decode_file_async(path).await?;
    Ok(Arc::new(audio))
}

async fn decode_file_async(
    path: PathBuf,
) -> Result<vibez_core::audio_buffer::DecodedAudio, String> {
    tokio::task::spawn_blocking(move || {
        file_io::decode_audio_file(&path).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("decode task failed: {e}"))?
}

async fn decode_and_stage_local_async(
    path: PathBuf,
) -> Result<(vibez_core::audio_buffer::DecodedAudio, MediaSourceRef), String> {
    tokio::task::spawn_blocking(move || {
        let audio = file_io::decode_audio_file(&path).map_err(|error| error.to_string())?;
        let source = vibez_project::project_format_v1::stage_local_file(&path)
            .map_err(|error| error.to_string())?;
        Ok((audio, source))
    })
    .await
    .map_err(|error| format!("decode/stage task failed: {error}"))?
}

async fn save_project_async(
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

async fn quantize_audio_clip_async(
    input: QuantizeInput,
) -> Result<crate::message::AudioQuantizeSuccess, String> {
    tokio::task::spawn_blocking(move || compute_audio_quantize(input))
        .await
        .map_err(|e| format!("quantize task failed: {e}"))?
}

async fn detect_clip_bpm_async(
    audio: Arc<vibez_core::audio_buffer::DecodedAudio>,
    sample_rate: u32,
) -> Option<vibez_core::onset::BpmEstimate> {
    tokio::task::spawn_blocking(move || vibez_core::onset::detect_bpm(&audio, sample_rate))
        .await
        .unwrap_or(None)
}

async fn warp_browser_audition_async(
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

async fn prepare_browser_import_audio_async(
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

async fn auto_warp_clip_async(input: AutoWarpInput) -> crate::message::AutoWarpOutcome {
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

async fn connect_dropbox_async(
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

async fn fetch_dropbox_sample_async(
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
            cache
                .write(&entry.path_lower, entry.rev.as_deref(), &bytes)
                .map_err(|error| format!("Media Cache write failed: {error}"))?
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

async fn materialize_remote_sample_async(
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
            cache
                .write(&entry.path_lower, entry.rev.as_deref(), &bytes)
                .map_err(|error| format!("Media Cache write failed: {error}"))?
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

async fn export_async(
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

async fn bounce_async(
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
async fn finish_loaded_clip(
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

async fn load_project_async(
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
                Err(err) => warnings.push(format!("Skipped clip '{}' ({})", clip.name, err)),
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
        sampler_samples,
        drum_rack_pad_samples,
        warnings,
    })
}

async fn hydrate_saved_source(
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

async fn decode_blocking(path: PathBuf) -> Result<vibez_core::audio_buffer::DecodedAudio, String> {
    tokio::task::spawn_blocking(move || {
        file_io::decode_audio_file(&path).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("decode task failed: {e}"))?
}

async fn hydrate_dropbox_source(
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

async fn scan_sample_root_async(root: PathBuf) -> Result<SampleLibraryScanResult, String> {
    tokio::task::spawn_blocking(move || scan_sample_root(&root))
        .await
        .map_err(|err| format!("scan task failed: {err}"))?
}

#[cfg(test)]
mod project_format_v1_tests {
    use super::*;
    use vibez_core::audio_buffer::DecodedAudio;
    use vibez_core::id::ClipId;
    use vibez_core::midi::{InstrumentKind, TrackKind};
    use vibez_core::track::{InstrumentStateInfo, TrackInfo};
    use vibez_engine::commands::{AuditionSync, EngineCommand};

    fn one_second_audio() -> Arc<DecodedAudio> {
        Arc::new(DecodedAudio {
            channels: vec![vec![0.25; 44_100]],
            sample_rate: 44_100,
        })
    }

    fn format_fixture(file_name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../vibez-audio-io/tests/fixtures")
            .join(file_name)
    }

    fn assert_audible(audio: Arc<DecodedAudio>, label: &str) {
        let (mut engine, mut commands, _events) = vibez_engine::engine::AudioEngine::new();
        commands
            .push(EngineCommand::StartAudition {
                audio,
                sync: AuditionSync::Off,
                looped: false,
            })
            .unwrap();
        let mut output = vec![0.0_f32; 8_192];
        engine.process(&mut output, 2);
        assert!(
            output.iter().any(|sample| sample.abs() > 1.0e-5),
            "{label} produced no Audition output"
        );
    }

    #[tokio::test]
    async fn supported_format_matrix_catalogs_auditions_imports_and_reopens() {
        let fixtures = [
            ("mono-44100-s16.wav", "WAV", 1, 44_100),
            ("stereo-48000-s24.aiff", "AIFF", 2, 48_000),
            ("mono-32000-s24.flac", "FLAC", 1, 32_000),
            ("stereo-44100.mp3", "MP3", 2, 44_100),
            ("mono-48000.ogg", "OGG", 1, 48_000),
            ("stereo-44100.m4a", "M4A", 2, 44_100),
        ];

        for (file_name, format, channels, sample_rate) in fixtures {
            let directory = tempfile::tempdir().unwrap();
            let source_path = directory.path().join(file_name);
            std::fs::copy(format_fixture(file_name), &source_path).unwrap();

            let catalog =
                crate::app::audio_tasks::scan_sample_root(&directory.path().to_path_buf()).unwrap();
            assert_eq!(catalog.entries.len(), 1, "{file_name}");
            assert_eq!(catalog.entries[0].format, format);
            let source = catalog.entries[0].source.clone();

            let audition = decode_local_for_preview_async(source_path.clone())
                .await
                .unwrap();
            assert_eq!(audition.num_channels(), channels, "{file_name}");
            assert_eq!(audition.sample_rate, sample_rate, "{file_name}");
            assert_audible(Arc::clone(&audition), file_name);

            let mut browser = crate::state::BrowserState {
                entries: catalog.entries,
                ..crate::state::BrowserState::default()
            };
            browser.select_source(source);
            assert!(browser.install_audition(
                browser.selected_source.clone().unwrap(),
                Arc::clone(&audition)
            ));
            let metadata = &browser.entries[0];
            assert_eq!(metadata.channels, Some(channels));
            assert_eq!(metadata.sample_rate, Some(sample_rate));
            assert!(metadata
                .duration_seconds
                .is_some_and(|duration| duration > 0.0));

            let (imported, staged) = decode_and_stage_local_async(source_path.clone())
                .await
                .unwrap();
            assert_eq!(imported.num_frames(), audition.num_frames(), "{file_name}");
            let track = TrackInfo::new("Audio");
            let project_path = directory.path().join("format-roundtrip.vzp");
            let project = Project {
                tracks: vec![track.clone()],
                clips: vec![ClipInfo {
                    id: ClipId::new(),
                    track_id: track.id,
                    name: file_name.into(),
                    position: 0,
                    source_offset: 0,
                    duration: imported.num_frames() as u64,
                    source: Some(staged),
                    file_path: None,
                    loop_enabled: false,
                    loop_start: 0,
                    loop_end: 0,
                    original_bpm: None,
                    warped: false,
                    warped_to_bpm: None,
                }],
                ..Project::default()
            };
            vibez_project::project_format_v1::save_project_v1(&project_path, None, project)
                .unwrap();
            std::fs::remove_file(source_path).unwrap();

            let reopened = load_project_async(project_path, None).await.unwrap();
            let reopened_audio = Arc::clone(&reopened.clips[0].audio);
            assert_eq!(reopened_audio.num_channels(), channels, "{file_name}");
            assert_eq!(reopened_audio.sample_rate, sample_rate, "{file_name}");
            assert_eq!(
                reopened_audio.num_frames(),
                imported.num_frames(),
                "{file_name}"
            );
            assert!(matches!(
                reopened.project.clips[0].source,
                Some(MediaSourceRef::ProjectMedia { .. })
            ));
            assert_audible(reopened_audio, file_name);
        }
    }

    #[tokio::test]
    async fn corrupt_advertised_local_source_never_reaches_staging() {
        let directory = tempfile::tempdir().unwrap();
        let source_path = directory.path().join("corrupt.wav");
        std::fs::write(&source_path, b"RIFF not decodable audio").unwrap();

        let catalog =
            crate::app::audio_tasks::scan_sample_root(&directory.path().to_path_buf()).unwrap();
        assert_eq!(catalog.entries.len(), 1, "extension is only eligibility");
        let preview_error = decode_local_for_preview_async(source_path.clone())
            .await
            .unwrap_err();
        let import_error = decode_and_stage_local_async(source_path).await.unwrap_err();
        assert!(!preview_error.is_empty());
        assert!(!import_error.is_empty());
    }

    #[tokio::test]
    async fn warp_arrangement_import_reopens_from_project_media_without_local_source() {
        let directory = tempfile::tempdir().unwrap();
        let source_path = directory.path().join("loop.wav");
        let project_path = directory.path().join("warp-import.vzp");
        let raw = one_second_audio();
        vibez_audio_io::file_io::write_wav_file(&source_path, &raw).unwrap();
        let source = vibez_project::project_format_v1::stage_local_file(&source_path).unwrap();
        let treatment = crate::state::AuditionImportInput {
            mode: crate::state::AuditionMode::Warp,
            source_bpm: Some(120.0),
        };
        let (warped, original, staged) = prepare_browser_import_audio_async(
            crate::message::BrowserImportTarget::ArrangementNewTrackAt {
                position_samples: 0,
            },
            treatment,
            Arc::clone(&raw),
            source,
            60.0,
        )
        .await
        .unwrap();
        assert_eq!(warped.num_frames(), 88_200);
        assert_eq!(original.unwrap().num_frames(), raw.num_frames());

        let track = TrackInfo::new("Audio");
        let project = Project {
            tracks: vec![track.clone()],
            clips: vec![ClipInfo {
                id: ClipId::new(),
                track_id: track.id,
                name: "loop.wav".into(),
                position: 0,
                source_offset: 0,
                duration: warped.num_frames() as u64,
                source: Some(staged),
                file_path: None,
                loop_enabled: false,
                loop_start: 0,
                loop_end: 0,
                original_bpm: Some(120.0),
                warped: true,
                warped_to_bpm: Some(60.0),
            }],
            ..Project::default()
        };
        vibez_project::project_format_v1::save_project_v1(&project_path, None, project).unwrap();
        std::fs::remove_file(source_path).unwrap();

        let loaded = load_project_async(project_path, None).await.unwrap();
        assert_eq!(loaded.clips[0].audio.num_frames(), warped.num_frames());
        assert!(matches!(
            loaded.project.clips[0].source,
            Some(MediaSourceRef::ProjectMedia { .. })
        ));
    }

    #[tokio::test]
    async fn warp_sampler_import_bakes_heard_audio_into_project_media() {
        let directory = tempfile::tempdir().unwrap();
        let source_path = directory.path().join("device-loop.wav");
        let project_path = directory.path().join("warp-device.vzp");
        let raw = one_second_audio();
        vibez_audio_io::file_io::write_wav_file(&source_path, &raw).unwrap();
        let source = vibez_project::project_format_v1::stage_local_file(&source_path).unwrap();
        let mut track = TrackInfo::new("Sampler");
        track.kind = TrackKind::Midi;
        track.instrument = Some(InstrumentKind::Sampler);
        let (warped, original, staged) = prepare_browser_import_audio_async(
            crate::message::BrowserImportTarget::Sampler(track.id),
            crate::state::AuditionImportInput {
                mode: crate::state::AuditionMode::Warp,
                source_bpm: Some(120.0),
            },
            raw,
            source,
            60.0,
        )
        .await
        .unwrap();
        assert!(original.is_none(), "device media is the baked WARP buffer");
        track.native_instrument = Some(InstrumentStateInfo::Sampler {
            params: Vec::new(),
            source: Some(staged),
        });
        let project = Project {
            tracks: vec![track],
            ..Project::default()
        };
        vibez_project::project_format_v1::save_project_v1(&project_path, None, project).unwrap();
        std::fs::remove_file(source_path).unwrap();

        let loaded = load_project_async(project_path, None).await.unwrap();
        assert_eq!(loaded.sampler_samples.len(), 1);
        assert_eq!(
            loaded.sampler_samples[0].audio.num_frames(),
            warped.num_frames()
        );
        assert!(matches!(
            loaded.project.tracks[0]
                .native_instrument
                .as_ref()
                .and_then(|state| match state {
                    InstrumentStateInfo::Sampler { source, .. } => source.as_ref(),
                    _ => None,
                }),
            Some(MediaSourceRef::ProjectMedia { .. })
        ));
    }

    #[tokio::test]
    async fn v1_reopen_decodes_embedded_audio_after_source_removal() {
        let directory = tempfile::tempdir().unwrap();
        let source_path = directory.path().join("source.wav");
        let project_path = directory.path().join("self-contained.vzp");
        let audio = DecodedAudio {
            channels: vec![vec![0.0, 0.25, -0.5, 0.75, -1.0]],
            sample_rate: 44_100,
        };
        vibez_audio_io::file_io::write_wav_file(&source_path, &audio).unwrap();
        let track = TrackInfo::new("Audio");
        let project = Project {
            tracks: vec![track.clone()],
            clips: vec![ClipInfo {
                id: ClipId::new(),
                track_id: track.id,
                name: "source.wav".into(),
                position: 0,
                source_offset: 0,
                duration: audio.num_frames() as u64,
                source: Some(MediaSourceRef::LocalFile {
                    path: source_path.clone(),
                }),
                file_path: Some(source_path.clone()),
                loop_enabled: false,
                loop_start: 0,
                loop_end: audio.num_frames() as u64,
                original_bpm: None,
                warped: false,
                warped_to_bpm: None,
            }],
            ..Project::default()
        };
        vibez_project::project_format_v1::save_project_v1(&project_path, None, project).unwrap();
        std::fs::remove_file(source_path).unwrap();

        let loaded = load_project_async(project_path, None).await.unwrap();
        assert_eq!(loaded.clips.len(), 1);
        assert_eq!(loaded.clips[0].audio.num_frames(), audio.num_frames());
        assert!(matches!(
            loaded.project.clips[0].source,
            Some(MediaSourceRef::ProjectMedia { .. })
        ));
        assert_eq!(loaded.project.tracks[0].id, track.id);
    }

    #[tokio::test]
    async fn cached_remote_media_materializes_without_a_client_and_persists_metadata() {
        let directory = tempfile::tempdir().unwrap();
        let source_path = directory.path().join("source.wav");
        let audio = DecodedAudio {
            channels: vec![vec![0.0, 0.5, -0.5, 0.25]],
            sample_rate: 44_100,
        };
        vibez_audio_io::file_io::write_wav_file(&source_path, &audio).unwrap();
        let cache = DropboxCache::with_root(directory.path().join("media-cache"));
        cache
            .write(
                "/megalodon/source.wav",
                Some("rev-1"),
                &std::fs::read(source_path).unwrap(),
            )
            .unwrap();
        let entry = DropboxEntry {
            path_lower: "/megalodon/source.wav".into(),
            path_display: "/Megalodon/Source.wav".into(),
            name: "Source.wav".into(),
            is_folder: false,
            rev: Some("rev-1".into()),
            size: None,
        };
        let lease = cache.protect(&entry.path_lower, entry.rev.as_deref());
        let materialized =
            materialize_remote_sample_async(None, cache.clone(), entry, lease, false)
                .await
                .unwrap();
        assert_eq!(materialized.audio.num_frames(), audio.num_frames());
        assert_eq!(
            materialized.metadata.provider_revision.as_deref(),
            Some("rev-1")
        );
        assert_eq!(materialized.metadata.channels, 1);
        assert_eq!(materialized.metadata.sample_rate, 44_100);
        assert!(cache
            .derived_metadata("/megalodon/source.wav", Some("rev-1"))
            .unwrap()
            .is_some());
    }

    #[tokio::test]
    async fn remote_warp_import_reopens_after_cache_clear_without_dropbox() {
        let directory = tempfile::tempdir().unwrap();
        let source_path = directory.path().join("remote-loop.wav");
        let project_path = directory.path().join("remote-owned.vzp");
        let raw = one_second_audio();
        vibez_audio_io::file_io::write_wav_file(&source_path, &raw).unwrap();
        let cache = DropboxCache::with_root(directory.path().join("media-cache"));
        cache
            .write(
                "/megalodon/remote-loop.wav",
                Some("rev-9"),
                &std::fs::read(&source_path).unwrap(),
            )
            .unwrap();
        let entry = DropboxEntry {
            path_lower: "/megalodon/remote-loop.wav".into(),
            path_display: "/Megalodon/Remote Loop.wav".into(),
            name: "Remote Loop.wav".into(),
            is_folder: false,
            rev: Some("rev-9".into()),
            size: None,
        };
        let (decoded, name, staged) = fetch_dropbox_sample_async(None, cache.clone(), entry)
            .await
            .unwrap();
        assert!(matches!(
            staged,
            MediaSourceRef::StagedRemoteProjectMedia { .. }
        ));
        let treatment = crate::state::AuditionImportInput {
            mode: crate::state::AuditionMode::Warp,
            source_bpm: Some(120.0),
        };
        let (warped, original, staged) = prepare_browser_import_audio_async(
            crate::message::BrowserImportTarget::ArrangementNewTrackAt {
                position_samples: 0,
            },
            treatment,
            decoded,
            staged,
            60.0,
        )
        .await
        .unwrap();
        assert_eq!(warped.num_frames(), 88_200);
        assert_eq!(original.unwrap().num_frames(), raw.num_frames());

        cache.clear().unwrap();
        std::fs::remove_file(source_path).unwrap();
        let track = TrackInfo::new("Audio");
        let project = Project {
            tracks: vec![track.clone()],
            clips: vec![ClipInfo {
                id: ClipId::new(),
                track_id: track.id,
                name,
                position: 0,
                source_offset: 0,
                duration: warped.num_frames() as u64,
                source: Some(staged),
                file_path: None,
                loop_enabled: false,
                loop_start: 0,
                loop_end: 0,
                original_bpm: Some(120.0),
                warped: true,
                warped_to_bpm: Some(60.0),
            }],
            ..Project::default()
        };
        vibez_project::project_format_v1::save_project_v1(&project_path, None, project).unwrap();

        let reopened = load_project_async(project_path.clone(), None)
            .await
            .unwrap();
        assert_eq!(reopened.clips[0].audio.num_frames(), warped.num_frames());
        assert_audible(Arc::clone(&reopened.clips[0].audio), "Remote WARP reopen");
        let Some(MediaSourceRef::ProjectMedia {
            provenance: Some(provenance),
            ..
        }) = reopened.project.clips[0].source.as_ref()
        else {
            panic!("reopened clip must carry Remote provenance on Project Media");
        };
        let vibez_core::track::MediaProvenance::Remote {
            provider,
            connection_id,
            connection_name,
            source_path,
            revision,
            ..
        } = provenance.as_ref()
        else {
            panic!("reopened clip provenance must remain Remote");
        };
        assert_eq!(provider, "dropbox");
        assert_eq!(connection_id, "dropbox-primary");
        assert_eq!(connection_name.as_deref(), Some("Alex's Dropbox"));
        assert_eq!(source_path, "/Megalodon/Remote Loop.wav");
        assert_eq!(revision.as_deref(), Some("rev-9"));
        let serialized = serde_json::to_string(
            &vibez_project::project_format_v1::ProjectContainer::open(project_path)
                .unwrap()
                .load_document()
                .unwrap(),
        )
        .unwrap();
        assert!(!serialized.contains("access_token"));
        assert!(!serialized.contains("refresh_token"));
    }

    #[tokio::test]
    async fn dropping_a_debounced_uncached_request_before_200ms_materializes_nothing() {
        let directory = tempfile::tempdir().unwrap();
        let cache = DropboxCache::with_root(directory.path().join("media-cache"));
        let entry = DropboxEntry {
            path_lower: "/megalodon/transient.wav".into(),
            path_display: "/Megalodon/Transient.wav".into(),
            name: "Transient.wav".into(),
            is_folder: false,
            rev: Some("rev-1".into()),
            size: None,
        };
        let lease = cache.protect(&entry.path_lower, entry.rev.as_deref());
        let future = materialize_remote_sample_async(None, cache.clone(), entry, lease, true);
        tokio::pin!(future);
        tokio::select! {
            _ = tokio::time::sleep(std::time::Duration::from_millis(20)) => {}
            result = &mut future => panic!("debounced request completed early: {result:?}"),
        }
        drop(future);
        assert!(!cache.is_cached("/megalodon/transient.wav", Some("rev-1")));
        assert_eq!(cache.usage().unwrap(), vibez_dropbox::CacheUsage::default());
    }

    #[tokio::test]
    async fn uncached_degraded_materialization_requires_explicit_reconnect() {
        let directory = tempfile::tempdir().unwrap();
        let cache = DropboxCache::with_root(directory.path().join("media-cache"));
        let entry = DropboxEntry {
            path_lower: "/megalodon/uncached.wav".into(),
            path_display: "/Megalodon/Uncached.wav".into(),
            name: "Uncached.wav".into(),
            is_folder: false,
            rev: Some("rev-1".into()),
            size: None,
        };
        let lease = cache.protect(&entry.path_lower, entry.rev.as_deref());
        let error = materialize_remote_sample_async(None, cache, entry, lease, false)
            .await
            .unwrap_err();
        assert!(error.contains("Reconnect Required"));
    }
}
