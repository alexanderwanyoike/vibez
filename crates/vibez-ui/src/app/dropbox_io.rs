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

pub(super) fn remote_import_result_is_current(current: u64, result: u64) -> bool {
    current == result
}

pub(super) fn remote_catalog_page_task(
    client: Arc<DropboxClient>,
    checkpoint: Option<String>,
    completed_pages: usize,
) -> Task<Message> {
    Task::perform(
        async move {
            let provider = crate::remote_provider::DropboxRemoteProvider::new((*client).clone());
            crate::remote_provider::fetch_remote_catalog_page(&provider, checkpoint.as_deref())
                .await
        },
        move |result| Message::RemoteCatalogPageFetched {
            completed_pages,
            result,
        },
    )
}

impl App {
    pub(super) fn start_remote_import(
        &mut self,
        entry: DropboxEntry,
        target: BrowserImportTarget,
        treatment: crate::state::AuditionImportInput,
    ) -> Task<Message> {
        if let Some(handle) = self.remote_import_abort.take() {
            handle.abort();
        }
        if let Some(handle) = self.remote_materialization_abort.take() {
            handle.abort();
            self.remote_materialization_request_id =
                self.remote_materialization_request_id.saturating_add(1);
        }
        self.remote_audition_cache_lease = None;
        let _ = self.dropbox_cache.set_policy(self.dropbox_cache.policy());
        self.remote_import_request_id = self.remote_import_request_id.saturating_add(1);
        let request_id = self.remote_import_request_id;
        self.remote_import_in_flight = true;
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
        let (task, handle) = task.abortable();
        self.remote_import_abort = Some(handle);
        task
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
        self.state.browser.remote.catalog_state = crate::state::RemoteCatalogState::Refreshing;
        self.state.browser.remote.refresh_pages = 0;
        self.state.browser.remote.refresh_items = self.state.browser.remote.catalog.entries.len();
        self.state.status_text = "Refreshing Remote catalog…".into();
        let checkpoint = self.state.browser.remote.catalog.checkpoint.clone();
        remote_catalog_page_task(client, checkpoint, 0)
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
        if self.remote_import_in_flight {
            queue_latest_remote_audition(&mut self.pending_remote_audition, entry);
            self.state.status_text = "Remote Audition queued behind active import".into();
            return Task::none();
        }
        if let Some(handle) = self.remote_materialization_abort.take() {
            handle.abort();
        }
        self.remote_audition_cache_lease = None;
        let _ = self.dropbox_cache.set_policy(self.dropbox_cache.policy());
        self.remote_materialization_request_id =
            self.remote_materialization_request_id.saturating_add(1);
        let request_id = self.remote_materialization_request_id;
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
            return Task::none();
        }

        if self.state.browser.audition_enabled {
            self.state.browser.begin_audition_load(&source);
        } else {
            self.state.browser.begin_waveform_load(&source);
        }
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
                source: source.clone(),
                result,
            },
        );
        let (task, handle) = task.abortable();
        self.remote_materialization_abort = Some(handle);
        task
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

    #[test]
    fn cancelled_or_superseded_remote_import_results_cannot_create_project_media() {
        assert!(remote_import_result_is_current(7, 7));
        assert!(!remote_import_result_is_current(8, 7));
        assert!(!remote_import_result_is_current(9, 7));
    }
}
