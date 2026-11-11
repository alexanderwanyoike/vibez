//! One line of text cut in the middle to fit the width it is laid out in.

use iced::widget::{container, responsive, text};
use iced::{Color, Element, Font, Length};

use crate::message::Message;
use crate::typography::fit_text_middle;

use super::hint::with_hint;

/// A label that keeps the start and end of `content` when its row is too
/// narrow. Names show the whole text on hover when cut; detail lines such
/// as paths do not, so one row never stacks two hints.
pub(crate) struct FittedLabel {
    content: String,
    size: f32,
    font: Font,
    color: Color,
    whole_on_hover: bool,
}

impl FittedLabel {
    pub(crate) fn name(content: impl Into<String>, size: f32, color: Color) -> Self {
        Self {
            content: content.into(),
            size,
            font: Font::DEFAULT,
            color,
            whole_on_hover: true,
        }
    }

    pub(crate) fn detail(content: impl Into<String>, size: f32, color: Color) -> Self {
        Self {
            whole_on_hover: false,
            ..Self::name(content, size, color)
        }
    }

    pub(crate) fn font(self, font: Font) -> Self {
        Self { font, ..self }
    }

    pub(crate) fn view<'a>(self, height: f32) -> Element<'a, Message> {
        container(responsive(move |bounds| {
            let fitted = fit_text_middle(&self.content, bounds.width, self.size, self.font);
            let cut = fitted != self.content;
            let label = text(fitted)
                .size(self.size)
                .font(self.font)
                .color(self.color)
                .shaping(text::Shaping::Advanced)
                .wrapping(text::Wrapping::None);
            if cut && self.whole_on_hover {
                with_hint(label, self.content.clone())
            } else {
                label.into()
            }
        }))
        .width(Length::Fill)
        .height(Length::Fixed(height))
        .into()
    }
}
