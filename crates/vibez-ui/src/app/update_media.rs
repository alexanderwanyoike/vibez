//! Media, sample-browser, and engine-event message handlers.
//! Split from update.rs; part of the App::update dispatch chain.

use std::sync::Arc;

use iced::Task;

use crate::domains::arrangement::ArrangementMsg;
use crate::domains::browser::BrowserMsg;
use crate::domains::piano_roll::PianoRollMsg;
use crate::domains::view::ViewMsg;
use vibez_core::track::MediaSourceRef;
use vibez_engine::commands::EngineCommand;

use crate::message::{BrowserImportTarget, Message};

use super::*;

impl App {
    pub(super) fn update_media(&mut self, message: Message) -> Task<Message> {
        match message {
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
                            .add_filter(
                                "Supported Audio",
                                vibez_core::audio_format::SUPPORTED_AUDIO_EXTENSIONS,
                            )
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
                            .add_filter(
                                "Supported Audio",
                                vibez_core::audio_format::SUPPORTED_AUDIO_EXTENSIONS,
                            )
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
                            let generation = self.state.browser.begin_audition_load(&source);
                            self.state.status_text = "Preparing Audition...".to_string();
                            return Task::perform(
                                decode_local_for_preview_async(path),
                                move |result| {
                                    Message::LocalSamplePreviewReady(
                                        source.clone(),
                                        generation,
                                        result,
                                    )
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
                let generation = self.state.browser.begin_audition_load(&source);
                if let MediaSourceRef::LocalFile { path } = source.clone() {
                    self.state.status_text = "Preparing Audition...".to_string();
                    return Task::perform(decode_local_for_preview_async(path), move |result| {
                        Message::LocalSamplePreviewReady(source.clone(), generation, result)
                    });
                }
            }
            Message::StopBrowserPreview => {
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
                return self.media_cache_maintenance_task();
            }
            Message::ToggleAuditionEnabled => {
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
                return maintenance;
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
                } else if self.remote_import_active() {
                    if let Some(handle) = self.remote_import_abort.take() {
                        handle.abort();
                    }
                    self.remote_import_request_id = self.remote_import_request_id.saturating_add(1);
                    self.browser_import_generation = self.browser_import_generation.wrapping_add(1);
                    self.remote_import_in_flight = None;
                    self.state.status_text =
                        "Remote import cancelled; no project media added".into();
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
            Message::LocalSamplePreviewReady(source, generation, Ok(audio)) => {
                if self
                    .state
                    .browser
                    .install_audition(generation, source, Arc::clone(&audio))
                {
                    let source = self.state.browser.selected_source.clone().unwrap();
                    return self.play_browser_mode(source, audio);
                }
            }
            Message::LocalSamplePreviewReady(source, generation, Err(err)) => {
                let is_current = self.state.browser.selected_source.as_ref() == Some(&source)
                    && self.state.browser.audition_request_is_current(generation);
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
            }
            Message::BrowserAuditionWarpReady {
                source,
                generation,
                source_bpm,
                project_bpm,
                result,
            } => {
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
                // The active import's own completion has already cleared
                // `remote_import_in_flight` (RemoteImportReady). A remote
                // source decoded while another import is still running (e.g.
                // a dropped staged copy) must not run its cleanup or release
                // the queued audition.
                let remote_import_finished = matches!(
                    &source,
                    MediaSourceRef::DropboxFile { .. }
                        | MediaSourceRef::StagedRemoteProjectMedia { .. }
                ) && !self.remote_import_active();
                let maintenance = if remote_import_finished {
                    self.media_cache_maintenance_task()
                } else {
                    Task::none()
                };
                let import =
                    self.prepare_browser_sample_import(target, treatment, audio, name, source);
                let pending_audition = if remote_import_finished {
                    self.pending_remote_audition
                        .take()
                        .map(|entry| self.start_remote_audition(entry, true))
                        .unwrap_or_else(Task::none)
                } else {
                    Task::none()
                };
                return Task::batch([import, pending_audition, maintenance]);
            }
            Message::RemoteImportReady {
                request_id,
                target,
                treatment,
                result,
            } => {
                if !dropbox_io::remote_import_result_is_current(
                    self.remote_import_request_id,
                    request_id,
                ) {
                    // Staging copies are content-addressed, so a superseding
                    // import of the same bytes shares this exact file;
                    // deleting it here would break that import's next save.
                    // The startup staging sweep owns orphan cleanup.
                    return Task::none();
                }
                self.remote_import_abort = None;
                self.remote_import_in_flight = None;
                return match result {
                    Ok((audio, name, source)) => self.update(Message::BrowserSampleDecoded(
                        target, treatment, audio, name, source,
                    )),
                    Err(error) => Task::batch([
                        self.media_cache_maintenance_task(),
                        self.update(Message::BrowserSampleDecodeError(error)),
                    ]),
                };
            }
            Message::BrowserImportPrepared {
                target,
                generation,
                payload,
            } => {
                if generation != self.browser_import_generation {
                    return Task::none();
                }
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
                // Release the queued audition only when no import is active:
                // a local decode failure must not preempt a Remote import
                // that is still downloading.
                if !self.remote_import_active() {
                    if let Some(entry) = self.pending_remote_audition.take() {
                        return self.start_remote_audition(entry, true);
                    }
                }
            }

            other => return self.update_remote(other),
        }
        Task::none()
    }
}
