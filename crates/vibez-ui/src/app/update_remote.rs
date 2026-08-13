//! Dropbox, remote-catalog, media-cache, and remote-browser message handlers.
//! Split from update.rs; each method backs one arm of the App::update match.

use std::sync::Arc;

use iced::Task;

use crate::domains::browser::BrowserMsg;
use vibez_core::track::MediaSourceRef;
use vibez_dropbox::{load_app_key_with_env_override, DropboxClient};

use crate::message::Message;

use super::*;

impl App {
    pub(super) fn on_remote_catalog_startup_loaded(
        &mut self,
        result: crate::message::RemoteCatalogStartupResult,
    ) -> Task<Message> {
        let Some(result) = result.take() else {
            return Task::none();
        };
        match result {
            Ok(data) => {
                let item_count = data.catalog.entries.len();
                self.state.browser.remote.catalog = Arc::new(data.catalog);
                self.state.browser.remote.catalog_children = data.catalog_children;
                self.state.browser.remote.availability.clear();
                self.state.browser.remote.availability.extend(
                    data.cached_provider_item_ids
                        .into_iter()
                        .map(|provider_item_id| {
                            (provider_item_id, crate::state::RemoteAvailability::Cached)
                        }),
                );
                self.state.browser.remote.mark_catalog_runtime_changed();
                self.state.browser.remote.cache_usage_bytes = data.cache_usage.bytes;
                self.state.browser.remote.cache_entries = data.cache_usage.entries;
                self.state.browser.remote.refresh_items = item_count;
                if let Some(error) = data.load_error {
                    self.state.browser.remote.catalog_state =
                        crate::state::RemoteCatalogState::Stale {
                            error: error.clone(),
                        };
                    self.state.status_text = format!("Remote catalog load failed: {error}");
                } else if self.dropbox_client.is_none() {
                    self.state.browser.remote.catalog_state =
                        crate::state::RemoteCatalogState::AuthenticationRequired {
                            error: "Sign in to refresh; showing the last saved Remote catalog"
                                .into(),
                        };
                    self.state.status_text =
                        format!("Loaded {item_count} saved Remote catalog items");
                } else {
                    self.state.browser.remote.catalog_state =
                        crate::state::RemoteCatalogState::Ready;
                    self.state.status_text =
                        format!("Loaded {item_count} saved Remote catalog items");
                }
            }
            Err(error) => {
                self.state.browser.remote.catalog_state = crate::state::RemoteCatalogState::Stale {
                    error: error.clone(),
                };
                self.state.status_text = error;
            }
        }

        if self.dropbox_client.is_some() {
            self.handle_remote_catalog_refresh()
        } else {
            Task::none()
        }
    }

    pub(super) fn on_save_dropbox_app_key(&mut self) -> Task<Message> {
        let value = self.state.browser.remote.app_key_input.trim().to_string();
        self.dropbox_settings.app_key = if value.is_empty() { None } else { Some(value) };
        if let Err(err) = self.dropbox_settings.save() {
            self.state.browser.remote.last_error = Some(format!("save settings: {err}"));
        }
        self.state.browser.remote.has_app_key =
            load_app_key_with_env_override(&self.dropbox_settings).is_some();
        self.state.status_text = "Dropbox app key saved".to_string();
        Task::none()
    }

    pub(super) fn on_dropbox_connected(
        &mut self,
        outcome: crate::message::DropboxConnectOutcome,
    ) -> Task<Message> {
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
        self.handle_remote_catalog_refresh()
    }

    pub(super) fn on_disconnect_dropbox(&mut self) -> Task<Message> {
        self.dropbox_client = None;
        // Invalidate any in-flight refresh so pages fetched for this
        // (possibly different) account cannot reconcile after a
        // reconnect.
        self.remote_catalog_request.cancel();
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
        Task::none()
    }

    pub(super) fn on_remote_catalog_page_fetched(
        &mut self,
        generation: u64,
        completed_pages: usize,
        result: Result<
            crate::remote_provider::RemotePage,
            crate::remote_provider::RemoteProviderError,
        >,
    ) -> Task<Message> {
        if !self.remote_catalog_request.is_current(generation) {
            return Task::none();
        }
        match result {
            Ok(page) => {
                let pages = completed_pages.saturating_add(1);
                let has_more = page.has_more;
                let next_checkpoint = page.checkpoint.clone();
                self.remote_catalog_pending.extend(page.changes);
                self.state.browser.remote.refresh_pages = pages;
                // Reconciliation is prepared off the UI thread at save
                // intervals and at the final page.
                let save_due = !has_more
                    || pages.is_multiple_of(super::dropbox_io::REMOTE_CATALOG_SAVE_PAGE_INTERVAL);
                if has_more {
                    self.state.status_text = format!(
                        "Remote catalog: {} items available · fetching page {}…",
                        self.state.browser.remote.refresh_items,
                        pages.saturating_add(1)
                    );
                    if self.dropbox_client.is_none() {
                        self.state.browser.remote.catalog_state =
                            crate::state::RemoteCatalogState::AuthenticationRequired {
                                error: "Disconnected during refresh; showing fetched metadata"
                                    .into(),
                            };
                        return Task::none();
                    }
                    if save_due {
                        return self.prepare_remote_catalog_refresh(
                            pages,
                            None,
                            crate::message::RemoteCatalogRefreshContinuation::FetchNext {
                                checkpoint: next_checkpoint,
                            },
                        );
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
                    return self.prepare_remote_catalog_refresh(
                        pages,
                        Some(page.checkpoint),
                        crate::message::RemoteCatalogRefreshContinuation::Complete,
                    );
                }
            }
            Err(error) => {
                if !self.remote_catalog_pending.is_empty() {
                    return self.prepare_remote_catalog_refresh(
                        completed_pages,
                        None,
                        crate::message::RemoteCatalogRefreshContinuation::Failed(error),
                    );
                }
                return self.finish_remote_catalog_refresh_error(
                    generation,
                    completed_pages,
                    error,
                    false,
                );
            }
        }
        Task::none()
    }

    pub(super) fn on_remote_catalog_refresh_prepared(
        &mut self,
        result: crate::message::RemoteCatalogRefreshResult,
    ) -> Task<Message> {
        if !self.remote_catalog_request.is_current(result.generation()) {
            std::thread::spawn(move || drop(result));
            return Task::none();
        }
        let Some(result) = result.take() else {
            return Task::none();
        };
        let data = match result {
            Ok(data) => data,
            Err(error) => {
                self.state.browser.remote.catalog_state = crate::state::RemoteCatalogState::Stale {
                    error: error.clone(),
                };
                self.state.status_text = error;
                return Task::none();
            }
        };
        if data.base_runtime_revision != self.state.browser.remote.catalog_runtime_revision {
            return self.rebase_remote_catalog_refresh(data);
        }

        let retired_catalog =
            std::mem::replace(&mut self.state.browser.remote.catalog, data.catalog);
        let retired_children = data.catalog_children.map(|children| {
            std::mem::replace(&mut self.state.browser.remote.catalog_children, children)
        });
        let retired_availability = data.availability.map(|availability| {
            std::mem::replace(&mut self.state.browser.remote.availability, availability)
        });
        // Large snapshots and their derived maps are destructed on a worker,
        // too; replacing them must remain constant-time on the UI thread.
        std::thread::spawn(move || {
            drop((retired_catalog, retired_children, retired_availability));
        });

        self.state.browser.remote.refresh_items = self.state.browser.remote.catalog.entries.len();
        match data.continuation {
            crate::message::RemoteCatalogRefreshContinuation::FetchNext { checkpoint } => {
                self.state.status_text = format!(
                    "Remote catalog: {} items available · saving page {}…",
                    self.state.browser.remote.refresh_items, data.pages
                );
                self.remote_catalog_persist_task(Some(checkpoint))
            }
            crate::message::RemoteCatalogRefreshContinuation::Complete => {
                self.state.browser.remote.catalog_state = crate::state::RemoteCatalogState::Ready;
                self.state.status_text = format!(
                    "Remote catalog refreshed: {} items across {} page(s)",
                    self.state.browser.remote.refresh_items, data.pages
                );
                self.remote_catalog_persist_task(None)
            }
            crate::message::RemoteCatalogRefreshContinuation::Failed(error) => {
                self.finish_remote_catalog_refresh_error(data.generation, data.pages, error, true)
            }
        }
    }

    fn finish_remote_catalog_refresh_error(
        &mut self,
        generation: u64,
        completed_pages: usize,
        error: crate::remote_provider::RemoteProviderError,
        reconciled_pages: bool,
    ) -> Task<Message> {
        if error.kind == crate::remote_provider::RemoteProviderErrorKind::CheckpointExpired {
            // The provider invalidated our delta cursor; keep the browsable
            // catalog but restart the refresh as a full listing from scratch.
            Arc::make_mut(&mut self.state.browser.remote.catalog).checkpoint = None;
            if let Some(client) = self.dropbox_client.clone() {
                self.state.browser.remote.refresh_pages = 0;
                self.state.status_text =
                    "Remote checkpoint expired; rebuilding the catalog from a full listing…".into();
                return Task::batch([
                    self.remote_catalog_persist_task(None),
                    super::dropbox_io::remote_catalog_page_task(client, None, 0, generation),
                ]);
            }
        }
        self.state.browser.remote.catalog_state =
            if error.kind == crate::remote_provider::RemoteProviderErrorKind::Authentication {
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
        if reconciled_pages {
            self.remote_catalog_persist_task(None)
        } else {
            Task::none()
        }
    }

    pub(super) fn on_remote_catalog_saved(
        &mut self,
        generation: u64,
        next_checkpoint: Option<String>,
        result: Result<(), String>,
    ) -> Task<Message> {
        if !self.remote_catalog_request.is_current(generation) {
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
                            error: "Disconnected during refresh; showing fetched metadata".into(),
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
        Task::none()
    }

    pub(super) fn on_set_media_cache_budget(&mut self, gib: f32) -> Task<Message> {
        let gib = gib.clamp(1.0, 500.0);
        let bytes = (gib as f64 * 1024.0 * 1024.0 * 1024.0) as u64;
        self.state.browser.remote.cache_budget_bytes = bytes;
        self.persist_ui_settings();
        self.media_cache_policy_task(vibez_dropbox::MediaCachePolicy {
            budget_bytes: bytes,
            automatic_eviction: self.state.browser.remote.cache_automatic_eviction,
        })
    }

    pub(super) fn on_toggle_media_cache_automatic_eviction(&mut self) -> Task<Message> {
        let enabled = !self.state.browser.remote.cache_automatic_eviction;
        self.state.browser.remote.cache_automatic_eviction = enabled;
        self.persist_ui_settings();
        self.media_cache_policy_task(vibez_dropbox::MediaCachePolicy {
            budget_bytes: self.state.browser.remote.cache_budget_bytes,
            automatic_eviction: enabled,
        })
    }

    pub(super) fn on_media_cache_maintenance_complete(
        &mut self,
        result: Result<vibez_dropbox::CacheUsage, String>,
    ) -> Task<Message> {
        match result {
            Ok(usage) => {
                self.state.browser.remote.cache_usage_bytes = usage.bytes;
                self.state.browser.remote.cache_entries = usage.entries;
                self.state.browser.remote.cache_error = None;
                self.reseed_remote_availability();
            }
            Err(error) => {
                self.state.browser.remote.cache_error = Some(error);
            }
        }
        Task::none()
    }

    pub(super) fn on_clear_media_cache(&mut self) -> Task<Message> {
        let cache = self.dropbox_cache.clone();
        Task::perform(
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
        )
    }

    pub(super) fn on_media_cache_cleared(
        &mut self,
        result: Result<(vibez_dropbox::CacheClearReport, vibez_dropbox::CacheUsage), String>,
    ) -> Task<Message> {
        match result {
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
        }
        Task::none()
    }

    pub(super) fn on_click_remote_browser_entry(
        &mut self,
        entry: crate::remote_provider::RemoteCatalogEntry,
    ) -> Task<Message> {
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
        Task::none()
    }

    pub(super) fn on_remote_audition_ready(
        &mut self,
        request_id: u64,
        generation: u64,
        source: MediaSourceRef,
        result: Result<crate::message::RemoteMaterializedSample, String>,
    ) -> Task<Message> {
        if !self.remote_materialization_request.finish(request_id) {
            return Task::none();
        }
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
                if let Some(entry) = Arc::make_mut(&mut self.state.browser.remote.catalog)
                    .entries
                    .iter_mut()
                    .find(|entry| entry.provider_item_id == path_lower)
                {
                    entry.derived_metadata = Some(materialized.metadata.clone());
                }
                self.state.browser.remote.mark_catalog_runtime_changed();
                let persist = self.remote_catalog_persist_task(None);
                let maintenance = self.media_cache_maintenance_task();
                self.remote_audition_cache_lease = Some(materialized.lease);
                let follow_up = if self.state.browser.selected_source.as_ref() != Some(&source) {
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
                    self.state.status_text = format!("Cached Remote media: {}", materialized.name);
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
                self.state.browser.remote.mark_catalog_runtime_changed();
                self.state
                    .browser
                    .fail_waveform_load(&source, error.clone());
                self.state.browser.remote.last_error = Some(error.clone());
                self.stop_browser_audition();
                self.state.status_text = format!("Remote Audition unavailable: {error}");
            }
        }
        Task::none()
    }
}
