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
    edge_shortcuts: EdgeShortcutState,
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
    remote_materialization_request: tracked_request::TrackedRequest,
    remote_import_request: tracked_request::TrackedRequest,
    remote_audition_cache_lease: Option<vibez_dropbox::CacheLease>,
    pending_remote_audition: Option<DropboxEntry>,
    /// Generation for the whole Browser import pipeline (local and
    /// remote). Bumped on cancellation and project reset so an
    /// in-flight WARP preparation cannot land a clip afterwards.
    browser_import_request: tracked_request::TrackedRequest,
    /// Bumped on every refresh start and on disconnect, so catalog pages
    /// fetched for a previous connection are dropped instead of reconciled.
    remote_catalog_request: tracked_request::TrackedRequest,
    /// Catalog changes accumulated since the last reconcile flush; applied in
    /// batches so each page does not re-sort the whole catalog in update().
    remote_catalog_pending: Vec<crate::remote_provider::RemoteChange>,

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
        .font(crate::typography::PLEX_SANS_CONDENSED_MEDIUM_BYTES)
        .font(crate::typography::PLEX_SANS_CONDENSED_SEMIBOLD_BYTES)
        .font(crate::typography::PLEX_MONO_MEDIUM_BYTES)
        .font(crate::typography::PLEX_MONO_SEMIBOLD_BYTES)
        .run_with(App::new)
}

mod actions;
mod async_helpers;
mod audio_tasks;
mod bounce;
mod keyboard;
mod local_watcher;
mod tracked_request;

use async_helpers::*;
pub(crate) use audio_tasks::*;
pub(crate) use keyboard::*;
mod dropbox_io;
mod media;
mod media_import;
mod plugins;
mod project_io;
mod project_replay;
mod project_sections;
mod update;
mod update_media;
mod update_remote;
mod update_timeline;
mod views_arrangement;
mod views_automation;
mod views_browser;
mod views_browser_audition;
mod views_browser_places;
mod views_browser_remote;
mod views_browser_style;
mod views_detail;
mod views_devices;
mod views_mixer;
mod views_overlays;
mod views_perform;
mod views_perform_playhead;
mod views_perform_sections;
mod views_settings;
mod views_settings_perform;
mod views_shell;
mod views_transport;

#[cfg(test)]
mod project_format_v1_tests;

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
        dropbox_io::seed_remote_availability(&dropbox_cache, &mut remote_ui_state);

        let mut state = AppState {
            transport: crate::state::TransportState {
                sample_rate,
                ..Default::default()
            },
            auto_warp_on_import: ui_settings.auto_warp_on_import,
            warp_confidence_threshold: ui_settings.warp_confidence_threshold,
            confirm_project_track_deletion: ui_settings.confirm_project_track_deletion,
            view: crate::state::ViewState {
                perform_surface_width: ui_settings.perform_surface_width,
                detail_panel_height: ui_settings.detail_panel_height,
                ..Default::default()
            },
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
        state.perform.input_mapping = ui_settings.perform_input_mapping.clone();

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
            edge_shortcuts: EdgeShortcutState::default(),
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
            remote_materialization_request: Default::default(),
            remote_import_request: Default::default(),
            remote_audition_cache_lease: None,
            pending_remote_audition: None,
            browser_import_request: Default::default(),
            remote_catalog_request: Default::default(),
            remote_catalog_pending: Vec::new(),
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

        // Staged Project Media copies are content-addressed and shared, so
        // saves and aborted imports never delete them eagerly; this sweep
        // is the cache policy that reclaims entries old enough that no
        // live session can still reference them.
        std::thread::spawn(|| {
            vibez_project::project_format_v1::sweep_stale_staging(std::time::Duration::from_secs(
                7 * 24 * 60 * 60,
            ));
        });

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
        let editor = self.state.active_timeline_editor();
        if let Some((track_id, clip_id)) = editor.selected_note_clip {
            if let Some(duration) =
                self.state
                    .active_timeline_content(track_id)
                    .and_then(|content| {
                        content
                            .note_clips
                            .iter()
                            .find(|clip| clip.id == clip_id)
                            .map(|clip| clip.duration_beats)
                    })
            {
                return crate::timeline_geometry::TimelineGeometry::fitted(
                    duration,
                    self.state.view.window_width,
                    52.0,
                )
                .pixels_per_beat();
            }
        }
        crate::timeline_geometry::TimelineGeometry::from_zoom(
            self.state.view.zoom_level,
            self.state.view.scroll_offset_beats,
        )
        .pixels_per_beat()
    }

    /// Walk `next_track_number` forward past any names already in use so
    /// that `format!("{prefix} {n}")` is unique. Prevents e.g. two lanes
    /// both named "Track 2" when numbering gets out of sync after loads,
    /// deletes, or undo chains.
    fn next_unique_track_number(&mut self, prefix: &str) -> u32 {
        loop {
            let candidate = self.state.project_tracks.next_track_number;
            let name = format!("{prefix} {candidate}");
            let clash = self
                .state
                .project_tracks
                .tracks
                .iter()
                .any(|t| t.name == name);
            if !clash {
                return candidate;
            }
            Arc::make_mut(&mut self.state.project_tracks).next_track_number += 1;
        }
    }

    /// Auto-scroll the arrangement when a clip's right edge nears the visible boundary.
    /// Called from resize/move handlers so the view follows the drag.
    fn auto_scroll_to_beat(&mut self, clip_end_beat: f64) {
        // Conservative estimate of canvas width (window minus track headers)
        let canvas_width = 1400.0_f32;
        let geometry = crate::timeline_geometry::TimelineGeometry::from_zoom(
            self.state.view.zoom_level,
            self.state.view.scroll_offset_beats,
        );
        let visible_beats = geometry.visible_beats(canvas_width);
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
            iced::event::listen_with(|event, _status, _id| match event {
                iced::Event::Keyboard(event) => keyboard_input_message(event, _status),
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
