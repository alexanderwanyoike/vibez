//! Split out of app.rs; inherent methods on [`super::App`].

use iced::widget::{
    button, canvas, column, container, horizontal_space, mouse_area, progress_bar, row, stack,
    text, Space,
};
use iced::{Element, Length, Theme};

use crate::domains::browser::BrowserMsg;
use crate::domains::view::ViewMsg;

use crate::icons;
use crate::message::Message;
use crate::shell_layout::PANE_GAP;
use crate::state::Workspace;
use crate::theme as th;
use crate::widgets::shell_card::shell_card;

use super::*;

const STATUS_TEXT_SIZE: f32 = 11.0;
const STATUS_SPACING: f32 = 20.0;

/// A gap between cards that resizes them, marked by a slim rule while it
/// is being dragged.
fn pane_splitter<'a>(
    active: bool,
    vertical: bool,
    on_press: Message,
    interaction: iced::mouse::Interaction,
) -> Element<'a, Message> {
    let rule = container(Space::new(Length::Fill, Length::Fill)).style(move |_theme: &Theme| {
        container::Style {
            background: active.then(|| th::accent_dim().into()),
            ..Default::default()
        }
    });
    let inset = (PANE_GAP - PANE_RULE) / 2.0;
    let gap = if vertical {
        container(rule)
            .width(Length::Fixed(PANE_GAP))
            .height(Length::Fill)
            .padding([0.0, inset])
    } else {
        container(rule)
            .width(Length::Fill)
            .height(Length::Fixed(PANE_GAP))
            .padding([inset, 0.0])
    };
    mouse_area(gap)
        .on_press(on_press)
        .interaction(interaction)
        .into()
}

const PANE_RULE: f32 = 2.0;

pub(super) fn horizontal_pane_splitter(
    active: bool,
    on_press: Message,
) -> Element<'static, Message> {
    pane_splitter(
        active,
        true,
        on_press,
        iced::mouse::Interaction::ResizingHorizontally,
    )
}

pub(super) fn vertical_pane_splitter(active: bool, on_press: Message) -> Element<'static, Message> {
    pane_splitter(
        active,
        false,
        on_press,
        iced::mouse::Interaction::ResizingVertically,
    )
}

impl App {
    /// Where the Arrange or Perform workspace sits beside the Browser.
    pub(super) fn workspace_span(&self) -> crate::shell_layout::Span {
        let browser_width = self.state.browser.open.then(|| {
            self.state
                .browser
                .effective_dock_width(self.state.view.window_width)
        });
        crate::shell_layout::workspace_span(self.state.view.window_width, browser_width)
    }

    /// Width left for the Arrange or Perform workspace beside the Browser.
    pub(super) fn browser_workspace_width(&self) -> f32 {
        self.workspace_span().width
    }

    // ── View ──

    pub(super) fn view(&self) -> Element<'_, Message> {
        let workspace: Element<'_, Message> = match self.state.view.workspace {
            Workspace::Mix => shell_card(self.view_mixer())
                .height(Length::Fixed(self.mix_workspace_height()))
                .into(),
            workspace => container(if workspace == Workspace::Arrange {
                shell_card(self.view_arrangement()).into()
            } else {
                self.view_perform()
            })
            .height(Length::Fill)
            .into(),
        };
        let work_column = column![
            workspace,
            vertical_pane_splitter(
                self.state.view.detail_panel_resize_active,
                Message::View(ViewMsg::BeginDetailPanelResize),
            ),
            self.view_detail_panel()
        ];
        let browser_workspace = matches!(
            self.state.view.workspace,
            Workspace::Arrange | Workspace::Perform
        );
        let body: Element<'_, Message> = if browser_workspace && self.state.browser.open {
            row![
                self.view_sample_browser_panel(),
                self.view_browser_splitter(),
                work_column
            ]
            .into()
        } else {
            work_column.into()
        };

        let layout = column![
            self.view_control_bar(),
            container(body)
                .padding([0.0, PANE_GAP])
                .height(Length::Fill),
            Space::with_height(PANE_GAP),
            self.view_status()
        ];

        let layout_container = container(layout)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(|_theme: &Theme| container::Style {
                background: Some(th::display_bg().into()),
                ..Default::default()
            });
        // Outer mouse_area cancels an active sample-drag on any release
        // that wasn't captured by a drop target (clip canvas, drum pad).
        let base_layout: Element<'_, Message> = mouse_area(layout_container)
            .on_release(Message::Browser(BrowserMsg::EndDragSample))
            .into();
        let base_layout: Element<'_, Message> =
            if let Some(label) = self.state.browser.drag_label.as_ref() {
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
                let (target, valid) = self.state.browser.drag_ghost_label();
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

        // Quitting outranks every other overlay: whatever menu was open, the
        // answer to "are you about to lose work" has to be the visible one.
        if self.state.project.close_confirm_open {
            stack![base_layout, self.view_close_confirm_overlay()].into()
        } else if self.state.project.new_project_layout.is_some() {
            stack![base_layout, self.view_new_project_overlay()].into()
        } else if self
            .state
            .arrangement
            .pending_project_track_deletion
            .is_some()
        {
            stack![base_layout, self.view_track_deletion_overlay()].into()
        } else if self.state.view.transient_analysis_dialog.is_some() {
            stack![base_layout, self.view_transient_analysis_overlay()].into()
        } else if self.state.view.drum_rack_slice_dialog.is_some() {
            stack![base_layout, self.view_drum_rack_slice_overlay()].into()
        } else if self.state.settings_open {
            stack![base_layout, self.view_settings_modal()].into()
        } else if self.state.about_open {
            stack![base_layout, self.view_about_modal()].into()
        } else if self.state.project.file_menu_open {
            stack![base_layout, self.view_file_menu_overlay()].into()
        } else if self.state.view.edit_menu_open {
            stack![base_layout, self.view_edit_menu_overlay()].into()
        } else if self.state.view.context_menu.is_some() {
            stack![base_layout, self.view_context_menu_overlay()].into()
        } else if self.state.view.editing_clip_name.is_some()
            && !(self.state.view.workspace == Workspace::Perform
                && self.state.perform.layout == vibez_project::PerformLayout::Clips)
        {
            stack![base_layout, self.view_rename_overlay()].into()
        } else if self.state.devices.context_menu.is_some() {
            stack![base_layout, self.view_device_context_menu_overlay()].into()
        } else {
            base_layout
        }
    }

    /// The release notice: a chip in the status bar rather than a
    /// dialog, so learning about a new version never steals focus from
    /// the arrangement or interrupts playback. Both halves are
    /// one-click terminal: open the page, or dismiss.
    fn view_update_notice(&self, version: &str) -> Element<'_, Message> {
        let open = button(
            row![
                icons::icon(icons::CIRCLE_DOT).size(9).color(th::accent()),
                text(format!("Version {version} available"))
                    .size(11)
                    .color(th::accent()),
            ]
            .spacing(5)
            .align_y(iced::Alignment::Center),
        )
        .on_press(Message::OpenReleasesPage)
        .padding([1, 8])
        .style(|_theme: &Theme, status| {
            let bg = match status {
                button::Status::Hovered | button::Status::Pressed => Some(th::bg_hover().into()),
                _ => None,
            };
            button::Style {
                background: bg,
                text_color: th::accent(),
                border: iced::Border {
                    color: th::accent_dim(),
                    width: 1.0,
                    radius: 3.0.into(),
                },
                ..Default::default()
            }
        });

        let dismiss = button(icons::icon(icons::X).size(10).color(th::text_dim()))
            .on_press(Message::DismissUpdateNotice)
            .padding([1, 4])
            .style(|_theme: &Theme, status| {
                let text_color = match status {
                    button::Status::Hovered | button::Status::Pressed => th::text(),
                    _ => th::text_dim(),
                };
                button::Style {
                    background: None,
                    text_color,
                    border: iced::Border::default(),
                    ..Default::default()
                }
            });

        row![open, dismiss]
            .spacing(2)
            .align_y(iced::Alignment::Center)
            .into()
    }

    /// Persistent project-save state in the bottom bar. Unlike transient
    /// status text, this remains visible after another operation reports
    /// its own result.
    fn view_project_save_status(&self) -> Element<'_, Message> {
        let (icon, label, color) = if self.save_runtime.is_saving() {
            (icons::CIRCLE_DOT, "Saving...", th::accent())
        } else if self.state.project.dirty {
            (icons::CIRCLE, "Unsaved", th::text_dim())
        } else if self.state.project.current_path.is_some() {
            (icons::CIRCLE_DOT, "Saved", th::accent())
        } else {
            (icons::CIRCLE, "Not saved", th::text_dim())
        };

        row![
            icons::icon(icon).size(9).color(color),
            text(label).size(STATUS_TEXT_SIZE).color(th::text_muted())
        ]
        .spacing(5)
        .align_y(iced::Alignment::Center)
        .into()
    }

    pub(super) fn view_status(&self) -> Element<'_, Message> {
        let quiet = |label: String| -> Element<'_, Message> {
            text(label)
                .size(STATUS_TEXT_SIZE)
                .color(th::text_muted())
                .wrapping(text::Wrapping::None)
                .into()
        };
        let mut content = row![
            text(&self.state.status_text)
                .size(STATUS_TEXT_SIZE)
                .color(th::text_dim()),
            horizontal_space(),
        ]
        .spacing(STATUS_SPACING)
        .align_y(iced::Alignment::Center);
        content = match self.state.export_progress {
            Some(percent) => content.push(
                row![
                    progress_bar(0.0..=100.0, percent as f32)
                        .width(180)
                        .height(5),
                    text(format!("{percent}%"))
                        .size(STATUS_TEXT_SIZE)
                        .color(th::text()),
                ]
                .spacing(8)
                .align_y(iced::Alignment::Center),
            ),
            // An export in flight owns the bar; a release notice can wait
            // for the next frame that has room.
            None => content.push_maybe(
                self.state
                    .update_check
                    .notice()
                    .map(|version| self.view_update_notice(version)),
            ),
        };
        let project_file = self
            .state
            .project
            .current_path
            .as_ref()
            .and_then(|path| path.file_name())
            .map(|name| quiet(name.to_string_lossy().into_owned()));
        let content = content
            .push(quiet(format!("Theme: {}", self.state.current_theme_name)))
            .push(self.view_project_save_status())
            .push(quiet(format!("Vibez {}", env!("CARGO_PKG_VERSION"))))
            .push_maybe(project_file);

        container(content)
            .width(Length::Fill)
            .height(Length::Fixed(crate::shell_layout::STATUS_HEIGHT))
            .padding([0, 16])
            .align_y(iced::alignment::Vertical::Center)
            .into()
    }
}
