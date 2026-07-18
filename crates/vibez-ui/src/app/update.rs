//! The message router: one exhaustive match, bodies delegated to
//! domain updates and topic-module handlers.

use iced::Task;

use crate::domains::arrangement::ArrangementMsg;
use crate::domains::browser::BrowserMsg;
use crate::domains::project::ProjectMsg;
use crate::domains::timeline_editor::TimelineEditorAdapter;
use crate::domains::transport::TransportMsg;
use crate::domains::view::ViewMsg;
use vibez_engine::commands::EngineCommand;
use vibez_plugin_host::gui::PluginGuiKey;

use crate::services::plugin_loader::{load_plugin_effect_bg, load_plugin_instrument_bg};

use crate::message::Message;

use super::*;

fn apply_project_track_deletion_policy(message: Message, confirm: bool) -> Message {
    match message {
        Message::Arrangement(ArrangementMsg::RequestRemoveTrack(track_id)) if !confirm => {
            Message::Arrangement(ArrangementMsg::RemoveTrack(track_id))
        }
        message => message,
    }
}

impl App {
    pub(super) fn update(&mut self, message: Message) -> Task<Message> {
        let (message, undo_gesture) = match message {
            Message::UndoGesture { id, edit } => (*edit, Some(id)),
            message => (message, None),
        };
        let message =
            apply_project_track_deletion_policy(message, self.state.confirm_project_track_deletion);
        if self.state.view.edit_menu_open {
            let keep_menu = matches!(
                &message,
                Message::Tick
                    | Message::Transport(TransportMsg::EnginePosition(_))
                    | Message::EngineMetering { .. }
                    | Message::Transport(TransportMsg::EngineStopped)
                    | Message::Arrangement(ArrangementMsg::EngineTrackMeter { .. })
                    | Message::View(ViewMsg::ToggleEditMenu)
                    | Message::View(ViewMsg::CursorMoved(_, _))
                    | Message::View(ViewMsg::WindowResized(_, _))
                    | Message::View(ViewMsg::MouseReleased)
            );
            if !keep_menu {
                self.state.view.edit_menu_open = false;
            }
        }
        // Auto-dismiss context menu on any action except tick/engine/menu events
        if self.state.view.context_menu.is_some() {
            let keep_menu = matches!(
                message,
                Message::Tick
                    | Message::Transport(TransportMsg::EnginePosition(_))
                    | Message::EngineMetering { .. }
                    | Message::Transport(TransportMsg::EngineStopped)
                    | Message::Arrangement(ArrangementMsg::EngineTrackMeter { .. })
                    | Message::View(ViewMsg::ShowContextMenu { .. })
                    | Message::View(ViewMsg::DismissContextMenu)
                    | Message::Arrangement(ArrangementMsg::DeleteClipsInRegion { .. })
                    | Message::Arrangement(ArrangementMsg::SetSelectionAsLoop)
                    | Message::Arrangement(ArrangementMsg::DeleteSelectedClip)
                    | Message::Arrangement(ArrangementMsg::DuplicateSelectedClip)
                    | Message::Arrangement(ArrangementMsg::SplitSelectedAtPlayhead)
                    | Message::Arrangement(ArrangementMsg::JoinSelectedClips)
                    | Message::Arrangement(ArrangementMsg::SplitAudioClip { .. })
                    | Message::Arrangement(ArrangementMsg::SplitNoteClip { .. })
                    | Message::Arrangement(ArrangementMsg::SplitClipsAtRegion { .. })
                    | Message::Arrangement(ArrangementMsg::CreateNoteClipFromSelection(_))
                    | Message::View(ViewMsg::EditNameText(_))
                    | Message::View(ViewMsg::CursorMoved(_, _))
                    | Message::View(ViewMsg::WindowResized(_, _))
                    | Message::View(ViewMsg::MouseReleased)
                    | Message::NewProject
                    | Message::OpenProject
                    | Message::SaveProject
                    | Message::SaveProjectAs
                    | Message::Project(ProjectMsg::ToggleFileMenu)
                    | Message::Project(ProjectMsg::DismissFileMenu)
                    | Message::ProjectOpenPathSelected(_)
                    | Message::ProjectSavePathSelected(_)
                    | Message::ProjectLoaded(_)
                    | Message::ProjectSaved(_)
                    | Message::OpenSettings
                    | Message::CloseSettings
                    | Message::SelectSettingsTab(_)
                    | Message::SetBufferSize(_)
                    | Message::ScanPlugins
                    | Message::ScanPluginsComplete(_)
                    | Message::PluginLoadError(_)
            );
            if !keep_menu {
                self.state.view.context_menu = None;
            }
        }

        let should_mark_dirty = matches!(
            &message,
            Message::Transport(TransportMsg::BpmSubmit)
                | Message::Arrangement(ArrangementMsg::AddTrack)
                | Message::ClipAudioDecoded(..)
                | Message::Arrangement(ArrangementMsg::AddInstrumentTrack)
                | Message::SamplerSampleDecoded(..)
                | Message::DrumRackPadSampleDecoded(..)
                | Message::BrowserImportPrepared { .. }
                | Message::Arrangement(ArrangementMsg::SetClipLoopRegion { .. })
                | Message::Arrangement(ArrangementMsg::MoveAudioClip { .. })
                | Message::Arrangement(ArrangementMsg::MoveNoteClipPosition { .. })
                | Message::Arrangement(ArrangementMsg::ResizeAudioClip { .. })
                | Message::Arrangement(ArrangementMsg::MoveClipToTrack { .. })
                | Message::Arrangement(ArrangementMsg::DeleteSelectedClip)
                | Message::Arrangement(ArrangementMsg::DuplicateSelectedClip)
                | Message::Transport(TransportMsg::ToggleArrangementLoop)
                | Message::Transport(TransportMsg::SetArrangementLoopRegion { .. })
                | Message::Arrangement(ArrangementMsg::MoveSelectedTrackUp)
                | Message::Arrangement(ArrangementMsg::MoveSelectedTrackDown)
                | Message::Arrangement(ArrangementMsg::AddMidiTrack)
                | Message::AudioQuantizeReady { .. }
                | Message::ClipWarpReady { .. }
                | Message::ClipAutoWarpReady { .. }
        ) || matches!(&message, Message::Devices(m) if m.marks_dirty())
            || matches!(&message, Message::Arrangement(m) if m.marks_dirty())
            || matches!(&message, Message::PianoRoll(m) if m.marks_dirty())
            || matches!(&message, Message::Automation(m) if m.marks_dirty())
            || matches!(&message, Message::Perform(m) if m.marks_dirty());
        if should_mark_dirty {
            self.push_undo_snapshot(undo_gesture);
            self.mark_project_dirty();
        }

        match message {
            Message::UndoGesture { .. } => {
                unreachable!("undo gesture wrappers are removed before routing")
            }
            Message::KeyboardInput { event, occurred_at } => {
                return self.handle_keyboard_input(event, occurred_at);
            }
            // The transport domain owns its logic entirely; app.rs
            // only computes the cross-domain context, routes the
            // message, and applies the returned action.
            Message::Transport(msg) => {
                let ctx = crate::domains::transport::TransportCtx {
                    total_duration_samples: self.state.total_duration_samples(),
                    time_selection: if self.state.arrangement.time_selection_active {
                        Some((
                            self.state.arrangement.selection_start_beats,
                            self.state.arrangement.selection_end_beats,
                        ))
                    } else {
                        None
                    },
                };
                let action = {
                    let mut engine = crate::domains::EngineTx(&mut self.cmd_tx);
                    self.state.transport.update(msg, &mut engine, ctx)
                };
                return self.apply_transport_action(action);
            }
            // Devices domain: same routing pattern. Tracks are the
            // shared model handed in explicitly; the returned action
            // carries GUI teardown / selection / status effects.
            Message::Devices(msg) => {
                let sample_rate = self.state.transport.sample_rate;
                let action = {
                    let mut engine = crate::domains::EngineTx(&mut self.cmd_tx);
                    let project_tracks = Arc::make_mut(&mut self.state.project_tracks);
                    self.state.devices.update(
                        msg,
                        &mut engine,
                        &mut project_tracks.tracks,
                        &mut project_tracks.master,
                        &mut project_tracks.buses,
                        sample_rate,
                    )
                };
                self.apply_devices_action(action);
            }
            Message::Arrangement(msg) => {
                let ctx = crate::domains::arrangement::ArrangementCtx {
                    samples_per_beat: if self.state.transport.bpm > 0.0 {
                        60.0 * self.state.transport.sample_rate as f64 / self.state.transport.bpm
                    } else {
                        0.0
                    },
                    playhead_samples: self.state.transport.position_samples,
                    playhead_beats: self.state.position_beats(),
                };
                let action = self.route_arrangement_editor_message(msg, ctx);
                self.state
                    .perform
                    .sync_project_tracks(&self.state.project_tracks.tracks);
                return self.apply_arrangement_action(action);
            }
            Message::PianoRoll(msg) => {
                let ctx = crate::domains::piano_roll::PianoRollCtx {
                    snap_grid: self
                        .state
                        .view
                        .grid_config()
                        .effective_grid(self.active_editor_pixels_per_beat()),
                };
                let action = self.route_piano_roll_editor_message(msg, ctx);
                self.apply_piano_roll_action(action);
            }
            Message::Browser(msg) => {
                let action = self.state.browser.update(msg);
                return self.apply_browser_action(action);
            }
            Message::Automation(msg) => {
                let action = self.route_automation_editor_message(msg);
                if let Some(status) = action.status {
                    self.state.status_text = status;
                }
            }
            Message::Perform(msg) => {
                let ctx = crate::domains::perform::PerformCtx {
                    workspace_visible: self.state.view.workspace
                        == crate::state::Workspace::Perform,
                    project_tracks: &self.state.project_tracks.tracks,
                    selected_project_track: self.state.arrangement.selected_track,
                };
                let action = {
                    let mut engine = crate::domains::EngineTx(&mut self.cmd_tx);
                    self.state.perform.update(msg, &mut engine, ctx)
                };
                self.apply_perform_action(action);
            }
            Message::View(msg) => {
                if matches!(&msg, ViewMsg::ToggleEditMenu) {
                    self.state.project.file_menu_open = false;
                }
                let browser_resize = match &msg {
                    ViewMsg::CursorMoved(x, _) if self.state.browser.dock_resize_active => {
                        Some(BrowserMsg::ResizeDock(
                            self.state
                                .browser
                                .dock_drag_width(*x, self.state.view.window_width),
                        ))
                    }
                    ViewMsg::MouseReleased if self.state.browser.dock_resize_active => {
                        Some(BrowserMsg::EndDockResize)
                    }
                    _ => None,
                };
                if let Some(browser_msg) = browser_resize {
                    let action = self.state.browser.update(browser_msg);
                    if action.persist_settings {
                        self.persist_ui_settings();
                    }
                }
                let pending_drag_msg = match &msg {
                    ViewMsg::CursorMoved(x, y) if self.state.browser.pending_drag.is_some() => {
                        Some(BrowserMsg::PendingDragMoved { x: *x, y: *y })
                    }
                    ViewMsg::MouseReleased if self.state.browser.pending_drag.is_some() => {
                        Some(BrowserMsg::EndDragSample)
                    }
                    ViewMsg::MouseReleased if self.state.browser.drag_source.is_some() => {
                        Some(BrowserMsg::EndDragSample)
                    }
                    _ => None,
                };
                if let Some(browser_msg) = pending_drag_msg {
                    let action = self.state.browser.update(browser_msg);
                    if let Some(status) = action.status {
                        self.state.status_text = status;
                    }
                }
                let ctx = crate::domains::view::ViewCtx {
                    total_beats: self.state.total_beats(),
                };
                let action = self.state.view.update(
                    msg,
                    self.state.arrangement.resolve_timeline().editor,
                    ctx,
                );
                return self.apply_view_action(action);
            }
            Message::Project(msg) => {
                if matches!(&msg, ProjectMsg::ToggleFileMenu) {
                    self.state.view.edit_menu_open = false;
                }
                let ctx = crate::domains::project::ProjectCtx {
                    snapshot_now: self.take_snapshot(),
                };
                let action = self.state.project.update(msg, ctx);
                if let Some(status) = action.status {
                    self.state.status_text = status;
                }
                if let Some(snapshot) = action.apply_snapshot {
                    self.apply_snapshot(snapshot);
                }
            }

            // -- Workspace --

            // -- Zoom / scroll --

            // -- Snap grid --

            // -- File menu --
            Message::NewProject => {
                self.state.project.file_menu_open = false;
                self.reset_to_new_project();
            }
            Message::OpenProject => {
                self.state.project.file_menu_open = false;
                return Task::perform(
                    async {
                        let handle = rfd::AsyncFileDialog::new()
                            .set_title("Open Vibez Project")
                            .add_filter("Vibez Project", &["vzp", "vibez", "json"])
                            .pick_file()
                            .await;
                        handle.map(|file| file.path().to_path_buf())
                    },
                    Message::ProjectOpenPathSelected,
                );
            }
            Message::SaveProject => {
                self.state.project.file_menu_open = false;
                let project = self.project_for_save();
                if let Some(path) = self.state.project.current_path.clone() {
                    return Task::perform(
                        save_project_async(path.clone(), Some(path), project),
                        |result| Message::ProjectSaved(Box::new(result)),
                    );
                }
                return self.update(Message::SaveProjectAs);
            }
            Message::SaveProjectAs => {
                self.state.project.file_menu_open = false;
                return Task::perform(
                    async {
                        let handle = rfd::AsyncFileDialog::new()
                            .set_title("Save Vibez Project")
                            .set_file_name("Untitled.vzp")
                            .add_filter("Vibez Project Format V1", &["vzp"])
                            .save_file()
                            .await;
                        handle.map(|file| file.path().to_path_buf())
                    },
                    Message::ProjectSavePathSelected,
                );
            }
            Message::ProjectOpenPathSelected(path) => {
                if let Some(path) = path {
                    self.state.status_text = format!("Opening {}...", path.display());
                    let dropbox = self
                        .dropbox_client
                        .clone()
                        .map(|client| (client, self.dropbox_cache.clone()));
                    return Task::perform(load_project_async(path, dropbox), |result| {
                        Message::ProjectLoaded(Box::new(result))
                    });
                }
            }
            Message::ProjectSavePathSelected(path) => {
                if let Some(mut path) = path {
                    if !path
                        .extension()
                        .is_some_and(|extension| extension.eq_ignore_ascii_case("vzp"))
                    {
                        path.set_extension("vzp");
                    }
                    let project = self.project_for_save();
                    return Task::perform(
                        save_project_async(path, self.state.project.current_path.clone(), project),
                        |result| Message::ProjectSaved(Box::new(result)),
                    );
                }
            }
            Message::ProjectLoaded(result) => match *result {
                Ok(loaded) => {
                    self.rebuild_from_loaded_project(loaded);
                }
                Err(err) => {
                    self.state.status_text = format!("Project load error: {err}");
                }
            },
            Message::ProjectSaved(result) => match *result {
                Ok(saved) => {
                    self.apply_saved_project_sources(&saved.project);
                    self.state.project.current_path = Some(saved.path.clone());
                    self.state.project.dirty = false;
                    self.state.status_text = format!("Saved {}", saved.path.display());
                }
                Err(err) => {
                    self.state.status_text = format!("Project save error: {err}");
                }
            },

            // -- Settings --
            Message::OpenSettings => {
                self.state.settings_open = true;
                self.state.project.file_menu_open = false;
            }
            Message::CloseSettings => {
                self.state.settings_open = false;
                self.state.perform.key_rebind_target = None;
                let _ = self.state.plugin_settings.save();
            }
            Message::SelectSettingsTab(tab) => {
                self.state.settings_tab = tab;
            }
            Message::SetBufferSize(size) => {
                return self.handle_set_buffer_size(size);
            }

            // -- Plugin scanning --
            Message::ScanPlugins => {
                return self.handle_scan_plugins();
            }
            Message::ScanPluginsComplete(report) => {
                return self.handle_scan_plugins_complete(report);
            }
            Message::AddPluginScanPath => {
                return Task::perform(
                    async {
                        let result = rfd::AsyncFileDialog::new()
                            .set_title("Select Plugin Scan Directory")
                            .pick_folder()
                            .await;
                        result.map(|h| h.path().to_path_buf())
                    },
                    Message::PluginScanPathSelected,
                );
            }
            Message::PluginScanPathSelected(path) => {
                return self.handle_plugin_scan_path_selected(path);
            }
            Message::RemovePluginScanPath(index) => {
                if index < self.state.plugin_settings.extra_scan_paths.len() {
                    self.state.plugin_settings.extra_scan_paths.remove(index);
                    let _ = self.state.plugin_settings.save();
                }
            }
            Message::ToggleScanDefaultPaths => {
                self.state.plugin_settings.scan_default_paths =
                    !self.state.plugin_settings.scan_default_paths;
                let _ = self.state.plugin_settings.save();
            }

            // -- Plugin loading --
            Message::AddPluginToTrack(track_id, plugin_id) => {
                self.state.devices.context_menu = None;
                if let Some(info) = self
                    .state
                    .plugin_settings
                    .cache
                    .iter()
                    .find(|p| p.id == plugin_id)
                    .cloned()
                {
                    let sample_rate = self.state.transport.sample_rate as f64;
                    let is_instrument = info.category.is_instrument();
                    let loading_name = info.name.clone();

                    let is_bus = self
                        .state
                        .project_tracks
                        .buses
                        .iter()
                        .any(|b| b.id == track_id);
                    if is_instrument && (track_id.is_master() || is_bus) {
                        // Master and buses host effects only.
                        self.state.status_text = format!(
                            "{loading_name} is an instrument; this channel takes effects only"
                        );
                        return Task::none();
                    }
                    if is_instrument {
                        let tx = self.plugin_instrument_tx.clone();
                        std::thread::spawn(move || {
                            match load_plugin_instrument_bg(&info, sample_rate, None) {
                                Ok(mut result) => {
                                    result.track_id = track_id;
                                    let _ = tx.send(result);
                                }
                                Err(e) => {
                                    eprintln!("Plugin load error: {e}");
                                }
                            }
                        });
                    } else {
                        let tx = self.plugin_effect_tx.clone();
                        std::thread::spawn(move || {
                            match load_plugin_effect_bg(&info, sample_rate, None) {
                                Ok(mut result) => {
                                    result.track_id = track_id;
                                    let _ = tx.send(result);
                                }
                                Err(e) => {
                                    eprintln!("Plugin load error: {e}");
                                }
                            }
                        });
                    }
                    self.state.status_text = format!("Loading {loading_name}...");
                }
            }
            Message::PluginLoadError(err) => {
                self.state.status_text = format!("Plugin error: {err}");
            }

            // -- Plugin GUI windows --
            Message::OpenPluginGui(key) => {
                // If the window is already open, raise it
                if let Some(ref mgr) = self.plugin_window_manager {
                    if mgr.is_open(key) {
                        mgr.raise(key);
                        return Task::none();
                    }
                }
                if let Some(&raw_ptr) = self.plugin_gui_raw_ptrs.get(&key) {
                    let title = match key {
                        PluginGuiKey::Effect {
                            track_id,
                            effect_id,
                        } => self
                            .state
                            .find_track(track_id)
                            .and_then(|t| {
                                t.effects
                                    .iter()
                                    .find(|e| e.id == effect_id)
                                    .and_then(|e| e.plugin_name.clone())
                            })
                            .unwrap_or_else(|| "Plugin".to_string()),
                        PluginGuiKey::Instrument { track_id } => self
                            .state
                            .find_track(track_id)
                            .and_then(|t| t.plugin_instrument_name.clone())
                            .unwrap_or_else(|| "Plugin".to_string()),
                    };
                    if let Some(ref mut mgr) = self.plugin_window_manager {
                        if mgr.open(key, raw_ptr, title) {
                            self.state.status_text = "Plugin GUI opened".to_string();
                        } else {
                            self.state.status_text = "Failed to open plugin GUI".to_string();
                        }
                    } else {
                        self.state.status_text =
                            "No X11 display — plugin GUI unavailable".to_string();
                    }
                } else {
                    self.state.status_text = "Plugin GUI handle not available".to_string();
                }
            }
            Message::ClosePluginGui(key) => {
                return self.handle_close_plugin_gui(key);
            }

            // -- Bounce / resample --
            Message::BounceSelectionToAudio => {
                self.state.view.context_menu = None;
                if !self.state.arrangement.time_selection_active
                    || self.state.arrangement.selection_end_beats
                        <= self.state.arrangement.selection_start_beats
                {
                    self.state.status_text = "No time selection active".to_string();
                    return Task::none();
                }
                let start = self
                    .state
                    .beats_to_samples(self.state.arrangement.selection_start_beats);
                let end = self
                    .state
                    .beats_to_samples(self.state.arrangement.selection_end_beats);
                return self.dispatch_bounce(
                    vibez_engine::render::BounceMode::Master,
                    (start, end),
                    start,
                    format!(
                        "Selection {:.2}–{:.2}",
                        self.state.arrangement.selection_start_beats,
                        self.state.arrangement.selection_end_beats
                    ),
                );
            }
            Message::BounceClipToAudio {
                track_id,
                clip_id,
                is_note_clip,
            } => {
                return self.handle_bounce_clip_to_audio(track_id, clip_id, is_note_clip);
            }
            Message::BounceComplete(Ok(outcome)) => {
                self.finalize_bounce(outcome);
            }
            Message::BounceComplete(Err(err)) => {
                self.state.status_text = format!("Bounce error: {err}");
            }

            // -- Quantize --
            Message::QuantizeAudioClip { track_id, clip_id } => {
                self.state.view.context_menu = None;
                let grid = self
                    .state
                    .view
                    .grid_config()
                    .effective_grid(self.active_editor_pixels_per_beat());
                return self.dispatch_audio_quantize(track_id, clip_id, grid);
            }
            Message::QuantizeAudioClipAt {
                track_id,
                clip_id,
                grid,
            } => {
                return self.dispatch_audio_quantize(track_id, clip_id, grid);
            }
            Message::AudioQuantizeReady {
                track_id,
                old_clip_id,
                result,
            } => match result {
                Ok(success) => {
                    let sample_rate = self.state.transport.sample_rate;
                    let action = {
                        let mut engine = crate::domains::EngineTx(&mut self.cmd_tx);
                        self.state.arrangement.apply_audio_quantize_success(
                            &mut engine,
                            track_id,
                            old_clip_id,
                            success,
                            sample_rate,
                        )
                    };
                    return self.apply_arrangement_action(action);
                }
                Err(err) => {
                    self.state.status_text = format!("Quantize failed: {err}");
                }
            },

            // -- Warping --
            Message::DetectClipBpm { track_id, clip_id } => {
                return self.dispatch_detect_clip_bpm(track_id, clip_id);
            }
            Message::ClipBpmDetected {
                track_id,
                clip_id,
                bpm,
                confidence,
            } => {
                let action = self
                    .state
                    .arrangement
                    .apply_clip_bpm_detected(track_id, clip_id, bpm, confidence);
                return self.apply_arrangement_action(action);
            }
            Message::WarpClipToProject { track_id, clip_id } => {
                return self.dispatch_warp_clip_to_project(track_id, clip_id);
            }
            Message::ClipWarpReady {
                track_id,
                clip_id,
                result,
            } => match result {
                Ok(success) => {
                    let action = {
                        let mut engine = crate::domains::EngineTx(&mut self.cmd_tx);
                        self.state.arrangement.apply_clip_warp_success(
                            &mut engine,
                            track_id,
                            clip_id,
                            success,
                        )
                    };
                    return self.apply_arrangement_action(action);
                }
                Err(err) => {
                    self.state.status_text = format!("Warp failed: {err}");
                }
            },

            // -- Undo / redo --

            // -- Export --
            Message::ExportProject => {
                self.state.project.file_menu_open = false;
                let default_name = self
                    .state
                    .project
                    .current_path
                    .as_ref()
                    .and_then(|p| p.file_stem())
                    .map(|n| format!("{}.wav", n.to_string_lossy()))
                    .unwrap_or_else(|| "vibez-export.wav".to_string());
                return Task::perform(
                    async move {
                        let handle = rfd::AsyncFileDialog::new()
                            .set_title("Export to WAV")
                            .set_file_name(&default_name)
                            .add_filter("WAV", &["wav"])
                            .save_file()
                            .await;
                        handle.map(|file| file.path().to_path_buf())
                    },
                    Message::ExportPathSelected,
                );
            }
            Message::ExportPathSelected(path) => {
                return self.handle_export_path_selected(path);
            }
            Message::ExportComplete(Ok(path)) => {
                self.state.status_text = format!("Exported: {}", path.display());
            }
            Message::ExportComplete(Err(err)) => {
                self.state.status_text = format!("Export error: {err}");
            }

            // -- Engine events --
            Message::Tick => {
                return self.handle_tick();
            }
            Message::EngineMetering { peak_l, peak_r } => {
                self.state.peak_l = peak_l;
                self.state.peak_r = peak_r;
            }

            // -- Multi-track messages --
            Message::DeleteKeyPressed => return self.on_delete_key_pressed(),
            Message::AddClipToTrack(track_id) => {
                return self.handle_add_clip_to_track(track_id);
            }
            Message::ClipFileSelected(track_id, path) => {
                return self.handle_clip_file_selected(track_id, path);
            }
            Message::ClipAudioDecoded(track_id, clip_id, audio, name, source) => {
                return self.handle_clip_audio_decoded(track_id, clip_id, audio, name, source);
            }
            Message::ClipDecodeError(_, err) => {
                self.state.status_text = format!("Error: {err}");
            }

            // -- Sampler / drum rack --
            Message::LoadSamplerSample(track_id) => return self.on_load_sampler_sample(track_id),
            Message::SamplerFileSelected(track_id, path) => {
                return self.on_sampler_file_selected(track_id, path)
            }
            Message::SamplerSampleDecoded(track_id, audio, name, source) => {
                self.apply_sampler_sample_loaded(track_id, audio, name, source);
            }
            Message::SamplerDecodeError(track_id, err) => {
                self.state.arrangement.selected_track = Some(track_id);
                self.state.status_text = format!("Sample load error: {err}");
            }
            Message::LoadDrumRackPadSample(track_id, pad_index) => {
                return self.on_load_drum_rack_pad_sample(track_id, pad_index)
            }
            Message::DrumRackPadFileSelected(track_id, pad_index, path) => {
                return self.handle_drum_rack_pad_file_selected(track_id, pad_index, path);
            }
            Message::DrumRackPadSampleDecoded(track_id, pad_index, audio, name, source) => {
                self.apply_drum_rack_pad_loaded(track_id, pad_index, audio, name, source);
            }
            Message::DrumRackPadDecodeError(track_id, _pad_index, err) => {
                return self.handle_drum_rack_pad_decode_error(track_id, _pad_index, err);
            }

            // -- Sample browser --
            Message::ToggleAutoWarpOnImport => {
                self.state.auto_warp_on_import = !self.state.auto_warp_on_import;
                self.persist_ui_settings();
            }
            Message::SetWarpConfidenceThreshold(v) => {
                self.state.warp_confidence_threshold = v.clamp(0.0, 1.0);
                self.persist_ui_settings();
            }
            Message::ToggleProjectTrackDeleteConfirmation => {
                self.state.confirm_project_track_deletion =
                    !self.state.confirm_project_track_deletion;
                self.persist_ui_settings();
            }
            Message::RescanMidiInputs => return self.on_rescan_midi_inputs(),
            Message::OpenMidiInput(name) => return self.on_open_midi_input(name),
            Message::CloseMidiInput => {
                self.midi_input = None;
                self.persist_ui_settings();
                self.state.status_text = "MIDI input disconnected".to_string();
            }
            Message::SelectTheme(name) => return self.on_select_theme(name),
            Message::RescanThemes => return self.on_rescan_themes(),
            Message::ThemeSaveNameChanged(name) => {
                self.state.theme_save_name = name;
            }
            Message::SaveCurrentTheme => return self.on_save_current_theme(),
            Message::RewarpAllClips => {
                return self.handle_rewarp_all_clips();
            }
            Message::AddSampleLibraryRoot => return self.on_add_sample_library_root(),
            Message::SampleLibraryRootSelected(path) => {
                return self.on_sample_library_root_selected(path)
            }
            Message::RescanSampleLibrary => return self.on_rescan_sample_library(),
            Message::ClickLocalBrowserEntry(source) => {
                return self.on_click_local_browser_entry(source)
            }
            Message::BeginPendingBrowserDrag(source, label) => {
                let action = self.state.browser.update(BrowserMsg::BeginPendingDrag {
                    source,
                    label,
                    origin_x: self.state.view.cursor_x,
                    origin_y: self.state.view.cursor_y,
                });
                return self.apply_browser_action(action);
            }
            Message::PreviewLocalEntry(source) => return self.on_preview_local_entry(source),
            Message::StopBrowserPreview => return self.on_stop_browser_preview(),
            Message::ToggleAuditionEnabled => return self.on_toggle_audition_enabled(),
            Message::SetAuditionGain(gain) => {
                self.state.browser.set_audition_gain(gain);
                self.send_command(EngineCommand::SetAuditionGain(
                    self.state.browser.audition_gain,
                ));
                self.persist_ui_settings();
            }
            Message::SetAuditionMode(mode) => return self.on_set_audition_mode(mode),
            Message::SetAuditionSync(sync) => return self.on_set_audition_sync(sync),
            Message::ToggleAuditionLoop => return self.on_toggle_audition_loop(),
            Message::AuditionBpmEditChanged(value) => {
                return self.on_audition_bpm_edit_changed(value)
            }
            Message::ConfirmAuditionBpm => return self.on_confirm_audition_bpm(),
            Message::EscapePressed => return self.on_escape_pressed(),

            // -- Drag-and-drop from sample browser --
            Message::DropSampleOnArrangement {
                track_id,
                position_samples,
            } => return self.on_drop_sample_on_arrangement(track_id, position_samples),
            Message::DropSampleOnEmptyArrangement => {
                return self.on_drop_sample_on_empty_arrangement()
            }
            Message::DropSampleOnDrumPad {
                track_id,
                pad_index,
            } => {
                return self.handle_drop_sample_on_drum_pad(track_id, pad_index);
            }
            Message::DropSampleOnSampler { track_id } => {
                return self.on_drop_sample_on_sampler(track_id)
            }
            Message::LocalSamplePreviewReady(source, generation, Ok(audio)) => {
                return self.on_local_sample_preview_ready(source, generation, audio)
            }
            Message::LocalSamplePreviewReady(source, generation, Err(err)) => {
                return self.on_local_sample_preview_failed(source, generation, err)
            }
            Message::BrowserWaveformReady(source, Ok(audio)) => {
                return self.on_browser_waveform_ready(source, audio)
            }
            Message::BrowserWaveformReady(source, Err(err)) => {
                self.state.browser.fail_waveform_load(&source, err);
            }
            Message::BrowserBpmDetected(source, estimate) => {
                return self.on_browser_bpm_detected(source, estimate)
            }
            Message::BrowserAuditionWarpReady {
                source,
                generation,
                source_bpm,
                project_bpm,
                result,
            } => {
                return self.on_browser_audition_warp_ready(
                    source,
                    generation,
                    source_bpm,
                    project_bpm,
                    result,
                )
            }
            Message::ImportSelectedBrowserSampleToArrangement => {
                return self.handle_import_selected_browser_sample_to_arrangement();
            }
            Message::SelectAdjacentBrowserResult(direction) => {
                return self.select_adjacent_browser_result(direction);
            }
            Message::LoadSelectedBrowserSampleToDevice => {
                return self.handle_load_selected_browser_sample_to_device();
            }
            Message::BrowserSampleDecoded(target, treatment, audio, name, source) => {
                return self.on_browser_sample_decoded(target, treatment, audio, name, source)
            }
            Message::RemoteImportReady {
                request_id,
                target,
                treatment,
                result,
            } => return self.on_remote_import_ready(request_id, target, treatment, result),
            Message::BrowserImportPrepared {
                target,
                generation,
                payload,
            } => return self.on_browser_import_prepared(target, generation, payload),
            Message::ClipAutoWarpReady {
                track_id,
                clip_id,
                outcome,
            } => return self.on_clip_auto_warp_ready(track_id, clip_id, outcome),
            Message::BrowserSampleDecodeError(err) => {
                return self.on_browser_sample_decode_error(err)
            }

            // -- Dropbox / remote catalog --
            Message::SaveDropboxAppKey => return self.on_save_dropbox_app_key(),
            Message::ConnectDropbox => {
                return self.handle_connect_dropbox();
            }
            Message::DropboxConnected(Ok(outcome)) => return self.on_dropbox_connected(outcome),
            Message::DropboxConnected(Err(err)) => {
                self.state.browser.remote.auth_in_progress = false;
                self.state.browser.remote.last_error = Some(err.clone());
                self.state.status_text = format!("Dropbox connect failed: {err}");
            }
            Message::DisconnectDropbox => return self.on_disconnect_dropbox(),
            Message::RefreshRemoteConnection => {
                return self.handle_remote_catalog_refresh();
            }
            Message::RemoteCatalogPageFetched {
                generation,
                completed_pages,
                result,
            } => return self.on_remote_catalog_page_fetched(generation, completed_pages, result),
            Message::RemoteCatalogSaved {
                generation,
                next_checkpoint,
                result,
            } => return self.on_remote_catalog_saved(generation, next_checkpoint, result),
            Message::SetMediaCacheBudgetGiB(gib) => return self.on_set_media_cache_budget(gib),
            Message::ToggleMediaCacheAutomaticEviction => {
                return self.on_toggle_media_cache_automatic_eviction()
            }
            Message::MediaCacheMaintenanceComplete(result) => {
                return self.on_media_cache_maintenance_complete(result)
            }
            Message::ClearMediaCache => return self.on_clear_media_cache(),
            Message::MediaCacheCleared(result) => return self.on_media_cache_cleared(result),
            Message::ClickRemoteBrowserEntry(entry) => {
                return self.on_click_remote_browser_entry(entry)
            }
            Message::RemoteAuditionReady {
                request_id,
                generation,
                source,
                result,
            } => return self.on_remote_audition_ready(request_id, generation, source, result),
            Message::DropboxPreview(entry) => {
                return self.start_remote_audition(entry, false);
            }
            Message::DropboxImportToArrangement(entry) => {
                return self.handle_dropbox_import_to_arrangement(entry);
            }
            Message::DropboxImportToDevice(entry) => {
                return self.handle_dropbox_import_to_device(entry);
            }
        }
        Task::none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vibez_core::id::TrackId;

    #[test]
    fn project_track_deletion_policy_defaults_to_the_direct_undoable_command() {
        let track_id = TrackId::new();
        let direct = apply_project_track_deletion_policy(
            Message::Arrangement(ArrangementMsg::RequestRemoveTrack(track_id)),
            false,
        );
        assert!(matches!(
            direct,
            Message::Arrangement(ArrangementMsg::RemoveTrack(id)) if id == track_id
        ));

        let confirmed = apply_project_track_deletion_policy(
            Message::Arrangement(ArrangementMsg::RequestRemoveTrack(track_id)),
            true,
        );
        assert!(matches!(
            confirmed,
            Message::Arrangement(ArrangementMsg::RequestRemoveTrack(id)) if id == track_id
        ));
    }
}
