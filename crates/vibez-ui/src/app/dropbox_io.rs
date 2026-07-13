//! Split out of app.rs; inherent methods on [`super::App`].

use iced::Task;

use vibez_dropbox::{load_app_key_with_env_override, DropboxEntry};

use crate::message::{BrowserImportTarget, Message};

use super::*;

impl App {
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
        let Some(client) = self.dropbox_client.clone() else {
            self.state.browser.remote.catalog_state =
                crate::state::RemoteCatalogState::AuthenticationRequired {
                    error: "Connect Dropbox in Settings to refresh".into(),
                };
            return Task::none();
        };
        self.state.browser.remote.catalog_state = crate::state::RemoteCatalogState::Refreshing;
        let checkpoint = self.state.browser.remote.catalog.checkpoint.clone();
        Task::perform(
            async move {
                let provider =
                    crate::remote_provider::DropboxRemoteProvider::new((*client).clone());
                crate::remote_provider::refresh_remote_catalog(&provider, checkpoint.as_deref())
                    .await
            },
            Message::RemoteCatalogRefreshed,
        )
    }

    pub(super) fn handle_dropbox_import_to_arrangement(
        &mut self,
        entry: DropboxEntry,
    ) -> Task<Message> {
        let Some(client) = self.dropbox_client.clone() else {
            self.state.browser.remote.last_error = Some("Not connected to Dropbox".into());
            return Task::none();
        };
        let cache = self.dropbox_cache.clone();
        let target = BrowserImportTarget::ArrangementClip(self.state.arrangement.selected_track);
        let Some(treatment) = self.state.browser.audition_import_input() else {
            self.state.status_text = "Confirm source BPM before WARP import".into();
            return Task::none();
        };
        self.state.status_text = format!("Importing {}...", entry.name);
        Task::perform(
            fetch_dropbox_sample_async(client, cache, entry),
            move |result| match result {
                Ok((audio, name, source)) => {
                    Message::BrowserSampleDecoded(target.clone(), treatment, audio, name, source)
                }
                Err(err) => Message::BrowserSampleDecodeError(err),
            },
        )
    }

    pub(super) fn handle_dropbox_import_to_device(&mut self, entry: DropboxEntry) -> Task<Message> {
        let Some(client) = self.dropbox_client.clone() else {
            self.state.browser.remote.last_error = Some("Not connected to Dropbox".into());
            return Task::none();
        };
        let Some(target) = self.selected_browser_device_target() else {
            self.state.status_text = "Select a Sampler or Drum Pad track first".into();
            return Task::none();
        };
        let cache = self.dropbox_cache.clone();
        let Some(treatment) = self.state.browser.audition_import_input() else {
            self.state.status_text = "Confirm source BPM before WARP import".into();
            return Task::none();
        };
        self.state.status_text = format!("Importing {}...", entry.name);
        Task::perform(
            fetch_dropbox_sample_async(client, cache, entry),
            move |result| match result {
                Ok((audio, name, source)) => {
                    Message::BrowserSampleDecoded(target.clone(), treatment, audio, name, source)
                }
                Err(err) => Message::BrowserSampleDecodeError(err),
            },
        )
    }
}
