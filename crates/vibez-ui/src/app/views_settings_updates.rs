//! The Updates settings tab: opt in or out of the startup release
//! check, and see what the last one found.

use iced::widget::row;
use iced::Element;

use crate::message::Message;
use crate::update_check;

use super::views_settings_rows::{action, group, hint, setting_row, switch};
use super::*;

impl App {
    pub(super) fn view_settings_updates_tab(&self) -> Element<'_, Message> {
        let state = &self.state.update_check;
        // Report only what the user can act on. A failed check is
        // indistinguishable from "nothing new" by design, so the
        // absence of a version here is never framed as an error.
        let found = match &state.available {
            Some(version) => format!("Version {version} is available."),
            None => "No newer version found.".into(),
        };
        let mut actions = row![action(
            if state.in_flight {
                "Checking…"
            } else {
                "Check now"
            },
            (!state.in_flight).then_some(Message::CheckForUpdatesNow),
            false,
        )]
        .spacing(6);
        if state.available.is_some() {
            actions = actions.push(action(
                "View releases",
                Some(Message::OpenReleasesPage),
                true,
            ));
        }
        group([
            setting_row(
                "Check for updates on startup",
                Some(
                    hint(
                        "Asks GitHub once a day whether a newer version is out. Nothing \
                         is downloaded; a dismissible chip in the status bar links to \
                         the releases page. Off means no request at all.",
                    )
                    .into(),
                ),
                switch(state.enabled, Message::ToggleCheckForUpdates),
            ),
            setting_row(
                format!("Version {}", update_check::current_version()),
                Some(hint(found).into()),
                actions,
            ),
        ])
    }
}
