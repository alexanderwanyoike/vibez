//! The message router: one exhaustive match, bodies delegated to
//! domain updates and topic-module handlers.

use iced::Task;

use crate::domains::arrangement::ArrangementMsg;
use crate::domains::browser::BrowserMsg;
use crate::domains::transport::TransportMsg;
use vibez_engine::commands::EngineCommand;
use vibez_plugin_host::gui::PluginGuiKey;

use crate::services::plugin_loader::{load_plugin_effect_bg, load_plugin_instrument_bg};

use crate::message::Message;

use super::update_policy::apply_project_track_deletion_policy;
use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AudioRecordingTransportGuard {
    Pass,
    StopRecording,
    BlockTimelineChange,
}

fn audio_recording_transport_guard(
    phase: crate::domains::audio_recording::AudioRecordingPhase,
    message: &TransportMsg,
) -> AudioRecordingTransportGuard {
    match message {
        TransportMsg::Stop | TransportMsg::TogglePlayback
            if phase == crate::domains::audio_recording::AudioRecordingPhase::Recording =>
        {
            AudioRecordingTransportGuard::StopRecording
        }
        TransportMsg::Seek(_)
        | TransportMsg::SeekToBeat(_)
        | TransportMsg::ToggleArrangementLoop
        | TransportMsg::SetArrangementLoopRegion { .. }
        | TransportMsg::BpmSubmit
        | TransportMsg::NudgeBpm(_)
            if matches!(
                phase,
                crate::domains::audio_recording::AudioRecordingPhase::Recording
                    | crate::domains::audio_recording::AudioRecordingPhase::Stopping
            ) =>
        {
            AudioRecordingTransportGuard::BlockTimelineChange
        }
        _ => AudioRecordingTransportGuard::Pass,
    }
}

impl App {
    pub(super) fn update(&mut self, message: Message) -> Task<Message> {
        let message = match message {
            Message::SectionTimeline(edit) => {
                if super::update_timeline::section_timeline_claims_focus(
                    self.state.view.workspace,
                    self.state.perform.selected_section.is_some(),
                ) {
                    self.state.perform.editor_focus =
                        crate::domains::perform::PerformEditorFocus::SectionConstruction;
                }
                *edit
            }
            message => message,
        };
        let (message, undo_gesture) = match message {
            Message::UndoGesture { id, edit } => (*edit, Some(id)),
            message => (message, None),
        };
        let message =
            apply_project_track_deletion_policy(message, self.state.confirm_project_track_deletion);
        if let Message::Transport(transport) = &message {
            match audio_recording_transport_guard(self.state.audio_recording.phase, transport) {
                AudioRecordingTransportGuard::StopRecording => {
                    return self.stop_audio_recording();
                }
                AudioRecordingTransportGuard::BlockTimelineChange => {
                    self.state.status_text =
                        "Stop Audio recording before changing position, loop, or tempo".into();
                    return Task::none();
                }
                AudioRecordingTransportGuard::Pass => {}
            }
        }
        if self.prepare_capture_message(undo_gesture, &message) {
            return Task::none();
        }
        let owns_project_transaction = self.begin_project_track_deletion_transaction(&message);
        let deferred_arrangement_project_edit = matches!(
            &message,
            Message::Arrangement(msg) if msg.defers_project_edit()
        );
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
                | Message::AudioQuantizeReady { result: Ok(_), .. }
                | Message::ClipBpmDetected { bpm: Some(_), .. }
                | Message::ClipWarpReady {
                    record_undo: true,
                    ..
                }
                | Message::ClipAutoWarpReady { .. }
        ) || matches!(&message, Message::Devices(m) if m.marks_dirty())
            || matches!(&message, Message::Arrangement(m) if m.marks_dirty())
            || matches!(&message, Message::PianoRoll(m) if m.marks_dirty())
            || matches!(&message, Message::Automation(m) if m.marks_dirty())
            || matches!(&message, Message::Perform(m) if m.marks_dirty());
        if should_mark_dirty && !deferred_arrangement_project_edit {
            self.push_undo_snapshot(undo_gesture);
            self.mark_project_dirty();
        }

        match message {
            Message::SectionTimeline(_) => {
                unreachable!("section timeline wrappers are removed before routing")
            }
            Message::MenuItemSelected(overlay, action) => {
                let task = self.update(*action);
                menu_lifecycle::dismiss(&mut self.state, overlay);
                return task;
            }
            Message::DismissMenu(overlay) => {
                menu_lifecycle::dismiss(&mut self.state, overlay);
            }
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
                if self.place_focused_section_playhead(&msg) {
                    return Task::none();
                }
                let stops_perform = matches!(&msg, crate::domains::transport::TransportMsg::Stop)
                    || matches!(
                        &msg,
                        crate::domains::transport::TransportMsg::TogglePlayback
                            if self.state.transport.playing
                    );
                if stops_perform {
                    self.end_capture_automation_gesture();
                    self.section_residency_request.cancel();
                }
                let perform_playback_active = self.state.perform.playing_section.is_some()
                    || self.state.perform.queued_section.is_some()
                    || self.state.perform.section_record.is_active();
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
                    perform_tempo_locked: perform_playback_active,
                    perform_playback_active,
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
                let deferred_snapshot = msg.defers_project_edit().then(|| self.take_snapshot());
                let playhead_beats = self.focused_editor_playhead_beats();
                let samples_per_beat = if self.state.transport.bpm > 0.0 {
                    60.0 * self.state.transport.sample_rate as f64 / self.state.transport.bpm
                } else {
                    0.0
                };
                let playhead_samples = if self.focused_editor_is_section() {
                    (playhead_beats * samples_per_beat).round().max(0.0) as u64
                } else {
                    self.state.transport.position_samples
                };
                let ctx = crate::domains::arrangement::ArrangementCtx {
                    samples_per_beat,
                    playhead_samples,
                    playhead_beats,
                };
                let action = self.route_arrangement_editor_message(msg, ctx);
                self.state
                    .active_timeline_editor_mut()
                    .discard_orphaned_audio_clip_inspector_edits();
                if let (true, Some(snapshot)) = (action.mark_dirty, deferred_snapshot) {
                    self.state.project.history.push_edit(snapshot, undo_gesture);
                    self.mark_project_dirty();
                }
                self.state
                    .perform
                    .sync_project_tracks(&self.state.project_tracks.tracks);
                return self
                    .apply_arrangement_action_in_transaction(action, owns_project_transaction);
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
                return self.route_perform_message(msg);
            }
            Message::SectionResidencyReady {
                request_id,
                section_id,
                quantization,
                resident,
            } => {
                if self.section_residency_request.finish(request_id) {
                    if let Some(prepared) = resident.take() {
                        debug_assert_eq!(prepared.section_id, section_id);
                        self.send_command(EngineCommand::QueueSection {
                            prepared,
                            quantization,
                        });
                        self.state.status_text =
                            format!("Section ready · {}", quantization.label());
                    }
                }
            }
            Message::SectionRecordResidencyReady {
                request_id,
                request,
                resident,
            } => {
                self.finish_section_record_residency(request_id, request, resident);
            }
            Message::View(msg) => {
                return self.route_view_message(msg);
            }
            Message::Project(msg) => {
                return self.route_project_message(msg);
            }

            // -- Workspace --

            // -- Zoom / scroll --

            // -- Snap grid --

            // -- File menu --
            Message::NewProject => {
                return self.route_new_project();
            }
            Message::OpenProject => {
                return self.route_open_project();
            }
            Message::SaveProject => {
                return self.route_save_project();
            }
            Message::SaveProjectAs => {
                return self.route_save_project_as();
            }
            Message::ProjectOpenPathSelected(path) => {
                return self.route_project_open_path_selected(path);
            }
            Message::ProjectSavePathSelected(path) => {
                return self.route_project_save_path_selected(path);
            }
            Message::ProjectLoaded { path, result } => {
                return self.route_project_loaded(path, *result);
            }
            Message::ProjectSaved(result) => {
                return self.route_project_saved(*result);
            }

            // -- About --
            Message::OpenAbout => {
                self.state.about_open = true;
            }
            Message::OpenUrl(url) => {
                return Task::perform(open_url_async(url), Message::UrlOpened);
            }
            Message::UrlOpened(result) => {
                if let Err(error) = result {
                    self.state.status_text = format!("Could not open link: {error}");
                }
            }

            // -- Window close protection --
            Message::WindowCloseRequested => {
                return self.route_window_close_requested();
            }
            Message::CloseConfirmSave => {
                return self.route_close_confirm_save();
            }
            Message::CloseConfirmDiscard => {
                return self.route_close_confirm_discard();
            }
            Message::CloseConfirmCancel => {
                return self.route_close_confirm_cancel();
            }

            // -- Settings --
            Message::OpenSettings => {
                self.state.settings_open = true;
            }
            Message::CloseSettings => {
                self.state.settings_open = false;
                self.state.perform.key_rebind_target = None;
                let _ = self.state.plugin_settings.save();
            }
            Message::SelectSettingsTab(tab) => {
                self.state.settings_tab = tab;
            }
            Message::SelectAudioBackend(backend) => {
                return self.handle_select_audio_backend(backend);
            }
            Message::SetBufferSize(size) => {
                return self.handle_set_buffer_size(size);
            }
            Message::SetAudioSampleRate(sample_rate) => {
                return self.handle_set_audio_sample_rate(sample_rate);
            }
            Message::SelectAudioInput(choice) => {
                return self.handle_select_audio_input(choice);
            }
            Message::SelectAudioOutput(choice) => {
                return self.handle_select_audio_output(choice);
            }
            Message::RescanAudioDevices => {
                return self.handle_rescan_audio_devices();
            }
            Message::ReconnectAudioOutput => {
                return self.handle_reconnect_audio_output();
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
                let grid = self
                    .state
                    .view
                    .grid_config()
                    .effective_grid(self.active_editor_pixels_per_beat());
                return self.dispatch_audio_quantize(
                    self.active_timeline_location(),
                    track_id,
                    clip_id,
                    grid,
                );
            }
            Message::QuantizeAudioClipAt {
                track_id,
                clip_id,
                grid,
            } => {
                return self.dispatch_audio_quantize(
                    self.active_timeline_location(),
                    track_id,
                    clip_id,
                    grid,
                );
            }
            Message::AudioQuantizeReady {
                location,
                track_id,
                old_clip_id,
                result,
            } => match result {
                Ok(success) => {
                    let sample_rate = self.state.transport.sample_rate;
                    let action = self.apply_audio_quantize_success_at(
                        location,
                        track_id,
                        old_clip_id,
                        success,
                        sample_rate,
                    );
                    return self.apply_arrangement_action_at(action, location);
                }
                Err(err) => {
                    self.state.status_text = format!("Quantize failed: {err}");
                }
            },

            // -- Warping --
            Message::DetectClipBpm {
                location,
                track_id,
                clip_id,
            } => {
                return self.dispatch_detect_clip_bpm(location, track_id, clip_id);
            }
            Message::ClipBpmDetected {
                location,
                track_id,
                clip_id,
                bpm,
                confidence,
            } => {
                let action =
                    self.apply_clip_bpm_detected_at(location, track_id, clip_id, bpm, confidence);
                return self.apply_arrangement_action_at(action, location);
            }
            Message::WarpClipToProject {
                location,
                track_id,
                clip_id,
            } => {
                return self.dispatch_warp_clip_to_project(location, track_id, clip_id, true);
            }
            Message::ClipWarpReady {
                location,
                track_id,
                clip_id,
                record_undo: _,
                result,
            } => match result {
                Ok(success) => {
                    let action =
                        self.apply_clip_warp_success_at(location, track_id, clip_id, success);
                    return self.apply_arrangement_action_at(action, location);
                }
                Err(err) => {
                    self.state.status_text = format!("Warp failed: {err}");
                }
            },
            Message::ClipTransposeReady {
                location,
                track_id,
                clip_id,
                result,
            } => match result {
                Ok(success) => {
                    let action = match location {
                        vibez_project::TimelineLocation::Arrange => {
                            let mut engine = crate::domains::EngineTx(&mut self.cmd_tx);
                            self.state.arrangement.apply_clip_transpose_success(
                                &mut engine,
                                track_id,
                                clip_id,
                                success,
                            )
                        }
                        vibez_project::TimelineLocation::Section(section_id) => {
                            if self.state.perform.selected_section == Some(section_id) {
                                let action = {
                                    let mut engine = crate::domains::DiscardingEngine;
                                    self.state
                                        .perform
                                        .section_editor
                                        .editor_mut()
                                        .apply_clip_transpose_success(
                                            &mut engine,
                                            track_id,
                                            clip_id,
                                            success,
                                        )
                                };
                                self.state.perform.commit_selected_section_timeline();
                                self.refresh_playing_section_after_edit(section_id);
                                action
                            } else {
                                let Some(section) = Arc::make_mut(&mut self.state.perform.sections)
                                    .by_id_mut(section_id)
                                else {
                                    return Task::none();
                                };
                                let mut editor = crate::state::TimelineEditorState {
                                    timeline: Arc::clone(&section.timeline),
                                    ..crate::state::TimelineEditorState::default()
                                };
                                let mut engine = crate::domains::DiscardingEngine;
                                let action = editor.apply_clip_transpose_success(
                                    &mut engine,
                                    track_id,
                                    clip_id,
                                    success,
                                );
                                section.timeline = editor.timeline;
                                self.refresh_playing_section_after_edit(section_id);
                                action
                            }
                        }
                    };
                    return self.apply_arrangement_action_at(action, location);
                }
                Err(error) => {
                    self.state.status_text = format!("Transpose failed: {error}");
                }
            },
            Message::CommitAudioClipTransposeAfterDelay {
                location,
                track_id,
                clip_id,
                expected_semitones,
                expected_revision,
            } => {
                if self.active_timeline_location() != location {
                    return Task::none();
                }
                let expected = expected_semitones.to_string();
                let still_current = self
                    .state
                    .active_timeline_editor()
                    .audio_clip_inspector_edits
                    .get(&(clip_id, crate::state::AudioClipInspectorField::Transpose))
                    == Some(&expected)
                    && self
                        .state
                        .active_timeline_editor()
                        .audio_clip_transpose_debounce
                        .get(&clip_id)
                        == Some(&expected_revision);
                if !still_current {
                    return Task::none();
                }
                return self.update(Message::Arrangement(
                    ArrangementMsg::SubmitAudioClipInspectorField {
                        track_id,
                        clip_id,
                        field: crate::state::AudioClipInspectorField::Transpose,
                    },
                ));
            }

            // -- Undo / redo --

            // -- Export --
            Message::ExportProject => {
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
                self.finish_export_runtime();
                self.state.status_text = format!("Exported: {}", path.display());
            }
            Message::ExportComplete(Err(err)) => {
                self.finish_export_runtime();
                self.state.status_text =
                    format!("Export failed — {err}. No destination WAV was written.");
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
            Message::SelectAllPressed => return self.on_select_all_pressed(),
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

            Message::ToggleAudioTrackArm(track_id) => {
                return self.toggle_audio_track_arm(track_id);
            }
            Message::SetAudioTrackInputRoute(track_id, route) => {
                return self.set_audio_track_input_route(track_id, route);
            }
            Message::SetAudioTrackMonitoring(track_id, monitoring) => {
                return self.set_audio_track_monitoring(track_id, monitoring);
            }
            Message::ToggleAudioRecording => return self.toggle_audio_recording(),
            Message::AudioRecordingFinalized(result) => {
                return self.finish_audio_recording(result);
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
            Message::SetInterfaceScale(scale) => {
                self.interface_scale = crate::ui_settings::clamp_interface_scale(scale);
                self.persist_ui_settings();
            }
            Message::ToggleProjectTrackDeleteConfirmation => {
                self.state.confirm_project_track_deletion =
                    !self.state.confirm_project_track_deletion;
                self.persist_ui_settings();
            }
            Message::ToggleAutoSave => {
                self.state.auto_save_enabled = !self.state.auto_save_enabled;
                self.save_runtime.set_auto_save_enabled(
                    self.state.auto_save_enabled,
                    self.state.project.dirty && self.state.project.current_path.is_some(),
                    std::time::Instant::now(),
                );
                self.persist_ui_settings();
            }
            Message::ToggleCheckForUpdates => {
                self.state.update_check.enabled = !self.state.update_check.enabled;
                self.persist_ui_settings();
            }
            Message::CheckForUpdatesNow => {
                if self.state.update_check.begin_check() {
                    return Task::perform(
                        crate::update_check::fetch_latest_tag(),
                        Message::UpdateCheckCompleted,
                    );
                }
            }
            Message::UpdateCheckCompleted(tag) => {
                self.state
                    .update_check
                    .record_result(tag, crate::update_check::now_unix());
                // Persist so the throttle survives a restart, which is
                // the whole point of recording failures too.
                self.persist_ui_settings();
            }
            Message::DismissUpdateNotice => {
                self.state.update_check.dismissed = true;
            }
            Message::OpenReleasesPage => {
                // Retiring the notice once the URL has been handed off
                // keeps a single mechanism for hiding it.
                return Task::perform(crate::update_check::open_releases_page(), |()| {
                    Message::DismissUpdateNotice
                });
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
            Message::BrowserAuditionWarpReady {
                source,
                generation,
                project_bpm,
                result,
            } => {
                return self.on_browser_audition_warp_ready(source, generation, project_bpm, result)
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
            Message::RemoteCatalogStartupLoaded(result) => {
                return self.on_remote_catalog_startup_loaded(result)
            }
            Message::RefreshRemoteConnection => {
                return self.handle_remote_catalog_refresh();
            }
            Message::RemoteCatalogPageFetched {
                generation,
                completed_pages,
                result,
            } => return self.on_remote_catalog_page_fetched(generation, completed_pages, result),
            Message::RemoteCatalogRefreshPrepared(result) => {
                return self.on_remote_catalog_refresh_prepared(result)
            }
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
mod audio_recording_transport_tests {
    use super::*;

    #[test]
    fn stop_and_space_finalize_an_active_audio_recording() {
        assert_eq!(
            audio_recording_transport_guard(
                crate::domains::audio_recording::AudioRecordingPhase::Recording,
                &TransportMsg::Stop,
            ),
            AudioRecordingTransportGuard::StopRecording
        );
        assert_eq!(
            audio_recording_transport_guard(
                crate::domains::audio_recording::AudioRecordingPhase::Recording,
                &TransportMsg::TogglePlayback,
            ),
            AudioRecordingTransportGuard::StopRecording
        );
    }

    #[test]
    fn recording_blocks_seek_loop_and_tempo_changes_before_the_transport_domain() {
        for message in [
            TransportMsg::Seek(0.5),
            TransportMsg::SeekToBeat(8.0),
            TransportMsg::ToggleArrangementLoop,
            TransportMsg::SetArrangementLoopRegion {
                start_beats: 4.0,
                end_beats: 8.0,
            },
            TransportMsg::BpmSubmit,
            TransportMsg::NudgeBpm(1.0),
        ] {
            assert_eq!(
                audio_recording_transport_guard(
                    crate::domains::audio_recording::AudioRecordingPhase::Recording,
                    &message,
                ),
                AudioRecordingTransportGuard::BlockTimelineChange
            );
        }
    }

    #[test]
    fn the_internal_stop_passes_to_transport_after_recording_enters_stopping() {
        assert_eq!(
            audio_recording_transport_guard(
                crate::domains::audio_recording::AudioRecordingPhase::Stopping,
                &TransportMsg::Stop,
            ),
            AudioRecordingTransportGuard::Pass
        );
    }
}
