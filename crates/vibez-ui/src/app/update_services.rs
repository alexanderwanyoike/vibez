//! The message router: one exhaustive match, bodies delegated to
//! domain updates and topic-module handlers.

use iced::Task;

use crate::domains::browser::BrowserMsg;
use vibez_engine::commands::EngineCommand;
use vibez_plugin_host::gui::PluginGuiKey;

use crate::services::plugin_loader::{load_plugin_effect_bg, load_plugin_instrument_bg};

use crate::message::Message;

use super::*;

impl App {
    pub(super) fn update_services(&mut self, message: Message) -> Task<Message> {
        match message {
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
                        let token = self
                            .plugin_load_requests
                            .begin(PluginGuiKey::Instrument { track_id });
                        let tx = self.plugin_instrument_tx.clone();
                        std::thread::spawn(move || {
                            match load_plugin_instrument_bg(&info, sample_rate, None) {
                                Ok(mut result) => {
                                    result.load_token = token;
                                    result.track_id = track_id;
                                    let _ = tx.send(result);
                                }
                                Err(e) => {
                                    eprintln!("Plugin load error: {e}");
                                }
                            }
                        });
                    } else {
                        let effect_id = vibez_core::id::EffectId::new();
                        let token = self.plugin_load_requests.begin(PluginGuiKey::Effect {
                            track_id,
                            effect_id,
                        });
                        let tx = self.plugin_effect_tx.clone();
                        std::thread::spawn(move || {
                            match load_plugin_effect_bg(&info, sample_rate, None) {
                                Ok(mut result) => {
                                    result.load_token = token;
                                    result.track_id = track_id;
                                    result.effect_id = effect_id;
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
                            "Native window system unavailable: plugin GUI cannot open".to_string();
                    }
                } else {
                    self.state.status_text = "Plugin GUI handle not available".to_string();
                }
            }
            Message::ClosePluginGui(key) => {
                return self.handle_close_plugin_gui(key);
            }

            // -- Bounce / resample --
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
            _ => unreachable!("service messages are routed by App::update"),
        }
        Task::none()
    }
}
