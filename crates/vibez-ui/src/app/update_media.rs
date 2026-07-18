//! Media, sample-browser, and engine-event message handlers.
//! Split from update.rs; each method backs one arm of the App::update match.

use std::path::PathBuf;
use std::sync::Arc;

use iced::Task;

use crate::domains::arrangement::ArrangementMsg;
use crate::domains::browser::BrowserMsg;
use crate::domains::piano_roll::PianoRollMsg;
use crate::domains::view::ViewMsg;
use vibez_core::id::{ClipId, TrackId};
use vibez_core::track::MediaSourceRef;
use vibez_engine::commands::EngineCommand;

use crate::message::{BrowserImportTarget, Message};

use super::*;

impl App {
    pub(super) fn on_delete_key_pressed(&mut self) -> Task<Message> {
        // Never delete anything while a text field is being
        // edited; backspace belongs to the text there.
        if self.state.view.editing_track_name.is_some()
            || self.state.view.editing_clip_name.is_some()
            || self.state.perform.editing_section_name.is_some()
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
                .arrange_content(track_id)
                .and_then(|content| content.note_clips.iter().find(|c| c.id == clip_id))
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
        Task::none()
    }

    pub(super) fn on_load_sampler_sample(&mut self, track_id: TrackId) -> Task<Message> {
        Task::perform(
            async {
                let handle = rfd::AsyncFileDialog::new()
                    .set_title("Load Sample")
                    .add_filter(
                        "Supported Audio",
                        vibez_core::audio_format::SUPPORTED_AUDIO_EXTENSIONS,
                    )
                    .pick_file()
                    .await;
                handle.map(|h| h.path().to_path_buf())
            },
            move |path| Message::SamplerFileSelected(track_id, path),
        )
    }

    pub(super) fn on_sampler_file_selected(
        &mut self,
        track_id: TrackId,
        path: Option<PathBuf>,
    ) -> Task<Message> {
        if let Some(path) = path {
            let file_name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            self.state.status_text = format!("Loading {file_name}...");
            return Task::perform(
                decode_and_stage_local_async(path),
                move |result| match result {
                    Ok((audio, source)) => Message::SamplerSampleDecoded(
                        track_id,
                        Arc::new(audio),
                        file_name.clone(),
                        source,
                    ),
                    Err(e) => Message::SamplerDecodeError(track_id, e),
                },
            );
        }
        Task::none()
    }

    pub(super) fn on_load_drum_rack_pad_sample(
        &mut self,
        track_id: TrackId,
        pad_index: usize,
    ) -> Task<Message> {
        Task::perform(
            async {
                let handle = rfd::AsyncFileDialog::new()
                    .set_title("Load Drum Pad Sample")
                    .add_filter(
                        "Supported Audio",
                        vibez_core::audio_format::SUPPORTED_AUDIO_EXTENSIONS,
                    )
                    .pick_file()
                    .await;
                handle.map(|h| h.path().to_path_buf())
            },
            move |path| Message::DrumRackPadFileSelected(track_id, pad_index, path),
        )
    }

    pub(super) fn on_rescan_midi_inputs(&mut self) -> Task<Message> {
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
        Task::none()
    }

    pub(super) fn on_open_midi_input(&mut self, name: String) -> Task<Message> {
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
        Task::none()
    }

    pub(super) fn on_select_theme(&mut self, name: String) -> Task<Message> {
        if let Some(palette) = self.resolve_theme(&name) {
            th::set_theme(palette);
            self.state.current_theme_name = name.clone();
            self.persist_ui_settings();
            self.state.status_text = format!("Theme: {name}");
        } else {
            self.state.status_text = format!("Theme {name:?} not found");
        }
        Task::none()
    }

    pub(super) fn on_rescan_themes(&mut self) -> Task<Message> {
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
        Task::none()
    }

    pub(super) fn on_save_current_theme(&mut self) -> Task<Message> {
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
        Task::none()
    }

    pub(super) fn on_add_sample_library_root(&mut self) -> Task<Message> {
        Task::perform(
            async {
                let handle = rfd::AsyncFileDialog::new()
                    .set_title("Add Sample Library Root")
                    .pick_folder()
                    .await;
                handle.map(|folder| folder.path().to_path_buf())
            },
            Message::SampleLibraryRootSelected,
        )
    }

    pub(super) fn on_sample_library_root_selected(
        &mut self,
        path: Option<PathBuf>,
    ) -> Task<Message> {
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
        Task::none()
    }

    pub(super) fn on_rescan_sample_library(&mut self) -> Task<Message> {
        self.state.status_text = "Rescanning sample library...".to_string();
        let roots = self.state.browser.roots.clone();
        Task::batch(roots.into_iter().map(|root| {
            let revision = self.state.browser.begin_root_scan(&root, false);
            Task::perform(scan_sample_root_async(root.clone()), move |result| {
                Message::Browser(BrowserMsg::LocalRootCatalogReconciled {
                    root: root.clone(),
                    revision,
                    result,
                })
            })
        }))
    }

    pub(super) fn on_click_local_browser_entry(&mut self, source: MediaSourceRef) -> Task<Message> {
        if self.state.browser.drag_source.is_some() {
            self.state.browser.cancel_media_drag();
            self.state.status_text = "Drag cancelled".into();
            return Task::none();
        }
        let changed = self.state.browser.select_source(source.clone());
        if changed {
            if let MediaSourceRef::LocalFile { path } = source.clone() {
                if self.state.browser.audition_enabled {
                    let generation = self.state.browser.begin_audition_load(&source);
                    self.state.status_text = "Preparing Audition...".to_string();
                    return Task::perform(decode_local_for_preview_async(path), move |result| {
                        Message::LocalSamplePreviewReady(source.clone(), generation, result)
                    });
                }
                self.state.browser.begin_waveform_load(&source);
                return Task::perform(decode_local_for_preview_async(path), move |result| {
                    Message::BrowserWaveformReady(source.clone(), result)
                });
            }
        }
        Task::none()
    }

    pub(super) fn on_preview_local_entry(&mut self, source: MediaSourceRef) -> Task<Message> {
        self.state.browser.select_source(source.clone());
        let generation = self.state.browser.begin_audition_load(&source);
        if let MediaSourceRef::LocalFile { path } = source.clone() {
            self.state.status_text = "Preparing Audition...".to_string();
            return Task::perform(decode_local_for_preview_async(path), move |result| {
                Message::LocalSamplePreviewReady(source.clone(), generation, result)
            });
        }
        Task::none()
    }

    pub(super) fn on_stop_browser_preview(&mut self) -> Task<Message> {
        if let Some(handle) = self.remote_materialization_abort.take() {
            handle.abort();
            self.remote_materialization_request_id =
                self.remote_materialization_request_id.saturating_add(1);
        }
        self.remote_audition_cache_lease = None;
        self.state.browser.remote.preview_in_progress = false;
        self.stop_browser_audition();
        self.state.status_text = "Audition stopped".to_string();
        // Re-enforce the cache budget off the update thread now that
        // the audition lease is released.
        self.media_cache_maintenance_task()
    }

    pub(super) fn on_toggle_audition_enabled(&mut self) -> Task<Message> {
        let enabled = self.state.browser.toggle_audition_enabled();
        let maintenance = if !enabled {
            if let Some(handle) = self.remote_materialization_abort.take() {
                handle.abort();
                self.remote_materialization_request_id =
                    self.remote_materialization_request_id.saturating_add(1);
            }
            self.remote_audition_cache_lease = None;
            self.stop_browser_audition();
            self.media_cache_maintenance_task()
        } else {
            Task::none()
        };
        self.persist_ui_settings();
        self.state.status_text = if enabled {
            "Selection Audition enabled".into()
        } else {
            "Selection Audition disabled; explicit Play remains available".into()
        };
        maintenance
    }

    pub(super) fn on_set_audition_mode(
        &mut self,
        mode: crate::state::AuditionMode,
    ) -> Task<Message> {
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
        self.play_browser_mode(source, raw)
    }

    pub(super) fn on_set_audition_sync(
        &mut self,
        sync: vibez_engine::commands::AuditionSync,
    ) -> Task<Message> {
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
        Task::none()
    }

    pub(super) fn on_toggle_audition_loop(&mut self) -> Task<Message> {
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
        Task::none()
    }

    pub(super) fn on_audition_bpm_edit_changed(&mut self, value: String) -> Task<Message> {
        self.state.browser.audition_bpm_edit = value;
        self.state.browser.audition_bpm_confirmed = None;
        if self.state.browser.audition_mode == crate::state::AuditionMode::Warp
            && (self.state.browser.audition_playing || self.state.browser.audition_queued)
        {
            self.stop_browser_audition();
            self.state.status_text = "Confirm the edited source BPM for WARP".into();
        }
        Task::none()
    }

    pub(super) fn on_confirm_audition_bpm(&mut self) -> Task<Message> {
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
        Task::none()
    }

    pub(super) fn on_escape_pressed(&mut self) -> Task<Message> {
        // Escape abandons any pending browser import at whatever stage
        // it is in. The fetch/decode stage is covered by the abort
        // handle below, but the WARP-preparation stage has no handle
        // (remote_import_in_flight is already cleared), so only this
        // generation bump stops BrowserImportPrepared from landing a
        // clip after the user cancelled.
        self.browser_import_generation = self.browser_import_generation.wrapping_add(1);
        if self.state.browser.audition_playing
            || self.state.browser.audition_loading
            || self.state.browser.audition_queued
        {
            self.stop_browser_audition();
            self.state.status_text = "Audition stopped".into();
        } else if self.remote_import_active() {
            if let Some(handle) = self.remote_import_abort.take() {
                handle.abort();
            }
            self.remote_import_request_id = self.remote_import_request_id.saturating_add(1);
            self.remote_import_in_flight = None;
            self.state.status_text = "Remote import cancelled; no project media added".into();
            let maintenance = self.media_cache_maintenance_task();
            if let Some(entry) = self.pending_remote_audition.take() {
                return Task::batch([maintenance, self.start_remote_audition(entry, true)]);
            }
            return maintenance;
        } else if self.state.browser.drag_source.is_some()
            || self.state.browser.pending_drag.is_some()
        {
            self.state.browser.cancel_media_drag();
            self.state.status_text = "Drag cancelled".into();
        } else if self.state.perform.editing_section_name.is_some() {
            return self.update(Message::Perform(
                crate::domains::perform::PerformMsg::CancelSectionNameEdit,
            ));
        } else {
            return self.update(Message::View(ViewMsg::CancelEditing));
        }
        Task::none()
    }

    pub(super) fn on_drop_sample_on_arrangement(
        &mut self,
        track_id: TrackId,
        position_samples: u64,
    ) -> Task<Message> {
        let Some(source) = self.state.browser.drag_source.take() else {
            return Task::none();
        };
        self.state.browser.cancel_media_drag();
        self.dispatch_drop_on_arrangement(track_id, position_samples, source)
    }

    pub(super) fn on_drop_sample_on_empty_arrangement(&mut self) -> Task<Message> {
        let Some(source) = self.state.browser.drag_source.take() else {
            return Task::none();
        };
        let beat = match self.state.browser.drag_target.take() {
            Some(crate::state::BrowserDropTarget::EmptyArrangement { beat }) => beat,
            _ => self.state.position_beats(),
        };
        self.state.browser.cancel_media_drag();
        let position_samples = self.state.beats_to_samples(beat);
        self.dispatch_drop_for_target(
            source,
            BrowserImportTarget::ArrangementNewTrackAt { position_samples },
        )
    }

    pub(super) fn on_drop_sample_on_sampler(&mut self, track_id: TrackId) -> Task<Message> {
        let Some(source) = self.state.browser.drag_source.take() else {
            return Task::none();
        };
        self.state.browser.cancel_media_drag();
        self.dispatch_drop_for_target(source, BrowserImportTarget::Sampler(track_id))
    }

    pub(super) fn on_local_sample_preview_ready(
        &mut self,
        source: MediaSourceRef,
        generation: u64,
        audio: Arc<vibez_core::audio_buffer::DecodedAudio>,
    ) -> Task<Message> {
        if self
            .state
            .browser
            .install_audition(generation, source, Arc::clone(&audio))
        {
            let source = self.state.browser.selected_source.clone().unwrap();
            return self.play_browser_mode(source, audio);
        }
        Task::none()
    }

    pub(super) fn on_local_sample_preview_failed(
        &mut self,
        source: MediaSourceRef,
        generation: u64,
        err: String,
    ) -> Task<Message> {
        let is_current = self.state.browser.selected_source.as_ref() == Some(&source)
            && self.state.browser.audition_request_is_current(generation);
        self.state.browser.fail_waveform_load(&source, err.clone());
        if is_current {
            self.stop_browser_audition();
            self.state.status_text = format!("Audition error: {err}");
        }
        Task::none()
    }

    pub(super) fn on_browser_waveform_ready(
        &mut self,
        source: MediaSourceRef,
        audio: Arc<vibez_core::audio_buffer::DecodedAudio>,
    ) -> Task<Message> {
        if self
            .state
            .browser
            .install_waveform(source.clone(), Arc::clone(&audio))
        {
            return self.schedule_browser_bpm_detection(source, audio);
        }
        Task::none()
    }

    pub(super) fn on_browser_bpm_detected(
        &mut self,
        source: MediaSourceRef,
        estimate: Option<(f64, f32)>,
    ) -> Task<Message> {
        let source_for_warp = source.clone();
        // A BPM the user confirmed while detection was running
        // already drives the audition; the late estimate must
        // not restart it or rewrite the status line.
        let already_confirmed = self.state.browser.audition_bpm_confirmed.is_some();
        if self.state.browser.install_bpm_suggestion(
            source,
            estimate,
            self.state.warp_confidence_threshold,
        ) && self.state.browser.audition_mode == crate::state::AuditionMode::Warp
            && !already_confirmed
        {
            if let Some(source_bpm) = self.state.browser.audition_bpm_confirmed {
                let project_bpm = self.state.transport.bpm;
                self.state.status_text = format!(
                    "Detected {source_bpm:.1} source BPM; WARP targets project {project_bpm:.1} BPM"
                );
                if self.state.browser.audition_enabled {
                    if let Some(raw) = self.state.browser.waveform_audio.clone() {
                        self.stop_browser_audition();
                        return self.prepare_browser_warp(source_for_warp, raw, source_bpm);
                    }
                }
            } else {
                self.state.status_text = match estimate {
                    Some((bpm, confidence))
                        if confidence < self.state.warp_confidence_threshold =>
                    {
                        format!("Low-confidence suggestion {bpm:.1} BPM; confirm or edit for WARP")
                    }
                    Some((bpm, _)) => {
                        format!("Suggested {bpm:.1} BPM; confirm for WARP")
                    }
                    None => "No BPM detected; enter a positive source BPM for WARP".into(),
                };
            }
        }
        Task::none()
    }

    pub(super) fn on_browser_audition_warp_ready(
        &mut self,
        source: MediaSourceRef,
        generation: u64,
        source_bpm: f64,
        project_bpm: f64,
        result: Result<Arc<vibez_core::audio_buffer::DecodedAudio>, String>,
    ) -> Task<Message> {
        let current = self.state.browser.audition_request_is_current(generation)
            && self.state.browser.selected_source.as_ref() == Some(&source)
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
        Task::none()
    }

    pub(super) fn on_browser_sample_decoded(
        &mut self,
        target: BrowserImportTarget,
        treatment: crate::state::AuditionImportInput,
        audio: Arc<vibez_core::audio_buffer::DecodedAudio>,
        name: String,
        source: MediaSourceRef,
    ) -> Task<Message> {
        // The active import's own completion has already cleared
        // `remote_import_in_flight` (RemoteImportReady). A remote
        // source decoded while another import is still running (e.g.
        // a dropped staged copy) must not run its cleanup or release
        // the queued audition.
        let remote_import_finished = matches!(
            &source,
            MediaSourceRef::DropboxFile { .. } | MediaSourceRef::StagedRemoteProjectMedia { .. }
        ) && !self.remote_import_active();
        let maintenance = if remote_import_finished {
            self.media_cache_maintenance_task()
        } else {
            Task::none()
        };
        let import = self.prepare_browser_sample_import(target, treatment, audio, name, source);
        let pending_audition = if remote_import_finished {
            self.pending_remote_audition
                .take()
                .map(|entry| self.start_remote_audition(entry, true))
                .unwrap_or_else(Task::none)
        } else {
            Task::none()
        };
        Task::batch([import, pending_audition, maintenance])
    }

    pub(super) fn on_remote_import_ready(
        &mut self,
        request_id: u64,
        target: BrowserImportTarget,
        treatment: crate::state::AuditionImportInput,
        result: Result<
            (
                Arc<vibez_core::audio_buffer::DecodedAudio>,
                String,
                MediaSourceRef,
            ),
            String,
        >,
    ) -> Task<Message> {
        if !dropbox_io::remote_import_result_is_current(self.remote_import_request_id, request_id) {
            // Staging copies are content-addressed, so a superseding
            // import of the same bytes shares this exact file;
            // deleting it here would break that import's next save.
            // The startup staging sweep owns orphan cleanup.
            return Task::none();
        }
        self.remote_import_abort = None;
        self.remote_import_in_flight = None;
        match result {
            Ok((audio, name, source)) => self.update(Message::BrowserSampleDecoded(
                target, treatment, audio, name, source,
            )),
            Err(error) => Task::batch([
                self.media_cache_maintenance_task(),
                self.update(Message::BrowserSampleDecodeError(error)),
            ]),
        }
    }

    pub(super) fn on_browser_import_prepared(
        &mut self,
        target: BrowserImportTarget,
        generation: u64,
        payload: crate::message::PreparedBrowserImport,
    ) -> Task<Message> {
        if generation != self.browser_import_generation {
            return Task::none();
        }
        self.apply_browser_import_prepared(target, payload)
    }

    pub(super) fn on_clip_auto_warp_ready(
        &mut self,
        track_id: TrackId,
        clip_id: ClipId,
        outcome: crate::message::AutoWarpOutcome,
    ) -> Task<Message> {
        let action = {
            let mut engine = crate::domains::EngineTx(&mut self.cmd_tx);
            self.state
                .arrangement
                .apply_auto_warp_outcome(&mut engine, track_id, clip_id, outcome)
        };
        self.apply_arrangement_action(action)
    }

    pub(super) fn on_browser_sample_decode_error(&mut self, err: String) -> Task<Message> {
        self.state.status_text = format!("Browser import error: {err}");
        // Release the queued audition only when no import is active:
        // a local decode failure must not preempt a Remote import
        // that is still downloading.
        if !self.remote_import_active() {
            if let Some(entry) = self.pending_remote_audition.take() {
                return self.start_remote_audition(entry, true);
            }
        }
        Task::none()
    }
}
