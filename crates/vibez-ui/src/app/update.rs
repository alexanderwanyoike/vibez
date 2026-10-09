//! The message router: one exhaustive match, bodies delegated to
//! domain updates and topic-module handlers.

use iced::Task;

use crate::domains::arrangement::ArrangementMsg;
use crate::domains::project::ProjectMsg;
use crate::domains::transport::TransportMsg;
use vibez_engine::commands::EngineCommand;

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
    pub(super) fn update_and_refresh_clips(&mut self, message: Message) -> Task<Message> {
        let before = Arc::clone(&self.state.perform.clips);
        let recording = self
            .state
            .perform
            .clip_record
            .session
            .as_ref()
            .map(|s| s.working.id);
        let task = self.update(message);
        self.sync_sidechain_routing();
        let spb = self.state.transport.samples_per_beat();
        super::clip_launcher::refresh_edited_clips(
            &before,
            &mut self.state.perform,
            recording,
            spb,
            &mut self.cmd_tx,
        );
        task
    }

    pub(super) fn update(&mut self, message: Message) -> Task<Message> {
        let message = match message {
            Message::SectionTimeline(edit) => {
                if super::update_timeline::section_timeline_claims_focus(
                    self.state.view.workspace,
                    self.state.perform.selected_section.is_some(),
                ) {
                    self.state.perform.editor_focus =
                        crate::domains::perform::PerformEditorFocus::TimelineEditor;
                }
                *edit
            }
            message => message,
        };
        let message = if self.state.view.workspace == crate::state::Workspace::Perform
            && self.state.perform.layout == vibez_project::PerformLayout::Clips
            && matches!(
                &message,
                Message::Arrangement(
                    ArrangementMsg::DuplicateSelectedClip | ArrangementMsg::DuplicateNoteClip(..)
                )
            ) {
            let Some(id) = self.state.perform.clip_editor.selected else {
                return Task::none();
            };
            Message::Perform(PerformMsg::Clips(
                crate::domains::perform::ClipMsg::Duplicate(id),
            ))
        } else {
            message
        };
        let (message, undo_gesture) = match message {
            Message::UndoGesture { id, edit } => (*edit, Some(id)),
            message => (message, None),
        };
        let message =
            apply_project_track_deletion_policy(message, self.state.confirm_project_track_deletion);
        if self.state.perform.clip_record.is_active()
            && matches!(
                &message,
                Message::Perform(PerformMsg::Clips(
                    crate::domains::perform::ClipMsg::Delete(_)
                        | crate::domains::perform::ClipMsg::ToggleLoop(_)
                )) | Message::Project(ProjectMsg::Undo | ProjectMsg::Redo)
                    | Message::SaveProject
                    | Message::SaveProjectAs
                    | Message::Arrangement(
                        ArrangementMsg::RequestRemoveTrack(_)
                            | ArrangementMsg::RemoveTrack(_)
                            | ArrangementMsg::ConfirmRemoveTrack(_)
                    )
            )
        {
            self.state.status_text =
                "Finish the Clip take before editing playback mode, deleting, saving or using Undo"
                    .into();
            return Task::none();
        }
        if self.state.perform.clip_record.is_active()
            && matches!(
                &message,
                Message::Transport(TransportMsg::Stop | TransportMsg::TogglePlayback)
            )
        {
            if self.state.perform.clip_record.pending_audio_arm.is_some() {
                self.cancel_clip_record();
            } else {
                self.send_command(EngineCommand::StopClipRecord { immediate: true });
            }
            self.send_command(EngineCommand::Stop);
            return Task::none();
        }
        if self.state.perform.clip_record.is_active()
            && matches!(&message, Message::Perform(PerformMsg::Capture(_)))
        {
            self.state.status_text = "Finish Clip recording before Capture".into();
            return Task::none();
        }
        if let Message::Transport(transport) = &message {
            if self.state.perform.clip_record.is_active()
                && audio_recording_transport_guard(
                    crate::domains::audio_recording::AudioRecordingPhase::Recording,
                    transport,
                ) == AudioRecordingTransportGuard::BlockTimelineChange
            {
                self.state.status_text =
                    "Finish Clip recording before changing position, loop, or tempo".into();
                return Task::none();
            }
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
        if self.reject_invalid_send_edit(&message) {
            return Task::none();
        }
        let owns_project_transaction = self.begin_project_track_deletion_transaction(&message);
        let deferred_arrangement_project_edit = matches!(
            &message,
            Message::Arrangement(msg) if msg.defers_project_edit()
        ) || matches!(
            &message,
            Message::Devices(
                crate::domains::devices::DevicesMsg::SetSidechainSource { .. }
                    | crate::domains::devices::DevicesMsg::SetSidechainTap { .. }
            )
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
                if self.state.view.workspace == crate::state::Workspace::Perform
                    && self.state.perform.layout == vibez_project::PerformLayout::Clips
                    && !self.state.transport.playing
                    && matches!(msg, crate::domains::transport::TransportMsg::TogglePlayback)
                {
                    self.state.perform.clip_editor.running = true;
                    self.send_command(vibez_engine::commands::EngineCommand::BeginClipPerformance);
                    return Task::none();
                }
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
                let perform_playback_active = self.state.perform.clip_editor.running
                    || self.state.perform.playing_section.is_some()
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
                if let crate::domains::devices::DevicesMsg::SetTrackInstrument(track_id, _)
                | crate::domains::devices::DevicesMsg::RemoveTrackInstrument(track_id) = &msg
                {
                    self.plugin_load_requests.cancel(PluginGuiKey::Instrument {
                        track_id: *track_id,
                    });
                }
                let routing_edit = matches!(
                    msg,
                    crate::domains::devices::DevicesMsg::SetSidechainSource { .. }
                        | crate::domains::devices::DevicesMsg::SetSidechainTap { .. }
                );
                let routing = routing_edit.then(|| self.sidechain_model());
                let snapshot = routing_edit.then(|| self.take_snapshot());
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
                        crate::domains::devices::DevicesCtx {
                            routing: routing.as_deref(),
                        },
                    )
                };
                if let (Some(true), Some(snapshot)) = (action.routing_changed, snapshot) {
                    self.state.project.history.push_edit(snapshot, undo_gesture);
                    self.mark_project_dirty();
                }
                self.apply_devices_action(action);
            }
            Message::SetDrumRackSliceMarkers(markers) => {
                if let Some(dialog) = self.state.view.drum_rack_slice_dialog.as_mut() {
                    dialog.markers = markers;
                }
            }
            Message::CancelDrumRackSlice => {
                self.state.view.drum_rack_slice_dialog = None;
            }
            Message::OpenTransientAnalysisDialog {
                location,
                track_id,
                clip_id,
            } => {
                let clip_exists = self
                    .timeline_content_at(location, track_id)
                    .is_some_and(|content| content.clips.iter().any(|clip| clip.id == clip_id));
                if clip_exists {
                    self.state.view.transient_analysis_dialog =
                        Some(crate::state::TransientAnalysisDialog {
                            location,
                            track_id,
                            clip_id,
                            sensitivity: vibez_core::onset::TransientSensitivity::DEFAULT,
                            sensitivity_input: vibez_core::onset::TransientSensitivity::DEFAULT
                                .percent()
                                .to_string(),
                        });
                } else {
                    self.state.status_text = "The Audio Clip is no longer available".into();
                }
            }
            Message::SetTransientAnalysisSensitivity(percent) => {
                if let Some(dialog) = self.state.view.transient_analysis_dialog.as_mut() {
                    dialog.set_sensitivity(percent);
                }
            }
            Message::TransientAnalysisSensitivityInputChanged(input) => {
                if let Some(dialog) = self.state.view.transient_analysis_dialog.as_mut() {
                    dialog.edit_sensitivity_input(input);
                }
            }
            Message::SubmitTransientAnalysisSensitivity => {
                if let Some(dialog) = self.state.view.transient_analysis_dialog.as_mut() {
                    dialog.normalize_sensitivity_input();
                }
            }
            Message::CancelTransientAnalysis => {
                self.state.view.transient_analysis_dialog = None;
            }
            Message::ConfirmTransientAnalysis => {
                let Some(dialog) = self.state.view.transient_analysis_dialog.as_mut() else {
                    return Task::none();
                };
                if !dialog.commit_sensitivity_input() {
                    self.state.status_text = "Enter a sensitivity from 0 to 100%".into();
                    return Task::none();
                }
                let dialog = self
                    .state
                    .view
                    .transient_analysis_dialog
                    .take()
                    .expect("validated dialog remains open");
                return self.dispatch_detect_clip_transients(
                    dialog.location,
                    dialog.track_id,
                    dialog.clip_id,
                    dialog.sensitivity,
                    true,
                );
            }
            Message::ConfirmDrumRackSlice => {
                let Some(dialog) = self.state.view.drum_rack_slice_dialog.take() else {
                    return Task::none();
                };
                let Some(clip) = self
                    .timeline_content_at(dialog.location, dialog.track_id)
                    .and_then(|content| content.clips.iter().find(|clip| clip.id == dialog.clip_id))
                    .cloned()
                else {
                    self.state.status_text = "The Audio Clip is no longer available".into();
                    return Task::none();
                };
                let slice_count =
                    crate::domains::arrangement::slice_region_count(&clip, dialog.markers);
                if slice_count == 0 || slice_count > vibez_core::track::DRUM_RACK_PAD_COUNT {
                    self.state.view.drum_rack_slice_dialog = Some(dialog);
                    self.state.status_text = if slice_count == 0 {
                        "The selected marker type has no interior slices".into()
                    } else {
                        format!(
                            "{slice_count} slices exceed the {}-pad Drum Rack",
                            vibez_core::track::DRUM_RACK_PAD_COUNT,
                        )
                    };
                    return Task::none();
                }
                self.state.status_text = "Preparing Drum Rack slices…".into();
                let expected_clip = Box::new(clip.clone());
                return Task::perform(prepare_drum_rack_audio_async(clip), move |result| {
                    Message::AudioClipDrumRackPrepared {
                        location: dialog.location,
                        track_id: dialog.track_id,
                        clip_id: dialog.clip_id,
                        markers: dialog.markers,
                        expected_clip: expected_clip.clone(),
                        result,
                    }
                });
            }
            Message::Arrangement(msg) => {
                if matches!(msg, ArrangementMsg::DeleteSelectedClip) {
                    let location = self.active_timeline_location();
                    let deleted: std::collections::HashSet<_> = self
                        .state
                        .active_timeline_editor()
                        .selected_clips
                        .iter()
                        .filter_map(|selection| match selection {
                            crate::state::ArrangementSelection::AudioClip { track_id, clip_id } => {
                                Some((*track_id, *clip_id))
                            }
                            crate::state::ArrangementSelection::NoteClip { .. } => None,
                        })
                        .collect();
                    self.state.view.dismiss_clip_dialogs_for(location, &deleted);
                }
                if let ArrangementMsg::RequestSliceAudioClipToDrumRack { track_id, clip_id } = &msg
                {
                    let (track_id, clip_id) = (*track_id, *clip_id);
                    let location = self.active_timeline_location();
                    let Some(clip) = self
                        .timeline_content_at(location, track_id)
                        .and_then(|content| content.clips.iter().find(|clip| clip.id == clip_id))
                        .cloned()
                    else {
                        return Task::none();
                    };
                    if clip.source.is_none() {
                        self.state.status_text =
                            "Slice to Drum Rack needs available Source Media".into();
                        return Task::none();
                    }
                    let transient_count = crate::domains::arrangement::slice_region_count(
                        &clip,
                        crate::domains::arrangement::AudioSliceMarkers::Transients,
                    );
                    let warp_count = crate::domains::arrangement::slice_region_count(
                        &clip,
                        crate::domains::arrangement::AudioSliceMarkers::Warp,
                    );
                    if transient_count == 0 && warp_count == 0 {
                        self.state.status_text =
                            "Add an interior Transient or Warp marker before slicing".into();
                        return Task::none();
                    }
                    self.state.view.drum_rack_slice_dialog =
                        Some(crate::state::DrumRackSliceDialog {
                            location,
                            track_id,
                            clip_id,
                            markers: if transient_count > 0 {
                                crate::domains::arrangement::AudioSliceMarkers::Transients
                            } else {
                                crate::domains::arrangement::AudioSliceMarkers::Warp
                            },
                        });
                    return Task::none();
                }
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
            Message::AudioClipDrumRackPrepared {
                location,
                track_id,
                clip_id,
                markers,
                expected_clip,
                result,
            } => match result {
                Ok(prepared) => {
                    let current = self
                        .timeline_content_at(location, track_id)
                        .and_then(|content| content.clips.iter().find(|clip| clip.id == clip_id));
                    if self.active_timeline_location() != location
                        || !current
                            .is_some_and(|clip| clip.has_same_audible_geometry(&expected_clip))
                    {
                        self.state.status_text =
                            "Drum Rack slices cancelled because the Audio Clip changed".into();
                        return Task::none();
                    }
                    return self.update(Message::Arrangement(
                        ArrangementMsg::SliceAudioClipToDrumRack {
                            track_id,
                            clip_id,
                            markers,
                            source: prepared.source,
                            audio: prepared.audio,
                        },
                    ));
                }
                Err(error) => {
                    self.state.status_text = format!("Slice to Drum Rack failed: {error}");
                }
            },
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
            Message::SelectNewProjectLayout(layout) => {
                if self.state.project.new_project_layout.is_some() {
                    self.state.project.new_project_layout = Some(layout);
                }
            }
            Message::CancelNewProject => self.state.project.new_project_layout = None,
            Message::ConfirmNewProject => {
                if let Some(layout) = self.state.project.new_project_layout.take() {
                    self.reset_to_new_project();
                    self.state.perform.layout = layout;
                    self.state.perform.mode = crate::domains::perform::PerformMode::Sections;
                    self.state.view.workspace = crate::state::Workspace::Perform;
                    self.state.status_text = format!("New {} project", layout.label());
                }
            }
            Message::ImportLauncherClip { track_id, row } => {
                if self.state.perform.layout != vibez_project::PerformLayout::Clips {
                    return Task::none();
                }
                let source = self
                    .state
                    .browser
                    .drag_source
                    .take()
                    .or_else(|| self.state.browser.selected_source.clone());
                self.state.browser.cancel_media_drag();
                if let Some(source) = source {
                    return self.dispatch_drop_for_target(
                        source,
                        crate::message::BrowserImportTarget::LauncherClipAt { track_id, row },
                    );
                }
                self.state.status_text =
                    "Select a sample in the Browser, then add it to a Clip slot".into();
                self.state.browser.open = true;
            }
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
            message @ (Message::OpenAbout
            | Message::OpenUrl(..)
            | Message::UrlOpened(..)
            | Message::WindowCloseRequested
            | Message::CloseConfirmSave
            | Message::CloseConfirmDiscard
            | Message::CloseConfirmCancel
            | Message::OpenSettings
            | Message::CloseSettings
            | Message::SelectSettingsTab(..)
            | Message::SelectAudioBackend(..)
            | Message::SetBufferSize(..)
            | Message::SetAudioSampleRate(..)
            | Message::SelectAudioInput(..)
            | Message::SelectAudioOutput(..)
            | Message::RescanAudioDevices
            | Message::ReconnectAudioOutput
            | Message::ScanPlugins
            | Message::ScanPluginsComplete(..)
            | Message::AddPluginScanPath
            | Message::PluginScanPathSelected(..)
            | Message::RemovePluginScanPath(..)
            | Message::ToggleScanDefaultPaths
            | Message::AddPluginToTrack(..)
            | Message::PluginLoadError(..)
            | Message::OpenPluginGui(..)
            | Message::ClosePluginGui(..)
            | Message::ToggleCheckForUpdates
            | Message::CheckForUpdatesNow
            | Message::UpdateCheckCompleted(..)
            | Message::DismissUpdateNotice
            | Message::OpenReleasesPage
            | Message::RescanMidiInputs
            | Message::OpenMidiInput(..)
            | Message::CloseMidiInput
            | Message::SelectTheme(..)
            | Message::RescanThemes
            | Message::ThemeSaveNameChanged(..)
            | Message::SaveCurrentTheme
            | Message::RewarpAllClips
            | Message::AddSampleLibraryRoot
            | Message::SampleLibraryRootSelected(..)
            | Message::RescanSampleLibrary
            | Message::ClickLocalBrowserEntry(..)
            | Message::BeginPendingBrowserDrag(..)
            | Message::PreviewLocalEntry(..)
            | Message::StopBrowserPreview
            | Message::ToggleAuditionEnabled
            | Message::SetAuditionGain(..)
            | Message::SetAuditionMode(..)
            | Message::EscapePressed
            | Message::DropSampleOnArrangement { .. }
            | Message::DropSampleOnEmptyArrangement
            | Message::DropSampleOnDrumPad { .. }
            | Message::DropSampleOnSampler { .. }
            | Message::LocalSamplePreviewReady(..)
            | Message::BrowserWaveformReady(..)
            | Message::BrowserAuditionWarpReady { .. }
            | Message::ImportSelectedBrowserSampleToArrangement
            | Message::SelectAdjacentBrowserResult(..)
            | Message::LoadSelectedBrowserSampleToDevice
            | Message::BrowserSampleDecoded(..)
            | Message::RemoteImportReady { .. }
            | Message::BrowserImportPrepared { .. }
            | Message::ClipAutoWarpReady { .. }
            | Message::BrowserSampleDecodeError(..)
            | Message::SaveDropboxAppKey
            | Message::ConnectDropbox
            | Message::DropboxConnected(..)
            | Message::DisconnectDropbox
            | Message::RemoteCatalogStartupLoaded(..)
            | Message::RefreshRemoteConnection
            | Message::RemoteCatalogPageFetched { .. }
            | Message::RemoteCatalogRefreshPrepared(..)
            | Message::RemoteCatalogSaved { .. }
            | Message::SetMediaCacheBudgetGiB(..)
            | Message::ToggleMediaCacheAutomaticEviction
            | Message::MediaCacheMaintenanceComplete(..)
            | Message::ClearMediaCache
            | Message::MediaCacheCleared(..)
            | Message::ClickRemoteBrowserEntry(..)
            | Message::RemoteAuditionReady { .. }
            | Message::DropboxPreview(..)
            | Message::DropboxImportToArrangement(..)
            | Message::DropboxImportToDevice(..)) => return self.update_services(message),
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
                self.finish_export_runtime();
                self.finalize_bounce(outcome);
            }
            Message::BounceComplete(Err(err)) => {
                self.finish_export_runtime();
                self.state.status_text = format!("Bounce error: {err}");
            }

            // -- Quantize --
            message @ (Message::QuantizeAudioClip { .. }
            | Message::QuantizeAudioClipAt { .. }
            | Message::AudioQuantizeReady { .. }
            | Message::DetectClipBpm { .. }
            | Message::DetectClipTransients { .. }
            | Message::ClipTransientsDetected(..)
            | Message::ClipBpmDetected { .. }
            | Message::WarpClipToProject { .. }
            | Message::ClipWarpReady { .. }
            | Message::ClipTransposeReady { .. }
            | Message::CommitAudioClipTransposeAfterDelay { .. }) => {
                return self.update_audio_edits(message, undo_gesture)
            }
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
        }
        Task::none()
    }
}

#[cfg(test)]
#[path = "update_tests.rs"]
mod audio_recording_transport_tests;
