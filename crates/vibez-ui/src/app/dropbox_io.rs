//! Split out of app.rs; inherent methods on [`super::App`].

use iced::Task;

use vibez_dropbox::{load_app_key_with_env_override, DropboxEntry};

use crate::message::{BrowserImportTarget, Message};

use super::*;

impl App {
    pub(super) fn handle_connect_dropbox(&mut self) -> Task<Message> {
        let Some(app_key) = load_app_key_with_env_override(&self.dropbox_settings) else {
            self.state.browser.dropbox.last_error = Some(
                "No Dropbox app key set. Register an app at dropbox.com/developers/apps \
                    and paste the App key above."
                    .into(),
            );
            return Task::none();
        };
        if self.state.browser.dropbox.auth_in_progress {
            return Task::none();
        }
        self.state.browser.dropbox.auth_in_progress = true;
        self.state.browser.dropbox.last_error = None;
        self.state.status_text = "Opening Dropbox authorisation...".to_string();
        Task::perform(connect_dropbox_async(app_key), |result| {
            Message::DropboxConnected(
                result.map(|(info, tokens)| crate::message::DropboxConnectOutcome { info, tokens }),
            )
        })
    }

    pub(super) fn handle_dropbox_expand_folder(&mut self, path: String) -> Task<Message> {
        self.state.browser.dropbox.expanded.insert(path.clone());
        if self.state.browser.dropbox.folders.contains_key(&path)
            || self
                .state
                .browser
                .dropbox
                .listing_in_progress
                .contains(&path)
        {
            return Task::none();
        }
        let Some(client) = self.dropbox_client.clone() else {
            self.state.browser.dropbox.last_error = Some("Not connected to Dropbox".into());
            return Task::none();
        };
        self.state
            .browser
            .dropbox
            .listing_in_progress
            .insert(path.clone());
        Task::perform(
            list_dropbox_folder_async(client, path),
            |result| match result {
                Ok((path, entries)) => Message::DropboxFolderListed {
                    path,
                    result: Ok(entries),
                },
                Err(err) => Message::DropboxFolderListed {
                    path: String::new(),
                    result: Err(err),
                },
            },
        )
    }

    pub(super) fn handle_dropbox_import_to_arrangement(
        &mut self,
        entry: DropboxEntry,
    ) -> Task<Message> {
        let Some(client) = self.dropbox_client.clone() else {
            self.state.browser.dropbox.last_error = Some("Not connected to Dropbox".into());
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
            self.state.browser.dropbox.last_error = Some("Not connected to Dropbox".into());
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
