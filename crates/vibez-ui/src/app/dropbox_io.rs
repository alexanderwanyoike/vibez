//! Split out of app.rs; inherent methods on [`super::App`].

use iced::Task;

use vibez_dropbox::{load_app_key_with_env_override, DropboxClient, DropboxEntry};

use crate::message::{BrowserImportTarget, Message};

use super::*;

pub(super) const REMOTE_SELECTION_DEBOUNCE: std::time::Duration =
    std::time::Duration::from_millis(200);
pub(super) const REMOTE_CATALOG_SAVE_PAGE_INTERVAL: usize = 10;

pub(super) fn remote_catalog_startup_task(cache: DropboxCache) -> Task<Message> {
    Task::perform(
        run_off_ui_thread("Remote catalog startup", move || {
            let store = crate::remote_provider::RemoteCatalogStore::for_dropbox();
            let (catalog, load_error) = match store.load() {
                Ok(catalog) => (catalog, None),
                Err(error) => (
                    crate::remote_provider::RemoteCatalogSnapshot::default(),
                    Some(error),
                ),
            };
            let catalog_children = crate::remote_provider::build_remote_catalog_children(&catalog);
            let availability =
                refreshed_remote_availability(&cache, &catalog, std::collections::HashMap::new());
            let cache_usage = cache.usage().unwrap_or_default();
            crate::message::RemoteCatalogStartupData {
                catalog,
                catalog_children,
                availability,
                cache_usage,
                load_error,
            }
        }),
        |result| {
            Message::RemoteCatalogStartupLoaded(crate::message::RemoteCatalogStartupResult::new(
                result,
            ))
        },
    )
}

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
    let refreshed = refreshed_remote_availability(
        cache,
        &remote.catalog,
        std::mem::take(&mut remote.availability),
    );
    remote.replace_availability(refreshed);
}

fn refreshed_remote_availability(
    cache: &DropboxCache,
    catalog: &crate::remote_provider::RemoteCatalogSnapshot,
    mut availability: std::collections::HashMap<String, crate::state::RemoteAvailability>,
) -> std::collections::HashMap<String, crate::state::RemoteAvailability> {
    use crate::state::RemoteAvailability;
    let cached: std::collections::HashMap<String, Option<String>> =
        cache.cached_identities().into_iter().collect();
    for entry in &catalog.entries {
        if entry.is_folder {
            continue;
        }
        let is_cached = cached
            .get(&entry.provider_item_id)
            .is_some_and(|revision| revision.as_deref() == entry.revision.as_deref());
        match availability.get(&entry.provider_item_id) {
            Some(RemoteAvailability::Fetching) => {}
            Some(RemoteAvailability::Cached) if !is_cached => {
                availability.remove(&entry.provider_item_id);
            }
            _ if is_cached => {
                availability.insert(entry.provider_item_id.clone(), RemoteAvailability::Cached);
            }
            _ => {}
        }
    }
    availability
}

fn catalog_with_refresh_checkpoint(
    previous_catalog: Arc<crate::remote_provider::RemoteCatalogSnapshot>,
    checkpoint: Option<String>,
) -> Arc<crate::remote_provider::RemoteCatalogSnapshot> {
    let Some(checkpoint) = checkpoint else {
        return previous_catalog;
    };
    if previous_catalog.checkpoint.as_deref() == Some(&checkpoint) {
        return previous_catalog;
    }
    let mut catalog = (*previous_catalog).clone();
    catalog.checkpoint = Some(checkpoint);
    Arc::new(catalog)
}

fn rebase_remote_catalog_refresh(
    mut data: crate::message::RemoteCatalogRefreshData,
    live_catalog: Arc<crate::remote_provider::RemoteCatalogSnapshot>,
    live_availability: std::collections::HashMap<String, crate::state::RemoteAvailability>,
    live_runtime_revision: u64,
) -> crate::message::RemoteCatalogRefreshData {
    let base_by_id: std::collections::HashMap<_, _> = data
        .base_catalog
        .entries
        .iter()
        .map(|entry| (entry.provider_item_id.as_str(), entry))
        .collect();
    let live_by_id: std::collections::HashMap<_, _> = live_catalog
        .entries
        .iter()
        .map(|entry| (entry.provider_item_id.as_str(), entry))
        .collect();
    for entry in &mut Arc::make_mut(&mut data.catalog).entries {
        let Some(live_entry) = live_by_id.get(entry.provider_item_id.as_str()) else {
            continue;
        };
        let base_metadata = base_by_id
            .get(entry.provider_item_id.as_str())
            .and_then(|base_entry| base_entry.derived_metadata.as_ref());
        if live_entry.revision == entry.revision
            && live_entry.derived_metadata.as_ref() != base_metadata
        {
            entry.derived_metadata = live_entry.derived_metadata.clone();
        }
    }

    if let Some(availability) = data.availability.as_mut() {
        for provider_item_id in data.base_availability.keys() {
            if !live_availability.contains_key(provider_item_id) {
                availability.remove(provider_item_id);
            }
        }
        for (provider_item_id, live_state) in &live_availability {
            if data.base_availability.get(provider_item_id) != Some(live_state) {
                availability.insert(provider_item_id.clone(), live_state.clone());
            }
        }
    }

    data.base_catalog = live_catalog;
    data.base_availability = live_availability;
    data.base_runtime_revision = live_runtime_revision;
    data
}

impl App {
    pub(super) fn prepare_remote_catalog_refresh(
        &mut self,
        pages: usize,
        checkpoint: Option<String>,
        continuation: crate::message::RemoteCatalogRefreshContinuation,
    ) -> Task<Message> {
        let generation = self.remote_catalog_request.current().unwrap_or(0);
        let previous_catalog = Arc::clone(&self.state.browser.remote.catalog);
        let previous_availability = self.state.browser.remote.availability.clone();
        let base_runtime_revision = self.state.browser.remote.catalog_runtime_revision;
        let changes = std::mem::take(&mut self.remote_catalog_pending);
        let cache = self.dropbox_cache.clone();
        Task::perform(
            run_off_ui_thread("Remote catalog refresh", move || {
                if changes.is_empty() {
                    let catalog =
                        catalog_with_refresh_checkpoint(Arc::clone(&previous_catalog), checkpoint);
                    return crate::message::RemoteCatalogRefreshData {
                        generation,
                        pages,
                        catalog,
                        catalog_children: None,
                        availability: None,
                        base_catalog: previous_catalog,
                        base_availability: previous_availability,
                        base_runtime_revision,
                        continuation,
                    };
                }

                let base_catalog = Arc::clone(&previous_catalog);
                let base_availability = previous_availability.clone();
                let mut catalog = (*previous_catalog).clone();
                crate::remote_provider::reconcile_remote_catalog(
                    &mut catalog,
                    &crate::remote_provider::RemoteRefreshResult {
                        pages: pages.max(1),
                        changes,
                        checkpoint,
                        error: None,
                    },
                );
                let catalog_children =
                    crate::remote_provider::build_remote_catalog_children(&catalog);
                let availability =
                    refreshed_remote_availability(&cache, &catalog, previous_availability);
                crate::message::RemoteCatalogRefreshData {
                    generation,
                    pages,
                    catalog: Arc::new(catalog),
                    catalog_children: Some(catalog_children),
                    availability: Some(availability),
                    base_catalog,
                    base_availability,
                    base_runtime_revision,
                    continuation,
                }
            }),
            move |result| {
                Message::RemoteCatalogRefreshPrepared(
                    crate::message::RemoteCatalogRefreshResult::new(generation, result),
                )
            },
        )
    }

    pub(super) fn rebase_remote_catalog_refresh(
        &self,
        data: crate::message::RemoteCatalogRefreshData,
    ) -> Task<Message> {
        let generation = data.generation;
        let live_catalog = Arc::clone(&self.state.browser.remote.catalog);
        let live_availability = self.state.browser.remote.availability.clone();
        let live_runtime_revision = self.state.browser.remote.catalog_runtime_revision;
        Task::perform(
            run_off_ui_thread("Remote catalog refresh", move || {
                rebase_remote_catalog_refresh(
                    data,
                    live_catalog,
                    live_availability,
                    live_runtime_revision,
                )
            }),
            move |result| {
                Message::RemoteCatalogRefreshPrepared(
                    crate::message::RemoteCatalogRefreshResult::new(generation, result),
                )
            },
        )
    }

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
        self.state.browser.remote.set_availability(
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
        if matches!(
            self.state.browser.remote.catalog_state,
            crate::state::RemoteCatalogState::Loading
                | crate::state::RemoteCatalogState::Refreshing
        ) {
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
            self.state.browser.remote.set_availability(
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
        self.state.browser.remote.set_availability(
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
        let treatment = self.state.browser.audition_import_input();
        self.start_remote_import(entry, target, treatment)
    }

    pub(super) fn handle_dropbox_import_to_device(&mut self, entry: DropboxEntry) -> Task<Message> {
        let Some(target) = self.selected_browser_device_target() else {
            self.state.status_text = "Select a Sampler or Drum Pad track first".into();
            return Task::none();
        };
        let treatment = self.state.browser.audition_import_input();
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

    #[test]
    fn intermediate_refresh_without_a_checkpoint_preserves_the_saved_cursor() {
        let previous = Arc::new(crate::remote_provider::RemoteCatalogSnapshot {
            checkpoint: Some("saved-cursor".into()),
            ..Default::default()
        });

        let refreshed = catalog_with_refresh_checkpoint(Arc::clone(&previous), None);

        assert!(Arc::ptr_eq(&previous, &refreshed));
        assert_eq!(refreshed.checkpoint.as_deref(), Some("saved-cursor"));
    }

    #[test]
    fn prepared_refresh_rebases_materialized_metadata_and_availability() {
        let base_catalog = Arc::new(crate::remote_provider::RemoteCatalogSnapshot {
            entries: vec![crate::remote_provider::RemoteCatalogEntry {
                provider_item_id: "/kick.wav".into(),
                path: "/kick.wav".into(),
                parent_path: String::new(),
                name: "kick.wav".into(),
                is_folder: false,
                revision: Some("rev-1".into()),
                size: Some(128),
                derived_metadata: None,
            }],
            checkpoint: Some("old".into()),
            ..Default::default()
        });
        let mut live_catalog = (*base_catalog).clone();
        live_catalog.entries[0].derived_metadata = Some(vibez_dropbox::DerivedMetadata {
            duration_seconds: 0.5,
            channels: 1,
            sample_rate: 44_100,
            ..Default::default()
        });
        let live_catalog = Arc::new(live_catalog);
        let base_availability = std::collections::HashMap::from([(
            "/kick.wav".into(),
            crate::state::RemoteAvailability::Fetching,
        )]);
        let live_availability = std::collections::HashMap::from([(
            "/kick.wav".into(),
            crate::state::RemoteAvailability::Cached,
        )]);
        let mut prepared_catalog = (*base_catalog).clone();
        prepared_catalog.checkpoint = Some("new".into());
        let data = crate::message::RemoteCatalogRefreshData {
            generation: 4,
            pages: 1,
            catalog: Arc::new(prepared_catalog),
            catalog_children: Some(Default::default()),
            availability: Some(base_availability.clone()),
            base_catalog,
            base_availability,
            base_runtime_revision: 7,
            continuation: crate::message::RemoteCatalogRefreshContinuation::Complete,
        };

        let rebased =
            rebase_remote_catalog_refresh(data, Arc::clone(&live_catalog), live_availability, 8);

        assert_eq!(
            rebased.catalog.entries[0]
                .derived_metadata
                .as_ref()
                .unwrap()
                .sample_rate,
            44_100
        );
        assert_eq!(
            rebased.availability.as_ref().unwrap().get("/kick.wav"),
            Some(&crate::state::RemoteAvailability::Cached)
        );
        assert!(Arc::ptr_eq(&rebased.base_catalog, &live_catalog));
        assert_eq!(rebased.base_runtime_revision, 8);
    }

    #[test]
    fn refresh_errors_retain_their_generation_for_stale_result_rejection() {
        let result =
            crate::message::RemoteCatalogRefreshResult::new(23, Err("worker stopped".into()));

        assert_eq!(result.generation(), 23);
        assert!(matches!(result.take(), Some(Err(error)) if error == "worker stopped"));
    }
}
