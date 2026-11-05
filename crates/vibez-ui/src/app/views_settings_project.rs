//! Project persistence preferences.

use iced::Element;

use crate::message::Message;

use super::views_settings_rows::{group, hint, setting_row, switch};
use super::*;

impl App {
    pub(super) fn view_settings_project_tab(&self) -> Element<'_, Message> {
        group([
            setting_row(
                "Auto save named projects",
                Some(
                    hint(
                        "Saves to the project's own file two seconds after editing stops. \
                         Untitled projects wait for Save As.",
                    )
                    .into(),
                ),
                switch(self.state.auto_save_enabled, Message::ToggleAutoSave),
            ),
            setting_row(
                "Reduced latency monitoring",
                Some(
                    hint(
                        "Monitored tracks wait less for delay compensation. Their clips \
                         share the earlier timing; sends and returns stay active.",
                    )
                    .into(),
                ),
                switch(
                    self.state.project_tracks.reduced_latency_monitoring,
                    Message::SetReducedLatencyMonitoring(
                        !self.state.project_tracks.reduced_latency_monitoring,
                    ),
                ),
            ),
        ])
    }
}
