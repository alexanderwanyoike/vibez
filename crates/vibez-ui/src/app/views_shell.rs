//! Split out of app.rs; inherent methods on [`super::App`].

use iced::widget::{
    button, canvas, column, container, horizontal_space, mouse_area, row, stack, text,
};
use iced::{Color, Element, Length, Theme};

use crate::domains::browser::BrowserMsg;
use crate::domains::project::ProjectMsg;
use crate::domains::view::ViewMsg;

use crate::icons;
use crate::message::Message;
use crate::state::Workspace;
use crate::theme as th;

use super::*;

pub(super) const HORIZONTAL_PANE_SPLITTER_WIDTH: f32 = 7.0;
pub(super) const VERTICAL_PANE_SPLITTER_HEIGHT: f32 = 7.0;

fn pane_splitter_style(active: bool) -> container::Style {
    container::Style {
        background: Some(
            if active {
                th::accent_dim()
            } else {
                th::divider()
            }
            .into(),
        ),
        ..Default::default()
    }
}

pub(super) fn horizontal_pane_splitter(
    active: bool,
    on_press: Message,
) -> Element<'static, Message> {
    mouse_area(
        container(text(""))
            .width(Length::Fixed(HORIZONTAL_PANE_SPLITTER_WIDTH))
            .height(Length::Fill)
            .style(move |_theme: &Theme| pane_splitter_style(active)),
    )
    .on_press(on_press)
    .interaction(iced::mouse::Interaction::ResizingHorizontally)
    .into()
}

pub(super) fn vertical_pane_splitter(active: bool, on_press: Message) -> Element<'static, Message> {
    mouse_area(
        container(text(""))
            .width(Length::Fill)
            .height(Length::Fixed(VERTICAL_PANE_SPLITTER_HEIGHT))
            .style(move |_theme: &Theme| pane_splitter_style(active)),
    )
    .on_press(on_press)
    .interaction(iced::mouse::Interaction::ResizingVertically)
    .into()
}

impl App {
    // ── View ──

    pub(super) fn view(&self) -> Element<'_, Message> {
        let header = self.view_header();

        let workspace_content = match self.state.view.workspace {
            Workspace::Arrange => self.view_arrangement(),
            Workspace::Perform => self.view_perform(),
            Workspace::Mix => self.view_mixer(),
        };
        let browser_workspace = matches!(
            self.state.view.workspace,
            Workspace::Arrange | Workspace::Perform
        );
        let content: Element<'_, Message> = if browser_workspace && self.state.browser.open {
            row![
                self.view_sample_browser_panel(),
                self.view_browser_splitter(),
                workspace_content
            ]
            .height(Length::FillPortion(5))
            .into()
        } else {
            workspace_content
        };

        let detail_splitter = vertical_pane_splitter(
            self.state.view.detail_panel_resize_active,
            Message::View(ViewMsg::BeginDetailPanelResize),
        );
        let detail_panel = self.view_detail_panel();
        let transport_bar = self.view_transport();
        let status_bar = self.view_status();

        let layout = column![
            header,
            transport_bar,
            content,
            detail_splitter,
            detail_panel,
            status_bar
        ];

        let layout_container = container(layout).width(Length::Fill).height(Length::Fill);
        // Outer mouse_area cancels an active sample-drag on any release
        // that wasn't captured by a drop target (clip canvas, drum pad).
        let base_layout: Element<'_, Message> = mouse_area(layout_container)
            .on_release(Message::Browser(BrowserMsg::EndDragSample))
            .into();
        let base_layout: Element<'_, Message> = if let Some(label) =
            self.state.browser.drag_label.as_ref()
        {
            let mode = match self.state.browser.audition_mode {
                crate::state::AuditionMode::Raw => "RAW",
                crate::state::AuditionMode::Warp => "WARP",
            };
            let length = self
                .state
                .browser
                .drag_preview_beats(self.state.transport.bpm)
                .map(|beats| format!(" · {beats:.2} beats"))
                .unwrap_or_default();
            let (target, valid) = match self.state.browser.drag_target {
                Some(crate::state::BrowserDropTarget::ArrangementLane {
                    beat, compatible, ..
                }) => (
                    if compatible {
                        format!("AUDIO LANE · BEAT {beat:.2}")
                    } else {
                        "INVALID · MIDI/INSTRUMENT LANE".into()
                    },
                    Some(compatible),
                ),
                Some(crate::state::BrowserDropTarget::EmptyArrangement { beat }) => {
                    (format!("NEW AUDIO TRACK · BEAT {beat:.2}"), Some(true))
                }
                Some(crate::state::BrowserDropTarget::Sampler { .. }) => {
                    ("LOAD SAMPLER".into(), Some(true))
                }
                Some(crate::state::BrowserDropTarget::DrumRackPad { pad_index, .. }) => {
                    (format!("ASSIGN DRUM PAD {}", pad_index + 1), Some(true))
                }
                None => ("CHOOSE A DROP TARGET".into(), None),
            };
            let ghost = canvas(crate::widgets::browser_drag_ghost::BrowserDragGhost {
                cursor: iced::Point::new(self.state.view.cursor_x, self.state.view.cursor_y),
                title: format!("{label} · {mode}{length}"),
                detail: target,
                valid,
            })
            .width(Length::Fill)
            .height(Length::Fill);
            stack![base_layout, ghost].into()
        } else {
            base_layout
        };

        if self
            .state
            .arrangement
            .pending_project_track_deletion
            .is_some()
        {
            stack![base_layout, self.view_track_deletion_overlay()].into()
        } else if self.state.settings_open {
            stack![base_layout, self.view_settings_modal()].into()
        } else if self.state.project.file_menu_open {
            stack![base_layout, self.view_file_menu_overlay()].into()
        } else if self.state.view.edit_menu_open {
            stack![base_layout, self.view_edit_menu_overlay()].into()
        } else if self.state.view.context_menu.is_some() {
            stack![base_layout, self.view_context_menu_overlay()].into()
        } else if self.state.view.editing_clip_name.is_some() {
            stack![base_layout, self.view_rename_overlay()].into()
        } else if self.state.devices.context_menu.is_some() {
            stack![base_layout, self.view_device_context_menu_overlay()].into()
        } else {
            base_layout
        }
    }

    pub(super) fn view_header(&self) -> Element<'_, Message> {
        let title = text("vibez").size(22).color(th::accent());

        // Workspace tabs
        let arrange_tab = {
            let active = self.state.view.workspace == Workspace::Arrange;
            let (bg, text_color, border_color) = if active {
                (th::bg_elevated(), th::accent(), th::accent_dim())
            } else {
                (
                    iced::Color::TRANSPARENT,
                    th::text_dim(),
                    iced::Color::TRANSPARENT,
                )
            };
            button(
                row![
                    icons::icon(icons::LAYOUT_LIST).size(13).color(text_color),
                    text("Arrange").size(13).color(text_color)
                ]
                .spacing(4)
                .align_y(iced::Alignment::Center),
            )
            .on_press(Message::View(ViewMsg::SwitchWorkspace(Workspace::Arrange)))
            .padding([6, 14])
            .style(move |_theme: &Theme, _status| button::Style {
                background: Some(bg.into()),
                text_color,
                border: iced::Border {
                    color: border_color,
                    width: if active { 1.0 } else { 0.0 },
                    radius: 4.0.into(),
                },
                ..Default::default()
            })
        };

        let mix_tab = {
            let active = self.state.view.workspace == Workspace::Mix;
            let (bg, text_color, border_color) = if active {
                (th::bg_elevated(), th::accent(), th::accent_dim())
            } else {
                (
                    iced::Color::TRANSPARENT,
                    th::text_dim(),
                    iced::Color::TRANSPARENT,
                )
            };
            button(
                row![
                    icons::icon(icons::SLIDERS_VERTICAL)
                        .size(13)
                        .color(text_color),
                    text("Mix").size(13).color(text_color)
                ]
                .spacing(4)
                .align_y(iced::Alignment::Center),
            )
            .on_press(Message::View(ViewMsg::SwitchWorkspace(Workspace::Mix)))
            .padding([6, 14])
            .style(move |_theme: &Theme, _status| button::Style {
                background: Some(bg.into()),
                text_color,
                border: iced::Border {
                    color: border_color,
                    width: if active { 1.0 } else { 0.0 },
                    radius: 4.0.into(),
                },
                ..Default::default()
            })
        };

        let perform_tab = {
            let active = self.state.view.workspace == Workspace::Perform;
            let (bg, text_color, border_color) = if active {
                (th::bg_elevated(), th::accent(), th::accent_dim())
            } else {
                (
                    iced::Color::TRANSPARENT,
                    th::text_dim(),
                    iced::Color::TRANSPARENT,
                )
            };
            button(
                row![
                    icons::icon(icons::MUSIC).size(13).color(text_color),
                    text("Perform").size(13).color(text_color)
                ]
                .spacing(4)
                .align_y(iced::Alignment::Center),
            )
            .on_press(Message::View(ViewMsg::SwitchWorkspace(Workspace::Perform)))
            .padding([6, 14])
            .style(move |_theme: &Theme, _status| button::Style {
                background: Some(bg.into()),
                text_color,
                border: iced::Border {
                    color: border_color,
                    width: if active { 1.0 } else { 0.0 },
                    radius: 4.0.into(),
                },
                ..Default::default()
            })
        };

        let tabs = row![perform_tab, arrange_tab, mix_tab].spacing(4);

        let file_btn = button(text("File").size(13).color(th::text_dim()))
            .on_press(Message::Project(ProjectMsg::ToggleFileMenu))
            .padding([6, 14])
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
            });

        let edit_btn = button(text("Edit").size(13).color(th::text_dim()))
            .on_press(Message::View(ViewMsg::ToggleEditMenu))
            .padding([6, 14])
            .style(|_theme: &Theme, status| {
                let background = match status {
                    button::Status::Hovered | button::Status::Pressed => {
                        Some(th::bg_hover().into())
                    }
                    _ => None,
                };
                button::Style {
                    background,
                    text_color: th::text_dim(),
                    border: iced::Border::default(),
                    ..Default::default()
                }
            });

        let browser_active = self.state.browser.open;
        let browser_btn = button(
            row![
                icons::icon(icons::AUDIO_WAVEFORM)
                    .size(13)
                    .color(if browser_active {
                        th::accent()
                    } else {
                        th::text_dim()
                    }),
                text("Browser").size(13).color(if browser_active {
                    th::accent()
                } else {
                    th::text_dim()
                })
            ]
            .spacing(4)
            .align_y(iced::Alignment::Center),
        )
        .on_press(Message::Browser(BrowserMsg::ToggleSampleBrowser))
        .padding([6, 14])
        .style(move |_theme: &Theme, status| {
            let bg = if browser_active {
                Some(th::bg_elevated().into())
            } else {
                match status {
                    button::Status::Hovered | button::Status::Pressed => {
                        Some(th::bg_hover().into())
                    }
                    _ => None,
                }
            };
            button::Style {
                background: bg,
                text_color: if browser_active {
                    th::accent()
                } else {
                    th::text_dim()
                },
                border: iced::Border {
                    color: if browser_active {
                        th::accent_dim()
                    } else {
                        Color::TRANSPARENT
                    },
                    width: if browser_active { 1.0 } else { 0.0 },
                    radius: 4.0.into(),
                },
                ..Default::default()
            }
        });

        let header_row = row![
            title,
            file_btn,
            edit_btn,
            browser_btn,
            tabs,
            horizontal_space()
        ]
        .spacing(8);

        let header = header_row.padding(10).align_y(iced::Alignment::Center);

        container(header)
            .width(Length::Fill)
            .style(|_theme: &Theme| container::Style {
                background: Some(th::bg_surface().into()),
                border: iced::Border {
                    color: th::border(),
                    width: 0.0,
                    radius: 0.0.into(),
                },
                ..Default::default()
            })
            .into()
    }

    pub(super) fn view_status(&self) -> Element<'_, Message> {
        let status = text(&self.state.status_text).size(11).color(th::text_dim());

        container(status)
            .width(Length::Fill)
            .padding([3, 12])
            .style(|_theme: &Theme| container::Style {
                background: Some(th::bg_dark().into()),
                ..Default::default()
            })
            .into()
    }
}
