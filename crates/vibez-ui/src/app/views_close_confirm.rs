//! The save/discard/cancel dialog raised when the window is closed with
//! unsaved edits. Its own file because `views_overlays.rs` is close to the
//! 1,000-line ceiling.

use iced::widget::{button, center, column, container, horizontal_space, row, text};
use iced::{Element, Length, Theme};

use crate::message::Message;
use crate::theme as th;

use super::window_policy::project_display_name;
use super::*;

impl App {
    pub(super) fn view_close_confirm_overlay(&self) -> Element<'_, Message> {
        let project_name = project_display_name(self.state.project.current_path.as_deref());
        let destination = match self.state.project.current_path.as_deref() {
            Some(path) => format!("Unsaved changes will be written to {}.", path.display()),
            None => "This project has never been saved. Saving will ask for a file.".to_string(),
        };

        let cancel = button(text("Cancel").size(12).color(th::text()))
            .on_press(Message::CloseConfirmCancel)
            .padding([7, 14]);
        // Discarding is the destructive path here, not closing, so it carries
        // the danger styling that Delete carries elsewhere.
        let discard = button(text("Discard").size(12).color(th::bg_dark()))
            .on_press(Message::CloseConfirmDiscard)
            .padding([7, 14])
            .style(|_theme: &Theme, status| button::Style {
                background: Some(
                    if matches!(status, button::Status::Hovered | button::Status::Pressed) {
                        th::blend(th::danger(), th::text(), 0.18)
                    } else {
                        th::danger()
                    }
                    .into(),
                ),
                text_color: th::bg_dark(),
                border: iced::Border::default(),
                ..Default::default()
            });
        let save = button(text("Save").size(12).color(th::bg_dark()))
            .on_press(Message::CloseConfirmSave)
            .padding([7, 14])
            .style(|_theme: &Theme, status| button::Style {
                background: Some(
                    if matches!(status, button::Status::Hovered | button::Status::Pressed) {
                        th::blend(th::accent(), th::text(), 0.18)
                    } else {
                        th::accent()
                    }
                    .into(),
                ),
                text_color: th::bg_dark(),
                border: iced::Border::default(),
                ..Default::default()
            });

        let card = container(
            column![
                text(format!("Save changes to {project_name}?"))
                    .font(crate::typography::PERFORM_DISPLAY)
                    .size(18)
                    .color(th::text()),
                text(destination).size(11).color(th::text_dim()),
                text("Discarding closes vibez and loses every edit since the last save.")
                    .size(10)
                    .color(th::text_dim()),
                row![horizontal_space(), cancel, discard, save].spacing(8),
            ]
            .spacing(12),
        )
        .padding(20)
        .width(Length::Fixed(440.0))
        .style(|_theme: &Theme| container::Style {
            background: Some(th::bg_surface().into()),
            border: iced::Border {
                color: th::border_light(),
                width: 1.0,
                radius: 4.0.into(),
            },
            ..Default::default()
        });

        container(center(card))
            .width(Length::Fill)
            .height(Length::Fill)
            .style(|_theme: &Theme| container::Style {
                background: Some(iced::Color::from_rgba(0.0, 0.0, 0.0, 0.62).into()),
                ..Default::default()
            })
            .into()
    }
}
