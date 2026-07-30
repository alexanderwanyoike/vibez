//! The Dropbox remote-storage settings tab.
//!
//! Lives apart from `views_settings.rs` only because that file sits at
//! the repository's 1,000-line ceiling; the tab is otherwise an
//! ordinary sibling of the other settings tabs.

use iced::widget::{button, column, horizontal_space, row, slider, text, text_input};
use iced::{Element, Length, Theme};

use crate::message::Message;
use crate::theme as th;

use super::views_browser_style::browser_utility_action_style;
use super::views_settings::format_settings_bytes;
use super::*;

impl App {
    pub(super) fn view_settings_dropbox_tab(&self) -> Element<'_, Message> {
        let title = text("Dropbox").size(14).color(th::text());
        let hint = text(
            "Register an app at https://www.dropbox.com/developers/apps \
            (Scoped access, Full Dropbox). Paste the App key below.",
        )
        .size(11)
        .color(th::text_dim());

        let app_key_input = text_input("App key", &self.state.browser.remote.app_key_input)
            .on_input(|s| Message::Browser(BrowserMsg::SetDropboxAppKey(s)))
            .on_submit(Message::SaveDropboxAppKey)
            .size(13)
            .width(Length::Fill);
        let save_key_btn = button(text("Save").size(12).color(th::text()))
            .on_press(Message::SaveDropboxAppKey)
            .padding([6, 12])
            .style(|_theme: &Theme, status| {
                let bg = match status {
                    button::Status::Hovered | button::Status::Pressed => {
                        Some(th::bg_hover().into())
                    }
                    _ => None,
                };
                button::Style {
                    background: bg,
                    text_color: th::text(),
                    border: iced::Border {
                        color: th::accent_dim(),
                        width: 1.0,
                        radius: 4.0.into(),
                    },
                    ..Default::default()
                }
            });

        let key_row = row![app_key_input, save_key_btn]
            .spacing(8)
            .align_y(iced::Alignment::Center);

        let account_line: Element<'_, Message> = if self.state.browser.remote.connected {
            let email = self
                .state
                .browser
                .remote
                .account_email
                .clone()
                .unwrap_or_else(|| "connected".into());
            text(format!("Connected: {email}"))
                .size(12)
                .color(th::accent())
                .into()
        } else if self.state.browser.remote.auth_in_progress {
            text("Waiting for browser authorisation...")
                .size(12)
                .color(th::text_dim())
                .into()
        } else {
            text("Not connected").size(12).color(th::text_dim()).into()
        };

        let can_connect =
            self.state.browser.remote.has_app_key && !self.state.browser.remote.auth_in_progress;
        let connect_label = if self.state.browser.remote.auth_in_progress {
            "Connecting..."
        } else if self.state.browser.remote.connected {
            "Reconnect"
        } else {
            "Connect"
        };
        let connect_btn = {
            let mut btn = button(text(connect_label).size(12).color(th::accent()));
            if can_connect {
                btn = btn.on_press(Message::ConnectDropbox);
            }
            btn.padding([6, 12]).style(|_theme: &Theme, status| {
                let bg = match status {
                    button::Status::Hovered | button::Status::Pressed => {
                        Some(th::bg_hover().into())
                    }
                    _ => None,
                };
                button::Style {
                    background: bg,
                    text_color: th::accent(),
                    border: iced::Border {
                        color: th::accent_dim(),
                        width: 1.0,
                        radius: 4.0.into(),
                    },
                    ..Default::default()
                }
            })
        };

        let disconnect_btn: Element<'_, Message> = if self.state.browser.remote.connected {
            button(text("Disconnect").size(12).color(th::text_dim()))
                .on_press(Message::DisconnectDropbox)
                .padding([6, 12])
                .style(|_theme: &Theme, status| {
                    let bg = match status {
                        button::Status::Hovered | button::Status::Pressed => {
                            Some(th::bg_hover().into())
                        }
                        _ => None,
                    };
                    button::Style {
                        background: bg,
                        text_color: th::text_dim(),
                        border: iced::Border::default(),
                        ..Default::default()
                    }
                })
                .into()
        } else {
            horizontal_space().width(Length::Shrink).into()
        };

        let error_line: Element<'_, Message> =
            if let Some(err) = self.state.browser.remote.last_error.clone() {
                text(err).size(11).color(th::danger()).into()
            } else {
                horizontal_space().width(Length::Shrink).into()
            };

        let budget_gib =
            self.state.browser.remote.cache_budget_bytes as f32 / (1024.0 * 1024.0 * 1024.0);
        let cache_usage = text(format!(
            "{} across {} item(s)",
            format_settings_bytes(self.state.browser.remote.cache_usage_bytes),
            self.state.browser.remote.cache_entries
        ))
        .size(11)
        .color(th::text_dim());
        let budget = slider(1.0..=500.0, budget_gib, Message::SetMediaCacheBudgetGiB)
            .step(1.0_f32)
            .width(Length::Fill);
        let eviction_enabled = self.state.browser.remote.cache_automatic_eviction;
        let eviction = button(
            text(if eviction_enabled {
                "LRU EVICTION ON"
            } else {
                "LRU EVICTION OFF"
            })
            .size(10)
            .color(if eviction_enabled {
                th::accent()
            } else {
                th::text_dim()
            }),
        )
        .on_press(Message::ToggleMediaCacheAutomaticEviction)
        .padding([5, 8])
        .style(browser_utility_action_style);
        let clear = button(text("CLEAR CACHE").size(10).color(th::text_dim()))
            .on_press(Message::ClearMediaCache)
            .padding([5, 8])
            .style(browser_utility_action_style);
        let cache_error: Element<'_, Message> = self
            .state
            .browser
            .remote
            .cache_error
            .as_ref()
            .map(|error| text(error).size(10).color(th::danger()).into())
            .unwrap_or_else(|| horizontal_space().width(Length::Shrink).into());

        column![
            title,
            hint,
            key_row,
            account_line,
            row![connect_btn, disconnect_btn]
                .spacing(8)
                .align_y(iced::Alignment::Center),
            error_line,
            text("MEDIA CACHE").size(10).color(th::text_muted()),
            row![
                cache_usage,
                horizontal_space(),
                text(format!("{budget_gib:.0} GiB budget"))
                    .size(11)
                    .color(th::text_dim())
            ]
            .align_y(iced::Alignment::Center),
            budget,
            row![eviction, clear]
                .spacing(8)
                .align_y(iced::Alignment::Center),
            cache_error,
        ]
        .spacing(10)
        .into()
    }
}
