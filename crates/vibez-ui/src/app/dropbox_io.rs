//! Split out of app.rs; inherent methods on [`super::App`].

use iced::Task;

use vibez_dropbox::{load_app_key_with_env_override, DropboxClient, DropboxEntry};

use crate::message::{BrowserImportTarget, Message};

use super::*;

pub(super) const REMOTE_SELECTION_DEBOUNCE: std::time::Duration =
    std::time::Duration::from_millis(200);
pub(super) const REMOTE_CATALOG_SAVE_PAGE_INTERVAL: usize = 10;

fn queue_latest_remote_audition(slot: &mut Option<DropboxEntry>, entry: DropboxEntry) {
    *slot = Some(entry);
}

pub(super) fn remote_catalog_page_task(
    client: Arc<DropboxClient>,
    checkpoint: Option<String>,
    completed_pages: usize,
    generation: u64,
) -> Task<Message> {
    Task::perform(
        async move {
            let provider = crate::remote_provider::DropboxRemoteProvider::new((*client).clone());
            crate::remote_provider::fetch_remote_catalog_page(&provider, checkpoint.as_deref())
                .await
        },
        move |result| Message::RemoteCatalogPageFetched {
            generation,
            completed_pages,
            result,
        },
    )
}

/// Mark catalog entries whose bytes are already materialized as Cached, so
/// rendering never has to stat the disk per row. Availability states that
/// track an in-flight fetch or a hard error are preserved.
pub(super) fn seed_remote_availability(
    cache: &DropboxCache,
    remote: &mut crate::state::RemoteUiState,
) {
    use crate::state::RemoteAvailability;
    let cached: std::collections::HashMap<String, Option<String>> =
        cache.cached_identities().into_iter().collect();
    for entry in &remote.catalog.entries {
        if entry.is_folder {
            continue;
        }
        let is_cached = cached
            .get(&entry.provider_item_id)
            .is_some_and(|revision| revision.as_deref() == entry.revision.as_deref());
        match remote.availability.get(&entry.provider_item_id) {
            Some(RemoteAvailability::Fetching) => {}
            Some(RemoteAvailability::Cached) if !is_cached => {
                remote.availability.remove(&entry.provider_item_id);
            }
            _ if is_cached => {
                remote
                    .availability
                    .insert(entry.provider_item_id.clone(), RemoteAvailability::Cached);
            }
            _ => {}
        }
    }
}

impl App {
    pub(super) fn remote_import_active(&self) -> bool {
        self.remote_import_request.is_active()
    }

    pub(super) fn reseed_remote_availability(&mut self) {
        seed_remote_availability(&self.dropbox_cache, &mut self.state.browser.remote);
    }

    /// Apply a Media Cache policy (and enforce its budget) off the update
    /// thread; the result lands as [`Message::MediaCacheMaintenanceComplete`].
    pub(super) fn media_cache_policy_task(
        &self,
        policy: vibez_dropbox::MediaCachePolicy,
    ) -> Task<Message> {
        let cache = self.dropbox_cache.clone();
        Task::perform(
            async move {
                tokio::task::spawn_blocking(move || {
                    cache.set_policy(policy).map_err(|error| error.to_string())
                })
                .await
                .map_err(|error| error.to_string())?
            },
            Message::MediaCacheMaintenanceComplete,
        )
    }

    /// Re-enforce the current budget (e.g. after releasing a lease).
    pub(super) fn media_cache_maintenance_task(&self) -> Task<Message> {
        self.media_cache_policy_task(self.dropbox_cache.policy())
    }

    /// Persist the current Remote catalog snapshot off the update thread.
    /// Passing `next_checkpoint` chains the following page fetch behind a
    /// successful save, so at most one save is ever in flight per refresh.
    pub(super) fn remote_catalog_persist_task(
        &self,
        next_checkpoint: Option<String>,
    ) -> Task<Message> {
        let generation = self.remote_catalog_request.current().unwrap_or(0);
        let snapshot = self.state.browser.remote.catalog.clone();
        Task::perform(
            async move {
                tokio::task::spawn_blocking(move || {
                    crate::remote_provider::RemoteCatalogStore::for_dropbox().save(&snapshot)
                })
                .await
                .map_err(|error| format!("remote catalog save task failed: {error}"))?
            },
            move |result| Message::RemoteCatalogSaved {
                generation,
                next_checkpoint: next_checkpoint.clone(),
                result,
            },
        )
    }

    /// Reconcile the pages accumulated since the last flush into the catalog
    /// and rebuild the derived lookups. Returns whether anything changed.
    pub(super) fn flush_remote_catalog_pages(
        &mut self,
        pages: usize,
        checkpoint: Option<String>,
    ) -> bool {
        if self.remote_catalog_pending.is_empty() && checkpoint.is_none() {
            return false;
        }
        let changes = std::mem::take(&mut self.remote_catalog_pending);
        crate::remote_provider::reconcile_remote_catalog(
            &mut self.state.browser.remote.catalog,
            &crate::remote_provider::RemoteRefreshResult {
                pages: pages.max(1),
                changes,
                checkpoint,
                error: None,
            },
        );
        self.state.browser.remote.rebuild_catalog_children();
        self.state.browser.remote.refresh_items = self.state.browser.remote.catalog.entries.len();
        self.reseed_remote_availability();
        true
    }

    pub(super) fn start_remote_import(
        &mut self,
        entry: DropboxEntry,
        target: BrowserImportTarget,
        treatment: crate::state::AuditionImportInput,
    ) -> Task<Message> {
        self.remote_materialization_request.cancel();
        self.remote_audition_cache_lease = None;
        let maintenance = self.media_cache_maintenance_task();
        let request_id = self.remote_import_request.begin();
        self.state.browser.remote.availability.insert(
            entry.path_lower.clone(),
            if self
                .dropbox_cache
                .is_cached(&entry.path_lower, entry.rev.as_deref())
            {
                crate::state::RemoteAvailability::Cached
            } else {
                crate::state::RemoteAvailability::Fetching
            },
        );
        self.state.status_text = format!("Importing Remote media: {}", entry.name);
        let client = self.dropbox_client.clone();
        let cache = self.dropbox_cache.clone();
        let task = Task::perform(
            fetch_dropbox_sample_async(client, cache, entry),
            move |result| Message::RemoteImportReady {
                request_id,
                target: target.clone(),
                treatment,
                result,
            },
        );
        let task = self.remote_import_request.attach(task);
        Task::batch([task, maintenance])
    }

    pub(super) fn handle_connect_dropbox(&mut self) -> Task<Message> {
        let Some(app_key) = load_app_key_with_env_override(&self.dropbox_settings) else {
            self.state.browser.remote.last_error = Some(
                "No Dropbox app key set. Register an app at dropbox.com/developers/apps \
                    and paste the App key above."
                    .into(),
            );
            return Task::none();
        };
        if self.state.browser.remote.auth_in_progress {
            return Task::none();
        }
        self.state.browser.remote.auth_in_progress = true;
        self.state.browser.remote.last_error = None;
        self.state.status_text = "Opening Dropbox authorisation...".to_string();
        Task::perform(connect_dropbox_async(app_key), |result| {
            Message::DropboxConnected(
                result.map(|(info, tokens)| crate::message::DropboxConnectOutcome { info, tokens }),
            )
        })
    }

    pub(super) fn handle_remote_catalog_refresh(&mut self) -> Task<Message> {
        if self.state.browser.remote.catalog_state == crate::state::RemoteCatalogState::Refreshing {
            return Task::none();
        }
        let Some(client) = self.dropbox_client.clone() else {
            self.state.browser.remote.catalog_state =
                crate::state::RemoteCatalogState::AuthenticationRequired {
                    error: "Connect Dropbox in Settings to refresh".into(),
                };
            return Task::none();
        };
        let generation = self.remote_catalog_request.begin();
        self.remote_catalog_pending.clear();
        self.state.browser.remote.catalog_state = crate::state::RemoteCatalogState::Refreshing;
        self.state.browser.remote.refresh_pages = 0;
        self.state.browser.remote.refresh_items = self.state.browser.remote.catalog.entries.len();
        self.state.status_text = "Refreshing Remote catalog…".into();
        let checkpoint = self.state.browser.remote.catalog.checkpoint.clone();
        remote_catalog_page_task(client, checkpoint, 0, generation)
    }

    pub(super) fn start_remote_audition(
        &mut self,
        entry: DropboxEntry,
        debounce: bool,
    ) -> Task<Message> {
        let source = vibez_core::track::MediaSourceRef::DropboxFile {
            path_lower: entry.path_lower.clone(),
            display_path: entry.path_display.clone(),
            rev: entry.rev.clone(),
        };
        self.state.browser.select_source(source.clone());
        if self.remote_import_active() {
            queue_latest_remote_audition(&mut self.pending_remote_audition, entry);
            self.state.status_text = "Remote Audition queued behind active import".into();
            return Task::none();
        }
        self.remote_materialization_request.cancel();
        self.remote_audition_cache_lease = None;
        let maintenance = self.media_cache_maintenance_task();
        let request_id = self.remote_materialization_request.begin();
        let cached = self
            .dropbox_cache
            .is_cached(&entry.path_lower, entry.rev.as_deref());
        if !cached && self.dropbox_client.is_none() {
            self.state.browser.remote.preview_in_progress = false;
            self.state.browser.remote.availability.insert(
                entry.path_lower,
                crate::state::RemoteAvailability::ReconnectRequired,
            );
            self.state.status_text =
                "Reconnect Required · this Remote item is not in Media Cache".into();
            self.state
                .browser
                .fail_waveform_load(&source, "Reconnect Required · uncached Remote media".into());
            self.remote_materialization_request.finish(request_id);
            return Task::none();
        }

        let generation = if self.state.browser.audition_enabled {
            self.state.browser.begin_audition_load(&source)
        } else {
            self.state.browser.begin_waveform_load(&source);
            self.state.browser.audition_generation
        };
        self.state.browser.remote.preview_in_progress = !cached;
        self.state.browser.remote.availability.insert(
            entry.path_lower.clone(),
            if cached {
                crate::state::RemoteAvailability::Cached
            } else {
                crate::state::RemoteAvailability::Fetching
            },
        );
        self.state.status_text = if cached {
            format!("Preparing cached Audition: {}", entry.name)
        } else {
            format!("Fetching Remote media: {}", entry.name)
        };
        let lease = self
            .dropbox_cache
            .protect(&entry.path_lower, entry.rev.as_deref());
        let task = Task::perform(
            materialize_remote_sample_async(
                self.dropbox_client.clone(),
                self.dropbox_cache.clone(),
                entry,
                lease,
                debounce,
            ),
            move |result| Message::RemoteAuditionReady {
                request_id,
                generation,
                source: source.clone(),
                result,
            },
        );
        let task = self.remote_materialization_request.attach(task);
        Task::batch([task, maintenance])
    }

    pub(super) fn handle_dropbox_import_to_arrangement(
        &mut self,
        entry: DropboxEntry,
    ) -> Task<Message> {
        let target = BrowserImportTarget::ArrangementClip(self.state.arrangement.selected_track);
        let Some(treatment) = self.state.browser.audition_import_input() else {
            self.state.status_text = "Confirm source BPM before WARP import".into();
            return Task::none();
        };
        self.start_remote_import(entry, target, treatment)
    }

    pub(super) fn handle_dropbox_import_to_device(&mut self, entry: DropboxEntry) -> Task<Message> {
        let Some(target) = self.selected_browser_device_target() else {
            self.state.status_text = "Select a Sampler or Drum Pad track first".into();
            return Task::none();
        };
        let Some(treatment) = self.state.browser.audition_import_input() else {
            self.state.status_text = "Confirm source BPM before WARP import".into();
            return Task::none();
        };
        self.start_remote_import(entry, target, treatment)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(path: &str) -> DropboxEntry {
        DropboxEntry {
            path_lower: path.into(),
            path_display: path.into(),
            name: path.rsplit('/').next().unwrap().into(),
            is_folder: false,
            rev: Some("1".into()),
            size: None,
        }
    }

    #[test]
    fn remote_selection_debounce_is_exactly_two_hundred_milliseconds() {
        assert_eq!(
            REMOTE_SELECTION_DEBOUNCE,
            std::time::Duration::from_millis(200)
        );
    }

    #[test]
    fn import_priority_defers_audition_and_retains_only_latest_selection() {
        let mut pending = None;
        queue_latest_remote_audition(&mut pending, entry("/one.wav"));
        queue_latest_remote_audition(&mut pending, entry("/two.wav"));
        queue_latest_remote_audition(&mut pending, entry("/winner.wav"));
        assert_eq!(
            pending.as_ref().map(|entry| entry.path_lower.as_str()),
            Some("/winner.wav")
        );
    }
}
