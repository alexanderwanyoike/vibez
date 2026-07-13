//! The message router: one exhaustive match, bodies delegated to
//! domain updates and topic-module handlers.

use std::sync::Arc;

use iced::Task;

use crate::domains::arrangement::ArrangementMsg;
use crate::domains::browser::BrowserMsg;
use crate::domains::piano_roll::PianoRollMsg;
use crate::domains::project::ProjectMsg;
use crate::domains::transport::TransportMsg;
use crate::domains::view::ViewMsg;
use vibez_core::track::MediaSourceRef;
use vibez_dropbox::{load_app_key_with_env_override, DropboxClient};
use vibez_engine::commands::EngineCommand;
use vibez_plugin_host::gui::PluginGuiKey;

use crate::services::plugin_loader::{load_plugin_effect_bg, load_plugin_instrument_bg};

use crate::message::{BrowserImportTarget, Message};

use super::*;

impl App {
    pub(super) fn update(&mut self, message: Message) -> Task<Message> {
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
            || matches!(&message, Message::Automation(m) if m.marks_dirty());
        if should_mark_dirty {
            self.push_undo_snapshot();
            self.mark_project_dirty();
        }

        match message {
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
                    self.state.devices.update(
                        msg,
                        &mut engine,
                        &mut self.state.arrangement.tracks,
                        &mut self.state.arrangement.master,
                        &mut self.state.arrangement.buses,
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
                let action = {
                    let mut engine = crate::domains::EngineTx(&mut self.cmd_tx);
                    self.state.arrangement.update(msg, &mut engine, ctx)
                };
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
                let action = {
                    let mut engine = crate::domains::EngineTx(&mut self.cmd_tx);
                    self.state.piano_roll.update(
                        msg,
                        &mut engine,
                        &mut self.state.arrangement.tracks,
                        ctx,
                    )
                };
                self.apply_piano_roll_action(action);
            }
            Message::Browser(msg) => {
                let action = self.state.browser.update(msg);
                return self.apply_browser_action(action);
            }
            Message::Automation(msg) => {
                let action = {
                    let mut engine = crate::domains::EngineTx(&mut self.cmd_tx);
                    self.state.automation_ui.update(
                        msg,
                        &mut engine,
                        &mut self.state.arrangement.tracks,
                        &mut self.state.arrangement.master,
                        &mut self.state.arrangement.buses,
                    )
                };
                if let Some(status) = action.status {
                    self.state.status_text = status;
                }
            }
            Message::View(msg) => {
                if matches!(&msg, ViewMsg::ToggleEditMenu) {
                    self.state.project.file_menu_open = false;
                }
                let browser_resize = match &msg {
                    ViewMsg::CursorMoved(x, _) if self.state.browser.dock_resize_active => {
                        Some(BrowserMsg::ResizeDock(*x))
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
                let action = self
                    .state
                    .view
                    .update(msg, &self.state.arrangement.tracks, ctx);
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

            // -- Engine events --
            Message::Tick => {
                return self.handle_tick();
            }
            Message::EngineMetering { peak_l, peak_r } => {
                self.state.peak_l = peak_l;
                self.state.peak_r = peak_r;
            }

            // -- Multi-track messages --
            Message::DeleteKeyPressed => {
                // Never delete anything while a text field is being
                // edited; backspace belongs to the text there.
                if self.state.view.editing_track_name.is_some()
                    || self.state.view.editing_clip_name.is_some()
                {
                    return Task::none();
                }
                // Priority 1: a selected automation point.
                if self.state.automation_ui.selected.is_some() {
                    return self.update(Message::Automation(
                        crate::domains::automation::AutomationMsg::DeleteSelectedPoint,
                    ));
                }
                // Priority 2: selected notes in the open piano roll.
                if let Some((track_id, clip_id)) = self.state.arrangement.selected_note_clip {
                    let has_selection = self
                        .state
                        .find_track(track_id)
                        .and_then(|t| t.note_clips.iter().find(|c| c.id == clip_id))
                        .is_some_and(|c| !c.selected_notes.is_empty());
                    if has_selection {
                        return self.update(Message::PianoRoll(PianoRollMsg::RemoveSelectedNotes(
                            track_id, clip_id,
                        )));
                    }
                }
                // Priority 3: selected arrangement clips.
                if !self.state.arrangement.selected_clips.is_empty() {
                    return self.update(Message::Arrangement(ArrangementMsg::DeleteSelectedClip));
                }
            }
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

            // -- Effects --

            // -- Instrument tracks --

            // -- Sampler --
            Message::LoadSamplerSample(track_id) => {
                return Task::perform(
                    async {
                        let handle = rfd::AsyncFileDialog::new()
                            .set_title("Load Sample")
                            .add_filter("Audio", &["wav", "mp3", "flac", "ogg"])
                            .pick_file()
                            .await;
                        handle.map(|h| h.path().to_path_buf())
                    },
                    move |path| Message::SamplerFileSelected(track_id, path),
                );
            }
            Message::SamplerFileSelected(track_id, path) => {
                if let Some(path) = path {
                    let file_name = path
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default();
                    self.state.status_text = format!("Loading {file_name}...");
                    return Task::perform(decode_and_stage_local_async(path), move |result| {
                        match result {
                            Ok((audio, source)) => Message::SamplerSampleDecoded(
                                track_id,
                                Arc::new(audio),
                                file_name.clone(),
                                source,
                            ),
                            Err(e) => Message::SamplerDecodeError(track_id, e),
                        }
                    });
                }
            }
            Message::SamplerSampleDecoded(track_id, audio, name, source) => {
                self.apply_sampler_sample_loaded(track_id, audio, name, source);
            }
            Message::SamplerDecodeError(track_id, err) => {
                self.state.arrangement.selected_track = Some(track_id);
                self.state.status_text = format!("Sample load error: {err}");
            }
            Message::LoadDrumRackPadSample(track_id, pad_index) => {
                return Task::perform(
                    async {
                        let handle = rfd::AsyncFileDialog::new()
                            .set_title("Load Drum Pad Sample")
                            .add_filter("Audio", &["wav", "mp3", "flac", "ogg"])
                            .pick_file()
                            .await;
                        handle.map(|h| h.path().to_path_buf())
                    },
                    move |path| Message::DrumRackPadFileSelected(track_id, pad_index, path),
                );
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

            // -- Clip looping --

            // -- Piano roll / note clips --

            // -- Clip operations --

            // -- Piano roll scroll --

            // ── Arrangement clip interaction ──

            // -- Split (Ctrl+E) --
            // If time selection is active → split all clips at region boundaries.
            // Otherwise → split selected clips at the playhead.

            // -- Join selected clips (Ctrl+J) --

            // -- Arrangement loop --
            // -- Time selection + context menu --

            // -- Clip creation from region --

            // -- Track reordering --

            // -- Renaming --

            // -- MIDI track (no auto-synth) --

            // -- Instrument attach/detach --

            // -- Pattern halve --

            // -- Edit mode --

            // -- Device context menu --

            // -- Sample browser --
            Message::ToggleAutoWarpOnImport => {
                self.state.auto_warp_on_import = !self.state.auto_warp_on_import;
                self.persist_ui_settings();
            }
            Message::SetWarpConfidenceThreshold(v) => {
                self.state.warp_confidence_threshold = v.clamp(0.0, 1.0);
                self.persist_ui_settings();
            }
            Message::RescanMidiInputs => {
                match vibez_audio_io::midi_input::list_midi_input_ports() {
                    Ok(ports) => {
                        self.midi_input_ports = ports;
                        self.state.status_text =
                            format!("Found {} MIDI input port(s)", self.midi_input_ports.len());
                    }
                    Err(err) => {
                        self.midi_input_ports.clear();
                        self.state.status_text = format!("MIDI scan error: {err}");
                    }
                }
            }
            Message::OpenMidiInput(name) => {
                match vibez_audio_io::midi_input::open_midi_input(&name) {
                    Ok(handle) => {
                        self.state.status_text = format!("MIDI input: {}", handle.port_name);
                        self.midi_input = Some(handle);
                        self.persist_ui_settings();
                    }
                    Err(err) => {
                        self.state.status_text = format!("MIDI open error: {err}");
                    }
                }
            }
            Message::CloseMidiInput => {
                self.midi_input = None;
                self.persist_ui_settings();
                self.state.status_text = "MIDI input disconnected".to_string();
            }
            Message::SelectTheme(name) => {
                if let Some(palette) = self.resolve_theme(&name) {
                    th::set_theme(palette);
                    self.state.current_theme_name = name.clone();
                    self.persist_ui_settings();
                    self.state.status_text = format!("Theme: {name}");
                } else {
                    self.state.status_text = format!("Theme {name:?} not found");
                }
            }
            Message::RescanThemes => {
                let (themes, warnings) = crate::themes::scan_user_themes();
                let count = themes.len();
                self.state.user_themes = themes;
                self.state.status_text = if warnings.is_empty() {
                    format!("{count} user theme(s) found")
                } else {
                    format!("{count} user theme(s), {} skipped", warnings.len())
                };
                for warning in warnings {
                    eprintln!("vibez: theme scan: {warning}");
                }
            }
            Message::ThemeSaveNameChanged(name) => {
                self.state.theme_save_name = name;
            }
            Message::SaveCurrentTheme => {
                let name = self.state.theme_save_name.trim().to_string();
                if name.is_empty() {
                    self.state.status_text = "Name the theme before saving".to_string();
                    return Task::none();
                }
                let mut palette = th::current();
                palette.name = name.clone();
                match crate::themes::save_user_theme(&palette) {
                    Ok(path) => {
                        let (themes, _) = crate::themes::scan_user_themes();
                        self.state.user_themes = themes;
                        self.state.current_theme_name = name;
                        self.state.theme_save_name.clear();
                        self.persist_ui_settings();
                        self.state.status_text = format!("Theme saved to {}", path.display());
                    }
                    Err(err) => {
                        self.state.status_text = format!("Theme save error: {err}");
                    }
                }
            }
            Message::RewarpAllClips => {
                return self.handle_rewarp_all_clips();
            }
            Message::AddSampleLibraryRoot => {
                return Task::perform(
                    async {
                        let handle = rfd::AsyncFileDialog::new()
                            .set_title("Add Sample Library Root")
                            .pick_folder()
                            .await;
                        handle.map(|folder| folder.path().to_path_buf())
                    },
                    Message::SampleLibraryRootSelected,
                );
            }
            Message::SampleLibraryRootSelected(path) => {
                if let Some(path) = path {
                    if !self.state.browser.roots.iter().any(|root| root == &path) {
                        self.state.browser.roots.push(path.clone());
                        self.state.browser.roots.sort();
                        self.persist_ui_settings();
                    }
                    self.state.browser.select_local_folder(Some(path.clone()));
                    let revision = self.state.browser.begin_root_scan(&path, false);
                    self.state.status_text = format!("Scanning {}...", path.display());
                    return Task::perform(scan_sample_root_async(path.clone()), move |result| {
                        Message::Browser(BrowserMsg::LocalRootCatalogReconciled {
                            root: path.clone(),
                            revision,
                            result,
                        })
                    });
                }
            }
            Message::RescanSampleLibrary => {
                self.state.status_text = "Rescanning sample library...".to_string();
                let roots = self.state.browser.roots.clone();
                return Task::batch(roots.into_iter().map(|root| {
                    let revision = self.state.browser.begin_root_scan(&root, false);
                    Task::perform(scan_sample_root_async(root.clone()), move |result| {
                        Message::Browser(BrowserMsg::LocalRootCatalogReconciled {
                            root: root.clone(),
                            revision,
                            result,
                        })
                    })
                }));
            }
            Message::ClickLocalBrowserEntry(source) => {
                if self.state.browser.drag_source.is_some() {
                    self.state.browser.cancel_media_drag();
                    self.state.status_text = "Drag cancelled".into();
                    return Task::none();
                }
                let changed = self.state.browser.select_source(source.clone());
                if changed {
                    if let MediaSourceRef::LocalFile { path } = source.clone() {
                        if self.state.browser.audition_enabled {
                            self.state.browser.begin_audition_load(&source);
                            self.state.status_text = "Preparing Audition...".to_string();
                            return Task::perform(
                                decode_local_for_preview_async(path),
                                move |result| {
                                    Message::LocalSamplePreviewReady(source.clone(), result)
                                },
                            );
                        }
                        self.state.browser.begin_waveform_load(&source);
                        return Task::perform(
                            decode_local_for_preview_async(path),
                            move |result| Message::BrowserWaveformReady(source.clone(), result),
                        );
                    }
                }
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
            Message::PreviewLocalEntry(source) => {
                self.state.browser.select_source(source.clone());
                self.state.browser.begin_audition_load(&source);
                if let MediaSourceRef::LocalFile { path } = source.clone() {
                    self.state.status_text = "Preparing Audition...".to_string();
                    return Task::perform(decode_local_for_preview_async(path), move |result| {
                        Message::LocalSamplePreviewReady(source.clone(), result)
                    });
                }
            }
            Message::StopBrowserPreview => {
                self.stop_browser_audition();
                self.state.status_text = "Audition stopped".to_string();
            }
            Message::ToggleAuditionEnabled => {
                let enabled = self.state.browser.toggle_audition_enabled();
                if !enabled {
                    self.stop_browser_audition();
                }
                self.persist_ui_settings();
                self.state.status_text = if enabled {
                    "Selection Audition enabled".into()
                } else {
                    "Selection Audition disabled; explicit Play remains available".into()
                };
            }
            Message::SetAuditionGain(gain) => {
                self.state.browser.set_audition_gain(gain);
                self.send_command(EngineCommand::SetAuditionGain(
                    self.state.browser.audition_gain,
                ));
                self.persist_ui_settings();
            }
            Message::SetAuditionMode(mode) => {
                if self.state.browser.audition_mode == mode {
                    return Task::none();
                }
                self.stop_browser_audition();
                self.state.browser.audition_mode = mode;
                let Some(source) = self.state.browser.selected_source.clone() else {
                    return Task::none();
                };
                let Some(raw) = self.state.browser.waveform_audio.clone() else {
                    self.state.status_text = "Select a source to audition".into();
                    return Task::none();
                };
                return self.play_browser_mode(source, raw);
            }
            Message::SetAuditionSync(sync) => {
                self.state.browser.audition_sync = sync;
                self.state.status_text = match sync {
                    vibez_engine::commands::AuditionSync::Off => {
                        "Audition Sync Off: starts immediately".into()
                    }
                    vibez_engine::commands::AuditionSync::Beat => {
                        "Audition Sync Beat: queues only while transport runs".into()
                    }
                    vibez_engine::commands::AuditionSync::Bar => {
                        "Audition Sync Bar: queues only while transport runs".into()
                    }
                };
            }
            Message::ToggleAuditionLoop => {
                self.state.browser.audition_loop = !self.state.browser.audition_loop;
                self.send_command(EngineCommand::SetAuditionLoop(
                    self.state.browser.audition_loop,
                ));
                self.persist_ui_settings();
                self.state.status_text = if self.state.browser.audition_loop {
                    "Audition Loop enabled".into()
                } else {
                    "Audition Loop disabled".into()
                };
            }
            Message::AuditionBpmEditChanged(value) => {
                self.state.browser.audition_bpm_edit = value;
                self.state.browser.audition_bpm_confirmed = None;
                if self.state.browser.audition_mode == crate::state::AuditionMode::Warp
                    && (self.state.browser.audition_playing || self.state.browser.audition_queued)
                {
                    self.stop_browser_audition();
                    self.state.status_text = "Confirm the edited source BPM for WARP".into();
                }
            }
            Message::ConfirmAuditionBpm => {
                let source_bpm = match self.state.browser.confirm_audition_bpm() {
                    Ok(value) => value,
                    Err(error) => {
                        self.state.status_text = error.into();
                        return Task::none();
                    }
                };
                self.state.status_text = format!("Confirmed {source_bpm:.1} source BPM");
                if self.state.browser.audition_mode == crate::state::AuditionMode::Warp {
                    let Some(source) = self.state.browser.selected_source.clone() else {
                        return Task::none();
                    };
                    let Some(raw) = self.state.browser.waveform_audio.clone() else {
                        return Task::none();
                    };
                    self.stop_browser_audition();
                    return self.prepare_browser_warp(source, raw, source_bpm);
                }
            }
            Message::EscapePressed => {
                if self.state.browser.audition_playing
                    || self.state.browser.audition_loading
                    || self.state.browser.audition_queued
                {
                    self.stop_browser_audition();
                    self.state.status_text = "Audition stopped".into();
                } else if self.state.browser.drag_source.is_some()
                    || self.state.browser.pending_drag.is_some()
                {
                    self.state.browser.cancel_media_drag();
                    self.state.status_text = "Drag cancelled".into();
                } else {
                    return self.update(Message::View(ViewMsg::CancelEditing));
                }
            }
            // -- Drag-and-drop from sample browser --
            Message::DropSampleOnArrangement {
                track_id,
                position_samples,
            } => {
                let Some(source) = self.state.browser.drag_source.take() else {
                    return Task::none();
                };
                self.state.browser.cancel_media_drag();
                return self.dispatch_drop_on_arrangement(track_id, position_samples, source);
            }
            Message::DropSampleOnEmptyArrangement => {
                let Some(source) = self.state.browser.drag_source.take() else {
                    return Task::none();
                };
                let beat = match self.state.browser.drag_target.take() {
                    Some(crate::state::BrowserDropTarget::EmptyArrangement { beat }) => beat,
                    _ => self.state.position_beats(),
                };
                self.state.browser.cancel_media_drag();
                let position_samples = self.state.beats_to_samples(beat);
                return self.dispatch_drop_for_target(
                    source,
                    BrowserImportTarget::ArrangementNewTrackAt { position_samples },
                );
            }
            Message::DropSampleOnDrumPad {
                track_id,
                pad_index,
            } => {
                return self.handle_drop_sample_on_drum_pad(track_id, pad_index);
            }
            Message::DropSampleOnSampler { track_id } => {
                let Some(source) = self.state.browser.drag_source.take() else {
                    return Task::none();
                };
                self.state.browser.cancel_media_drag();
                return self
                    .dispatch_drop_for_target(source, BrowserImportTarget::Sampler(track_id));
            }
            Message::LocalSamplePreviewReady(source, Ok(audio)) => {
                if self
                    .state
                    .browser
                    .install_audition(source, Arc::clone(&audio))
                {
                    let source = self.state.browser.selected_source.clone().unwrap();
                    return self.play_browser_mode(source, audio);
                }
            }
            Message::LocalSamplePreviewReady(source, Err(err)) => {
                let is_current = self.state.browser.selected_source.as_ref() == Some(&source);
                self.state.browser.fail_waveform_load(&source, err.clone());
                if is_current {
                    self.stop_browser_audition();
                    self.state.status_text = format!("Audition error: {err}");
                }
            }
            Message::BrowserWaveformReady(source, Ok(audio)) => {
                if self
                    .state
                    .browser
                    .install_waveform(source.clone(), Arc::clone(&audio))
                {
                    return self.schedule_browser_bpm_detection(source, audio);
                }
            }
            Message::BrowserWaveformReady(source, Err(err)) => {
                self.state.browser.fail_waveform_load(&source, err);
            }
            Message::BrowserBpmDetected(source, estimate) => {
                if self.state.browser.install_bpm_suggestion(source, estimate)
                    && self.state.browser.audition_mode == crate::state::AuditionMode::Warp
                    && self.state.browser.audition_bpm_confirmed.is_none()
                {
                    self.state.status_text = match estimate {
                        Some((bpm, confidence))
                            if confidence < self.state.warp_confidence_threshold =>
                        {
                            format!(
                                "Low-confidence suggestion {bpm:.1} BPM; confirm or edit for WARP"
                            )
                        }
                        Some((bpm, _)) => {
                            format!("Suggested {bpm:.1} BPM; confirm for WARP")
                        }
                        None => "No BPM detected; enter a positive source BPM for WARP".into(),
                    };
                }
            }
            Message::BrowserAuditionWarpReady {
                source,
                source_bpm,
                project_bpm,
                result,
            } => {
                let current = self.state.browser.selected_source.as_ref() == Some(&source)
                    && self.state.browser.audition_mode == crate::state::AuditionMode::Warp
                    && self.state.browser.audition_bpm_confirmed == Some(source_bpm)
                    && (self.state.transport.bpm - project_bpm).abs() < f64::EPSILON;
                if !current {
                    return Task::none();
                }
                self.state.browser.audition_loading = false;
                match result {
                    Ok(audio) => self.start_browser_audition(audio),
                    Err(error) => {
                        self.state.browser.stop_audition_state();
                        self.state.status_text = format!("WARP Audition error: {error}");
                    }
                }
            }
            Message::ImportSelectedBrowserSampleToArrangement => {
                return self.handle_import_selected_browser_sample_to_arrangement();
            }
            Message::LoadSelectedBrowserSampleToDevice => {
                return self.handle_load_selected_browser_sample_to_device();
            }
            Message::BrowserSampleDecoded(target, treatment, audio, name, source) => {
                return self.prepare_browser_sample_import(target, treatment, audio, name, source);
            }
            Message::BrowserImportPrepared { target, payload } => {
                return self.apply_browser_import_prepared(target, payload);
            }
            Message::ClipAutoWarpReady {
                track_id,
                clip_id,
                outcome,
            } => {
                let action = {
                    let mut engine = crate::domains::EngineTx(&mut self.cmd_tx);
                    self.state.arrangement.apply_auto_warp_outcome(
                        &mut engine,
                        track_id,
                        clip_id,
                        outcome,
                    )
                };
                return self.apply_arrangement_action(action);
            }
            Message::BrowserSampleDecodeError(err) => {
                self.state.status_text = format!("Browser import error: {err}");
            }

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
                let project = self.project_from_state();
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
                    let project = self.project_from_state();
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
                        .arrangement
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

            // -- Sample browser mode --

            // -- Dropbox --
            Message::SaveDropboxAppKey => {
                let value = self.state.browser.dropbox.app_key_input.trim().to_string();
                self.dropbox_settings.app_key = if value.is_empty() { None } else { Some(value) };
                if let Err(err) = self.dropbox_settings.save() {
                    self.state.browser.dropbox.last_error = Some(format!("save settings: {err}"));
                }
                self.state.browser.dropbox.has_app_key =
                    load_app_key_with_env_override(&self.dropbox_settings).is_some();
                self.state.status_text = "Dropbox app key saved".to_string();
            }
            Message::ConnectDropbox => {
                return self.handle_connect_dropbox();
            }
            Message::DropboxConnected(Ok(outcome)) => {
                self.state.browser.dropbox.auth_in_progress = false;
                if let Some(app_key) = load_app_key_with_env_override(&self.dropbox_settings) {
                    let client = DropboxClient::new(app_key, outcome.tokens.clone());
                    self.dropbox_client = Some(Arc::new(client));
                }
                self.dropbox_settings.tokens = Some(outcome.tokens.clone());
                self.dropbox_settings.account_email = Some(outcome.info.email.clone());
                if let Err(err) = self.dropbox_settings.save() {
                    self.state.browser.dropbox.last_error = Some(format!("save settings: {err}"));
                }
                self.state.browser.dropbox.connected = true;
                self.state.browser.dropbox.account_email = Some(outcome.info.email.clone());
                self.state.status_text = format!("Dropbox connected: {}", outcome.info.email);
            }
            Message::DropboxConnected(Err(err)) => {
                self.state.browser.dropbox.auth_in_progress = false;
                self.state.browser.dropbox.last_error = Some(err.clone());
                self.state.status_text = format!("Dropbox connect failed: {err}");
            }
            Message::DisconnectDropbox => {
                self.dropbox_client = None;
                self.dropbox_settings.clear_tokens();
                let _ = self.dropbox_settings.save();
                self.state.browser.dropbox = crate::state::DropboxUiState {
                    app_key_input: self.state.browser.dropbox.app_key_input.clone(),
                    has_app_key: self.state.browser.dropbox.has_app_key,
                    ..Default::default()
                };
                self.state.status_text = "Dropbox disconnected".to_string();
            }
            Message::DropboxExpandFolder(path) => {
                return self.handle_dropbox_expand_folder(path);
            }
            Message::DropboxFolderListed { path, result } => {
                self.state.browser.dropbox.listing_in_progress.remove(&path);
                match result {
                    Ok(entries) => {
                        self.state.browser.dropbox.folders.insert(path, entries);
                    }
                    Err(err) => {
                        self.state.browser.dropbox.last_error = Some(err.clone());
                        self.state.status_text = format!("Dropbox error: {err}");
                    }
                }
            }
            Message::DropboxPreview(entry) => {
                let Some(client) = self.dropbox_client.clone() else {
                    self.state.browser.dropbox.last_error = Some("Not connected to Dropbox".into());
                    return Task::none();
                };
                let source = MediaSourceRef::DropboxFile {
                    path_lower: entry.path_lower.clone(),
                    display_path: entry.path_display.clone(),
                    rev: entry.rev.clone(),
                };
                self.state.browser.select_source(source.clone());
                self.state.browser.begin_audition_load(&source);
                let cache = self.dropbox_cache.clone();
                self.state.browser.dropbox.preview_in_progress = true;
                self.state.status_text = format!("Fetching preview: {}", entry.name);
                return Task::perform(
                    fetch_dropbox_sample_async(client, cache, entry),
                    move |result| {
                        Message::DropboxPreviewReady(
                            source.clone(),
                            result.map(|(audio, _, _)| audio),
                        )
                    },
                );
            }
            Message::DropboxPreviewReady(source, Ok(audio)) => {
                self.state.browser.dropbox.preview_in_progress = false;
                if self
                    .state
                    .browser
                    .install_audition(source.clone(), Arc::clone(&audio))
                {
                    return self.play_browser_mode(source, audio);
                }
            }
            Message::DropboxPreviewReady(source, Err(err)) => {
                self.state.browser.dropbox.preview_in_progress = false;
                let is_current = self.state.browser.selected_source.as_ref() == Some(&source);
                self.state.browser.fail_waveform_load(&source, err.clone());
                self.state.browser.dropbox.last_error = Some(err.clone());
                if is_current {
                    self.stop_browser_audition();
                    self.state.status_text = format!("Preview error: {err}");
                }
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
