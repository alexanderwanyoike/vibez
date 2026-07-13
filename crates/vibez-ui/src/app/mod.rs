use std::path::PathBuf;
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
    SampleLibraryScanResult,
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
        let dropbox_cache = DropboxCache::new();
        let resolved_key = load_app_key_with_env_override(&dropbox_settings);
        let dropbox_client = match (&resolved_key, &dropbox_settings.tokens) {
            (Some(key), Some(tokens)) => {
                Some(Arc::new(DropboxClient::new(key.clone(), tokens.clone())))
            }
            _ => None,
        };
        let dropbox_ui_state = crate::state::DropboxUiState {
            connected: dropbox_client.is_some(),
            account_email: dropbox_settings.account_email.clone(),
            app_key_input: dropbox_settings.app_key.clone().unwrap_or_default(),
            has_app_key: resolved_key.is_some(),
            ..Default::default()
        };

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
                roots: ui_settings.sample_library_roots,
                dropbox: dropbox_ui_state,
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
            midi_input,
            midi_input_ports: Vec::new(),
        };

        // Inform the engine of the actual sample rate
        app.send_command(EngineCommand::SetBpm(app.state.transport.bpm));

        // Console model: the master bus carries its channel EQ from
        // the first frame.
        app.ensure_master_eq();

        let startup_task = if app.state.browser.roots.is_empty() {
            Task::none()
        } else {
            app.state.browser.scan_in_progress = true;
            Task::perform(
                scan_sample_library_async(app.state.browser.roots.clone()),
                |r| Message::Browser(BrowserMsg::SampleLibraryScanned(r)),
            )
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

        (app, Task::batch([startup_task, open_task]))
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

async fn save_project_async(path: PathBuf, project: Project) -> Result<PathBuf, String> {
    tokio::task::spawn_blocking(move || {
        let save_path = path;
        project
            .save_to_file(&save_path)
            .map(|_| save_path)
            .map_err(|err| err.to_string())
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

async fn list_dropbox_folder_async(
    client: Arc<DropboxClient>,
    path: String,
) -> Result<(String, Vec<DropboxEntry>), String> {
    let entries = client.list_folder(&path).await.map_err(|e| e.to_string())?;
    Ok((path, entries))
}

async fn fetch_dropbox_sample_async(
    client: Arc<DropboxClient>,
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
    let local = client
        .download_to_cache(&entry, &cache)
        .await
        .map_err(|e| e.to_string())?;
    let decoded = tokio::task::spawn_blocking(move || {
        vibez_audio_io::file_io::decode_audio_file(&local).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("decode task failed: {e}"))??;
    let source = MediaSourceRef::DropboxFile {
        path_lower: entry.path_lower.clone(),
        display_path: entry.path_display.clone(),
        rev: entry.rev.clone(),
    };
    Ok((Arc::new(decoded), entry.name, source))
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
    let project = tokio::task::spawn_blocking(move || {
        Project::load_from_file(&load_path).map_err(|err| err.to_string())
    })
    .await
    .map_err(|err| format!("load task failed: {err}"))??;

    let mut clips = Vec::new();
    let mut sampler_samples = Vec::new();
    let mut drum_rack_pad_samples = Vec::new();
    let mut warnings = Vec::new();

    for clip in &project.clips {
        match clip.resolved_source().cloned() {
            Some(MediaSourceRef::LocalFile { path: clip_path }) => {
                match decode_blocking(clip_path).await {
                    Ok(audio) => {
                        clips.push(finish_loaded_clip(clip.clone(), Arc::new(audio)).await)
                    }
                    Err(err) => warnings.push(format!("Skipped clip '{}' ({})", clip.name, err)),
                }
            }
            Some(source @ MediaSourceRef::DropboxFile { .. }) => {
                match hydrate_dropbox_source(dropbox.as_ref(), &source, &clip.name).await {
                    Ok(audio) => {
                        clips.push(finish_loaded_clip(clip.clone(), Arc::new(audio)).await)
                    }
                    Err(err) => warnings.push(err),
                }
            }
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
                } => match source {
                    MediaSourceRef::LocalFile { path: sample_path } => {
                        match decode_blocking(sample_path.clone()).await {
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
                        }
                    }
                    MediaSourceRef::DropboxFile { .. } => {
                        match hydrate_dropbox_source(dropbox.as_ref(), source, &track.name).await {
                            Ok(audio) => sampler_samples.push(LoadedSamplerData {
                                track_id: track.id,
                                source: source.clone(),
                                audio: Arc::new(audio),
                                name: source.display_name(),
                            }),
                            Err(err) => warnings.push(err),
                        }
                    }
                },
                InstrumentStateInfo::DrumRack { pads } => {
                    for (pad_index, pad) in pads.iter().enumerate() {
                        let Some(source) = &pad.source else {
                            continue;
                        };
                        let label = format!("drum pad {} on '{}'", pad_index + 1, track.name);
                        match source {
                            MediaSourceRef::LocalFile { path: sample_path } => {
                                match decode_blocking(sample_path.clone()).await {
                                    Ok(audio) => {
                                        drum_rack_pad_samples.push(LoadedDrumRackPadData {
                                            track_id: track.id,
                                            pad_index,
                                            source: source.clone(),
                                            audio: Arc::new(audio),
                                            name: source.display_name(),
                                            state: pad.clone(),
                                        })
                                    }
                                    Err(err) => warnings.push(format!("Skipped {label} ({err})")),
                                }
                            }
                            MediaSourceRef::DropboxFile { .. } => {
                                match hydrate_dropbox_source(dropbox.as_ref(), source, &label).await
                                {
                                    Ok(audio) => {
                                        drum_rack_pad_samples.push(LoadedDrumRackPadData {
                                            track_id: track.id,
                                            pad_index,
                                            source: source.clone(),
                                            audio: Arc::new(audio),
                                            name: source.display_name(),
                                            state: pad.clone(),
                                        })
                                    }
                                    Err(err) => warnings.push(err),
                                }
                            }
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

async fn scan_sample_library_async(roots: Vec<PathBuf>) -> Result<SampleLibraryScanResult, String> {
    tokio::task::spawn_blocking(move || {
        let mut entries = Vec::new();
        let mut folders = Vec::new();
        let mut warnings = Vec::new();

        for root in roots {
            if !root.is_dir() {
                warnings.push(format!("Missing root: {}", root.display()));
                continue;
            }
            scan_root_into(&root, &root, &mut entries, &mut folders, &mut warnings);
        }

        entries.sort_by(|a, b| {
            a.relative_path
                .cmp(&b.relative_path)
                .then_with(|| a.name.cmp(&b.name))
        });
        folders.sort_by(|a, b| {
            a.root_path
                .cmp(&b.root_path)
                .then_with(|| a.relative_path.cmp(&b.relative_path))
        });

        Ok(SampleLibraryScanResult {
            entries,
            folders,
            warnings,
        })
    })
    .await
    .map_err(|err| format!("scan task failed: {err}"))?
}
