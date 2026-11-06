//! The Dropbox remote-storage settings tab.

use iced::widget::{column, row, text_input};
use iced::Element;

use crate::message::Message;

use super::views_settings::format_settings_bytes;
use super::views_settings_rows::{
    action, group, hint, setting_row, slider_control, status, switch,
};
use super::*;

impl App {
    pub(super) fn view_settings_dropbox_tab(&self) -> Element<'_, Message> {
        let remote = &self.state.browser.remote;
        let account = if remote.connected {
            format!(
                "Connected: {}",
                remote.account_email.as_deref().unwrap_or("connected")
            )
        } else if remote.auth_in_progress {
            "Waiting for browser authorisation…".into()
        } else {
            "Not connected".into()
        };
        let connect_label = if remote.auth_in_progress {
            "Connecting…"
        } else if remote.connected {
            "Reconnect"
        } else {
            "Connect"
        };
        let mut connection = row![action(
            connect_label,
            (remote.has_app_key && !remote.auth_in_progress).then_some(Message::ConnectDropbox),
            true,
        )]
        .spacing(6);
        if remote.connected {
            connection = connection.push(action(
                "Disconnect",
                Some(Message::DisconnectDropbox),
                false,
            ));
        }
        let mut account_rows: Vec<Element<'_, Message>> = vec![
            setting_row(
                "App key",
                Some(
                    hint(
                        "Register an app at dropbox.com/developers/apps with scoped, full \
                         Dropbox access, then paste its key.",
                    )
                    .into(),
                ),
                row![
                    text_input("App key", &remote.app_key_input)
                        .on_input(|key| Message::Browser(BrowserMsg::SetDropboxAppKey(key)))
                        .on_submit(Message::SaveDropboxAppKey)
                        .size(11.5)
                        .padding([6, 10])
                        .width(180)
                        .style(super::views_browser_style::browser_search_style),
                    action("Save", Some(Message::SaveDropboxAppKey), false),
                ]
                .spacing(6)
                .align_y(iced::Alignment::Center),
            ),
            setting_row("Account", Some(hint(account).into()), connection),
        ];
        if let Some(error) = remote.last_error.clone() {
            account_rows.push(status(error, true));
        }

        let budget_gib = remote.cache_budget_bytes as f32 / (1024.0 * 1024.0 * 1024.0);
        let mut cache_rows: Vec<Element<'_, Message>> = vec![
            setting_row(
                "Media cache budget",
                Some(
                    hint(format!(
                        "{} across {} item(s) kept on disk for audition and playback.",
                        format_settings_bytes(remote.cache_usage_bytes),
                        remote.cache_entries
                    ))
                    .into(),
                ),
                slider_control(
                    1.0..=500.0,
                    budget_gib,
                    1.0,
                    Message::SetMediaCacheBudgetGiB,
                    format!("{budget_gib:.0} GiB"),
                ),
            ),
            setting_row(
                "Evict least recently used",
                Some(hint("Frees space automatically once the budget is reached.").into()),
                switch(
                    remote.cache_automatic_eviction,
                    Message::ToggleMediaCacheAutomaticEviction,
                ),
            ),
            setting_row(
                "Clear cache",
                Some(hint("Removes every cached file now.").into()),
                action("Clear cache", Some(Message::ClearMediaCache), false),
            ),
        ];
        if let Some(error) = remote.cache_error.as_ref() {
            cache_rows.push(status(error, true));
        }

        column![group(account_rows), group(cache_rows)]
            .spacing(16)
            .into()
    }
}
