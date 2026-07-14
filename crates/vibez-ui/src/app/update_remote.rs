//! Dropbox, remote-catalog, media-cache, and remote-browser message handlers.
//! Split from update.rs; tail of the App::update dispatch chain.

use std::sync::Arc;

use iced::Task;

use crate::domains::browser::BrowserMsg;
use vibez_core::track::MediaSourceRef;
use vibez_dropbox::{load_app_key_with_env_override, DropboxClient};

use crate::message::Message;

use super::*;

impl App {
    pub(super) fn update_remote(&mut self, message: Message) -> Task<Message> {
        match message {
            // -- Dropbox --
            Message::SaveDropboxAppKey => {
                let value = self.state.browser.remote.app_key_input.trim().to_string();
                self.dropbox_settings.app_key = if value.is_empty() { None } else { Some(value) };
                if let Err(err) = self.dropbox_settings.save() {
                    self.state.browser.remote.last_error = Some(format!("save settings: {err}"));
                }
                self.state.browser.remote.has_app_key =
                    load_app_key_with_env_override(&self.dropbox_settings).is_some();
                self.state.status_text = "Dropbox app key saved".to_string();
            }
            Message::ConnectDropbox => {
                return self.handle_connect_dropbox();
            }
            Message::DropboxConnected(Ok(outcome)) => {
                self.state.browser.remote.auth_in_progress = false;
                if let Some(app_key) = load_app_key_with_env_override(&self.dropbox_settings) {
                    let client = DropboxClient::new(app_key, outcome.tokens.clone());
                    self.dropbox_client = Some(Arc::new(client));
                }
                self.dropbox_settings.tokens = Some(outcome.tokens.clone());
                self.dropbox_settings.account_email = Some(outcome.info.email.clone());
                if let Err(err) = self.dropbox_settings.save() {
                    self.state.browser.remote.last_error = Some(format!("save settings: {err}"));
                }
                self.state.browser.remote.connected = true;
                self.state.browser.remote.account_email = Some(outcome.info.email.clone());
                self.state.status_text = format!("Dropbox connected: {}", outcome.info.email);
                return self.handle_remote_catalog_refresh();
            }
            Message::DropboxConnected(Err(err)) => {
                self.state.browser.remote.auth_in_progress = false;
                self.state.browser.remote.last_error = Some(err.clone());
                self.state.status_text = format!("Dropbox connect failed: {err}");
            }
            Message::DisconnectDropbox => {
                self.dropbox_client = None;
                // Invalidate any in-flight refresh so pages fetched for this
                // (possibly different) account cannot reconcile after a
                // reconnect.
                self.remote_catalog_generation = self.remote_catalog_generation.wrapping_add(1);
                self.remote_catalog_pending.clear();
                self.dropbox_settings.clear_tokens();
                let _ = self.dropbox_settings.save();
                self.state.browser.remote.connected = false;
                self.state.browser.remote.account_email = None;
                self.state.browser.remote.auth_in_progress = false;
                self.state.browser.remote.preview_in_progress = false;
                self.state.browser.remote.catalog_state =
                    crate::state::RemoteCatalogState::AuthenticationRequired {
                        error: "Disconnected; showing the last saved catalog".into(),
                    };
                self.state.status_text =
                    "Dropbox disconnected; saved Remote catalog remains available".to_string();
            }
            Message::RefreshRemoteConnection => {
                return self.handle_remote_catalog_refresh();
            }
            Message::RemoteCatalogPageFetched {
                generation,
                completed_pages,
                result,
            } => {
                if generation != self.remote_catalog_generation {
                    return Task::none();
                }
                match result {
                    Ok(page) => {
                        let pages = completed_pages.saturating_add(1);
                        let has_more = page.has_more;
                        let next_checkpoint = page.checkpoint.clone();
                        self.remote_catalog_pending.extend(page.changes);
                        self.state.browser.remote.refresh_pages = pages;
                        // Reconciling re-sorts the catalog and rebuilds the
                        // child index, so batch it to save intervals and the
                        // final page instead of every page.
                        let save_due = !has_more
                            || pages % super::dropbox_io::REMOTE_CATALOG_SAVE_PAGE_INTERVAL == 0;
                        if save_due {
                            self.flush_remote_catalog_pages(
                                pages,
                                (!has_more).then_some(page.checkpoint),
                            );
                        }
                        if has_more {
                            self.state.status_text = format!(
                                "Remote catalog: {} items available · fetching page {}…",
                                self.state.browser.remote.refresh_items,
                                pages.saturating_add(1)
                            );
                            if self.dropbox_client.is_none() {
                                self.state.browser.remote.catalog_state =
                                    crate::state::RemoteCatalogState::AuthenticationRequired {
                                        error:
                                            "Disconnected during refresh; showing fetched metadata"
                                                .into(),
                                    };
                                return Task::none();
                            }
                            if save_due {
                                // The next page is chained behind the save so
                                // a persistence failure still stops the
                                // refresh and only one save runs at a time.
                                return self.remote_catalog_persist_task(Some(next_checkpoint));
                            }
                            if let Some(client) = self.dropbox_client.clone() {
                                return super::dropbox_io::remote_catalog_page_task(
                                    client,
                                    Some(next_checkpoint),
                                    pages,
                                    generation,
                                );
                            }
                        } else {
                            self.state.browser.remote.catalog_state =
                                crate::state::RemoteCatalogState::Ready;
                            self.state.status_text = format!(
                                "Remote catalog refreshed: {} items across {pages} page(s)",
                                self.state.browser.remote.refresh_items
                            );
                            return self.remote_catalog_persist_task(None);
                        }
                    }
                    Err(error) => {
                        // Keep the pages that did arrive: reconcile them now
                        // and persist below so a mid-refresh failure cannot
                        // silently drop reconciled progress.
                        let flushed = self.flush_remote_catalog_pages(completed_pages, None);
                        if error.kind
                            == crate::remote_provider::RemoteProviderErrorKind::CheckpointExpired
                        {
                            // The provider invalidated our delta cursor; keep
                            // the browsable catalog but restart the refresh
                            // as a full listing from scratch.
                            self.state.browser.remote.catalog.checkpoint = None;
                            if let Some(client) = self.dropbox_client.clone() {
                                self.state.browser.remote.refresh_pages = 0;
                                self.state.status_text = "Remote checkpoint expired; rebuilding \
                                    the catalog from a full listing…"
                                    .into();
                                return Task::batch([
                                    self.remote_catalog_persist_task(None),
                                    super::dropbox_io::remote_catalog_page_task(
                                        client, None, 0, generation,
                                    ),
                                ]);
                            }
                        }
                        self.state.browser.remote.catalog_state = if error.kind
                            == crate::remote_provider::RemoteProviderErrorKind::Authentication
                        {
                            crate::state::RemoteCatalogState::AuthenticationRequired {
                                error: error.message.clone(),
                            }
                        } else if completed_pages > 0 {
                            crate::state::RemoteCatalogState::Partial {
                                pages: completed_pages,
                                error: error.message.clone(),
                            }
                        } else {
                            crate::state::RemoteCatalogState::Stale {
                                error: error.message.clone(),
                            }
                        };
                        self.state.status_text = format!(
                            "Remote catalog kept {} available items after refresh error: {}",
                            self.state.browser.remote.catalog.entries.len(),
                            error.message
                        );
                        if flushed {
                            return self.remote_catalog_persist_task(None);
                        }
                    }
                }
            }
            Message::RemoteCatalogSaved {
                generation,
                next_checkpoint,
                result,
            } => {
                if generation != self.remote_catalog_generation {
                    return Task::none();
                }
                match result {
                    Ok(()) => {
                        if let Some(checkpoint) = next_checkpoint {
                            if let Some(client) = self.dropbox_client.clone() {
                                return super::dropbox_io::remote_catalog_page_task(
                                    client,
                                    Some(checkpoint),
                                    self.state.browser.remote.refresh_pages,
                                    generation,
                                );
                            }
                            self.state.browser.remote.catalog_state =
                                crate::state::RemoteCatalogState::AuthenticationRequired {
                                    error: "Disconnected during refresh; showing fetched metadata"
                                        .into(),
                                };
                        }
                    }
                    Err(error) => {
                        if !matches!(
                            self.state.browser.remote.catalog_state,
                            crate::state::RemoteCatalogState::AuthenticationRequired { .. }
                        ) {
                            self.state.browser.remote.catalog_state =
                                crate::state::RemoteCatalogState::Stale {
                                    error: error.clone(),
                                };
                        }
                        self.state.status_text = format!("Remote catalog save failed: {error}");
                    }
                }
            }
            Message::SetMediaCacheBudgetGiB(gib) => {
                let gib = gib.clamp(1.0, 500.0);
                let bytes = (gib as f64 * 1024.0 * 1024.0 * 1024.0) as u64;
                self.state.browser.remote.cache_budget_bytes = bytes;
                self.persist_ui_settings();
                return self.media_cache_policy_task(vibez_dropbox::MediaCachePolicy {
                    budget_bytes: bytes,
                    automatic_eviction: self.state.browser.remote.cache_automatic_eviction,
                });
            }
            Message::ToggleMediaCacheAutomaticEviction => {
                let enabled = !self.state.browser.remote.cache_automatic_eviction;
                self.state.browser.remote.cache_automatic_eviction = enabled;
                self.persist_ui_settings();
                return self.media_cache_policy_task(vibez_dropbox::MediaCachePolicy {
                    budget_bytes: self.state.browser.remote.cache_budget_bytes,
                    automatic_eviction: enabled,
                });
            }
            Message::MediaCacheMaintenanceComplete(result) => match result {
                Ok(usage) => {
                    self.state.browser.remote.cache_usage_bytes = usage.bytes;
                    self.state.browser.remote.cache_entries = usage.entries;
                    self.state.browser.remote.cache_error = None;
                    self.reseed_remote_availability();
                }
                Err(error) => {
                    self.state.browser.remote.cache_error = Some(error);
                }
            },
            Message::ClearMediaCache => {
                let cache = self.dropbox_cache.clone();
                return Task::perform(
                    async move {
                        tokio::task::spawn_blocking(move || {
                            let report = cache.clear().map_err(|error| error.to_string())?;
                            let usage = cache.usage().map_err(|error| error.to_string())?;
                            Ok((report, usage))
                        })
                        .await
                        .map_err(|error| error.to_string())?
                    },
                    Message::MediaCacheCleared,
                );
            }
            Message::MediaCacheCleared(result) => match result {
                Ok((report, usage)) => {
                    self.state.browser.remote.cache_usage_bytes = usage.bytes;
                    self.state.browser.remote.cache_entries = usage.entries;
                    self.state.browser.remote.cache_error = None;
                    self.state.status_text = format!(
                        "Cleared {} Media Cache item(s); {} active item(s) protected",
                        report.removed_entries, report.protected_entries
                    );
                    self.reseed_remote_availability();
                }
                Err(error) => {
                    self.state.browser.remote.cache_error = Some(error.clone());
                    self.state.status_text = format!("Media Cache clear failed: {error}");
                }
            },
            Message::ClickRemoteBrowserEntry(entry) => {
                if self.state.browser.drag_source.is_some() {
                    self.state.browser.cancel_media_drag();
                    self.state.status_text = "Drag cancelled".into();
                    return Task::none();
                }
                let source = MediaSourceRef::DropboxFile {
                    path_lower: entry.provider_item_id.clone(),
                    display_path: entry.path.clone(),
                    rev: entry.revision.clone(),
                };
                let changed = self.state.browser.selected_source.as_ref() != Some(&source);
                self.state
                    .browser
                    .update(BrowserMsg::SelectRemoteEntry(entry.clone()));
                if changed {
                    return self.start_remote_audition(
                        DropboxEntry {
                            path_lower: entry.provider_item_id,
                            path_display: entry.path,
                            name: entry.name,
                            is_folder: false,
                            rev: entry.revision,
                            size: entry.size,
                        },
                        true,
                    );
                }
            }
            Message::RemoteAuditionReady {
                request_id,
                generation,
                source,
                result,
            } => {
                if request_id != self.remote_materialization_request_id {
                    return Task::none();
                }
                self.remote_materialization_abort = None;
                self.state.browser.remote.preview_in_progress = false;
                let path_lower = match &source {
                    MediaSourceRef::DropboxFile { path_lower, .. } => path_lower.clone(),
                    _ => return Task::none(),
                };
                match result {
                    Ok(materialized) => {
                        self.state
                            .browser
                            .remote
                            .availability
                            .insert(path_lower.clone(), crate::state::RemoteAvailability::Cached);
                        if let Some(entry) = self
                            .state
                            .browser
                            .remote
                            .catalog
                            .entries
                            .iter_mut()
                            .find(|entry| entry.provider_item_id == path_lower)
                        {
                            entry.derived_metadata = Some(materialized.metadata.clone());
                        }
                        let persist = self.remote_catalog_persist_task(None);
                        let maintenance = self.media_cache_maintenance_task();
                        self.remote_audition_cache_lease = Some(materialized.lease);
                        let follow_up =
                            if self.state.browser.selected_source.as_ref() != Some(&source) {
                                Task::none()
                            } else if self.state.browser.audition_enabled {
                                if self.state.browser.install_audition(
                                    generation,
                                    source.clone(),
                                    Arc::clone(&materialized.audio),
                                ) {
                                    self.play_browser_mode(source, materialized.audio)
                                } else {
                                    Task::none()
                                }
                            } else {
                                self.state
                                    .browser
                                    .install_waveform(source, materialized.audio);
                                self.state.status_text =
                                    format!("Cached Remote media: {}", materialized.name);
                                Task::none()
                            };
                        return Task::batch([persist, maintenance, follow_up]);
                    }
                    Err(error) => {
                        let availability = if error.contains("Reconnect Required") {
                            crate::state::RemoteAvailability::ReconnectRequired
                        } else {
                            crate::state::RemoteAvailability::Unavailable {
                                error: error.clone(),
                            }
                        };
                        self.state
                            .browser
                            .remote
                            .availability
                            .insert(path_lower, availability);
                        self.state
                            .browser
                            .fail_waveform_load(&source, error.clone());
                        self.state.browser.remote.last_error = Some(error.clone());
                        self.stop_browser_audition();
                        self.state.status_text = format!("Remote Audition unavailable: {error}");
                    }
                }
            }
            Message::DropboxPreview(entry) => {
                return self.start_remote_audition(entry, false);
            }
            Message::DropboxImportToArrangement(entry) => {
                return self.handle_dropbox_import_to_arrangement(entry);
            }
            Message::DropboxImportToDevice(entry) => {
                return self.handle_dropbox_import_to_device(entry);
            }
            _ => {}
        }
        Task::none()
    }
}
