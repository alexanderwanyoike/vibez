//! The control bar across the top of the window, in one row when the window
//! is wide enough and two when it is not.

use iced::widget::{button, column, container, horizontal_space, row, text};
use iced::{Alignment, Element, Length};

use crate::domains::browser::BrowserMsg;
use crate::domains::project::ProjectMsg;
use crate::domains::view::ViewMsg;

use crate::icons;
use crate::message::Message;
use crate::state::Workspace;
use crate::theme as th;

use super::views_control_bar_style as style;
use super::*;

/// The narrowest window, in interface pixels, that holds the whole bar in
/// one row without its groups touching.
const ONE_ROW_MIN_WIDTH: f32 = 1270.0;
const BAR_PADDING: f32 = 12.0;

/// Where the File and Edit menus open, under their buttons.
pub(super) const MENU_TOP: f32 = 46.0;
pub(super) const FILE_MENU_LEFT: f32 = 68.0;
pub(super) const EDIT_MENU_LEFT: f32 = 110.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ControlBarRows {
    One,
    Two,
}

pub(super) fn control_bar_rows(window_width: f32) -> ControlBarRows {
    if window_width >= ONE_ROW_MIN_WIDTH {
        ControlBarRows::One
    } else {
        ControlBarRows::Two
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct WorkspaceSegment {
    pub workspace: Workspace,
    pub label: &'static str,
    pub selected: bool,
}

pub(super) fn workspace_segments(current: Workspace) -> [WorkspaceSegment; 3] {
    [
        (Workspace::Perform, "Perform"),
        (Workspace::Arrange, "Arrange"),
        (Workspace::Mix, "Mix"),
    ]
    .map(|(workspace, label)| WorkspaceSegment {
        workspace,
        label,
        selected: workspace == current,
    })
}

fn bar_row<'a>(content: iced::widget::Row<'a, Message>) -> Element<'a, Message> {
    container(content.spacing(10).align_y(Alignment::Center))
        .padding([0.0, BAR_PADDING])
        .center_y(Length::Fixed(style::BAR_HEIGHT))
        .into()
}

impl App {
    pub(super) fn view_control_bar(&self) -> Element<'_, Message> {
        let rows: Element<'_, Message> = match control_bar_rows(self.state.view.window_width) {
            ControlBarRows::One => bar_row(row![
                self.view_control_bar_menus(),
                horizontal_space(),
                self.view_transport(),
                horizontal_space(),
                self.view_control_status(),
            ]),
            ControlBarRows::Two => column![
                bar_row(row![
                    self.view_control_bar_menus(),
                    horizontal_space(),
                    self.view_control_status(),
                ]),
                bar_row(row![
                    horizontal_space(),
                    self.view_transport(),
                    horizontal_space()
                ]),
            ]
            .into(),
        };
        column![
            container(rows).width(Length::Fill).style(style::bar),
            style::bar_rule(),
        ]
        .into()
    }

    fn view_control_bar_menus(&self) -> Element<'_, Message> {
        let logo = text("vibez").size(20).color(th::accent());
        let menu = |label: &'static str, message: Message| {
            button(text(label).size(13))
                .on_press(message)
                .padding([5, 8])
                .style(style::segment(None))
        };
        let browser = container(
            button(
                container(icons::icon(icons::PANEL_LEFT).size(14))
                    .center_x(Length::Fill)
                    .center_y(Length::Fill),
            )
            .on_press(Message::Browser(BrowserMsg::ToggleSampleBrowser))
            .width(Length::Fixed(28.0))
            .height(Length::Fixed(24.0))
            .padding(0)
            .style(style::segment(self.state.browser.open.then(th::accent))),
        )
        .padding(style::GROUP_INSET)
        .style(style::group);

        row![
            logo,
            row![
                menu("File", Message::Project(ProjectMsg::ToggleFileMenu)),
                menu("Edit", Message::View(ViewMsg::ToggleEditMenu)),
            ]
            .spacing(2),
            browser,
            self.view_workspace_switch(),
        ]
        .spacing(10)
        .align_y(Alignment::Center)
        .into()
    }

    fn view_workspace_switch(&self) -> Element<'_, Message> {
        let segments = workspace_segments(self.state.view.workspace).map(|segment| {
            button(text(segment.label).size(12).font(if segment.selected {
                crate::typography::PERFORM_LABEL
            } else {
                iced::Font::DEFAULT
            }))
            .on_press(Message::View(ViewMsg::SwitchWorkspace(segment.workspace)))
            .padding([4, 10])
            .style(style::segment(segment.selected.then(th::text)))
            .into()
        });
        container(row(segments).spacing(2))
            .padding(style::GROUP_INSET)
            .style(style::group)
            .into()
    }
}

#[cfg(test)]
mod tests {
    use super::{control_bar_rows, workspace_segments, ControlBarRows};
    use crate::state::Workspace;

    #[test]
    fn the_workspace_switch_marks_only_the_current_workspace() {
        for current in [Workspace::Perform, Workspace::Arrange, Workspace::Mix] {
            let selected: Vec<Workspace> = workspace_segments(current)
                .iter()
                .filter(|segment| segment.selected)
                .map(|segment| segment.workspace)
                .collect();
            assert_eq!(selected, vec![current]);
        }
        assert_eq!(
            workspace_segments(Workspace::Mix).map(|segment| segment.label),
            ["Perform", "Arrange", "Mix"]
        );
    }

    #[test]
    fn the_control_bar_is_one_row_when_it_fits_and_two_when_it_does_not() {
        assert_eq!(control_bar_rows(1440.0), ControlBarRows::One);
        assert_eq!(control_bar_rows(960.0), ControlBarRows::Two);
    }
}
