//! Perform's header on the frame above its cards: the mode switch, the
//! record status and the record group, in as few rows as fit.

use iced::widget::{column, container, row, Row, Space};
use iced::{Alignment, Element, Length};

use crate::domains::perform::{PerformMode, PerformMsg};
use crate::message::Message;
use crate::widgets::segment_group;

use super::App;

/// The narrowest workspaces, in interface pixels, that hold the switch, a
/// readable status and Sections' record group, the wider of the two, side
/// by side, and the switch beside the record group alone.
const ONE_ROW_MIN_WIDTH: f32 = 1_320.0;
const STATUS_BELOW_MIN_WIDTH: f32 = 930.0;
const GAP: f32 = 12.0;
const RECORD_SPACING: f32 = 6.0;
const GROUP_SPACING: f32 = 4.0;
const STATUS_SPACING: f32 = 4.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum HeaderRows {
    /// The switch, the status and the record group side by side.
    One,
    /// The switch and the record group share a row, the status beneath.
    StatusBelow,
    /// The switch and the status share a row, the record group beneath,
    /// wrapping onto more rows if it must.
    Stacked,
}

pub(super) fn header_rows(workspace_width: f32) -> HeaderRows {
    if workspace_width >= ONE_ROW_MIN_WIDTH {
        HeaderRows::One
    } else if workspace_width >= STATUS_BELOW_MIN_WIDTH {
        HeaderRows::StatusBelow
    } else {
        HeaderRows::Stacked
    }
}

/// What a Perform layout records with: its status line and its controls.
pub(super) struct RecordGroup<'a> {
    pub status: Element<'a, Message>,
    pub controls: Vec<Element<'a, Message>>,
}

impl App {
    /// The header row above `workspace`, then the workspace's cards.
    pub(super) fn perform_cards<'a>(
        &'a self,
        record: RecordGroup<'a>,
        workspace: impl Into<Element<'a, Message>>,
    ) -> Element<'a, Message> {
        column![
            self.view_perform_header(record),
            Space::with_height(crate::shell_layout::PANE_GAP),
            workspace.into()
        ]
        .height(Length::Fill)
        .into()
    }

    fn view_perform_header<'a>(&'a self, record: RecordGroup<'a>) -> Element<'a, Message> {
        let switch = segment_group::switch(PerformMode::ALL.map(|mode| segment_group::Choice {
            label: if mode == PerformMode::Sections {
                self.state.perform.layout.label()
            } else {
                mode.label()
            },
            shortcut: Some(mode.shortcut()),
            selected: self.state.perform.mode == mode,
            on_press: Message::Perform(PerformMsg::SelectMode(mode)),
        }));
        let status = container(record.status).clip(true);
        let record_group = Row::with_children(record.controls)
            .spacing(GROUP_SPACING)
            .align_y(Alignment::Center);
        let grouped = |content: Element<'a, Message>| {
            container(content)
                .padding(segment_group::GROUP_INSET)
                .style(segment_group::group)
        };
        match header_rows(self.browser_workspace_width()) {
            HeaderRows::One => row![
                switch,
                status.width(Length::Fill),
                grouped(record_group.into())
            ]
            .spacing(GAP)
            .align_y(Alignment::Center)
            .into(),
            HeaderRows::StatusBelow => column![
                row![
                    container(switch).width(Length::Fill),
                    grouped(record_group.into())
                ]
                .spacing(GAP)
                .align_y(Alignment::Center),
                status,
            ]
            .spacing(STATUS_SPACING)
            .into(),
            HeaderRows::Stacked => column![
                row![switch, status.width(Length::Fill)]
                    .spacing(GAP)
                    .align_y(Alignment::Center),
                grouped(record_group.wrap().into()),
            ]
            .spacing(RECORD_SPACING)
            .into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_header_adds_rows_before_controls_or_status_can_be_squashed() {
        assert_eq!(header_rows(ONE_ROW_MIN_WIDTH), HeaderRows::One);
        assert_eq!(
            header_rows(ONE_ROW_MIN_WIDTH - 1.0),
            HeaderRows::StatusBelow
        );
        assert_eq!(header_rows(STATUS_BELOW_MIN_WIDTH), HeaderRows::StatusBelow);
        assert_eq!(
            header_rows(STATUS_BELOW_MIN_WIDTH - 1.0),
            HeaderRows::Stacked
        );
    }

    #[test]
    fn the_header_takes_one_row_only_where_switch_status_and_record_group_all_fit() {
        assert_eq!(header_rows(1424.0), HeaderRows::One, "1440 window");
        assert_eq!(
            header_rows(1300.0),
            HeaderRows::StatusBelow,
            "no room for the status between the switch and the record group"
        );
        assert_eq!(
            header_rows(1006.0),
            HeaderRows::StatusBelow,
            "1440 beside the Browser"
        );
        assert_eq!(
            header_rows(846.0),
            HeaderRows::Stacked,
            "1280 beside the Browser: the record group gets a row of its own"
        );
    }
}
