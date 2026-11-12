//! The soft tint that marks where a Browser drop will land.

use iced::widget::{container, Space};
use iced::{Element, Length, Padding, Theme};

use crate::theme as th;
use crate::widgets::flat_stack::flat_stack;

const OUTLINE_W: f32 = 1.0;

/// `content` under a faint accent wash and hairline while `active`, rounded
/// to `radius` and held `inset` inside the content's edges so it never
/// covers chrome such as a scrollbar. The tint layer stays when inactive, so
/// a hover never rebuilds the content's widget state, and it sits on top so
/// cards cannot paint over it.
pub fn drop_highlight<'a, Message: 'a>(
    content: Element<'a, Message>,
    active: bool,
    radius: f32,
    inset: impl Into<Padding>,
) -> Element<'a, Message> {
    let size = content.as_widget().size_hint();
    let tint = container(Space::new(Length::Fill, Length::Fill))
        .width(Length::Fill)
        .height(Length::Fill)
        .style(move |_theme: &Theme| {
            if !active {
                return container::Style::default();
            }
            container::Style {
                background: Some(th::drop_target_wash().into()),
                border: iced::Border {
                    color: th::drop_target_outline(),
                    width: OUTLINE_W,
                    radius: radius.into(),
                },
                ..Default::default()
            }
        });
    let tint = container(tint)
        .width(Length::Fill)
        .height(Length::Fill)
        .padding(inset);
    flat_stack([content, tint.into()])
        .width(size.width)
        .height(size.height)
        .into()
}
