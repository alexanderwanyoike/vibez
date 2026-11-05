//! The pieces every Settings section is built from: cards grouping rows
//! that carry their label and hint on the left and their control on the
//! right, and the controls that sit in them.

use std::borrow::Borrow;

use iced::widget::{button, column, container, pick_list, row, slider, text, Space};
use iced::{Alignment, Element, Font, Length, Theme};

use crate::message::Message;
use crate::theme as th;
use crate::widgets::control_style;

const LABEL_SIZE: f32 = 12.5;
const HINT_SIZE: f32 = 11.5;
const CONTROL_TEXT_SIZE: f32 = 11.5;
const GROUP_RADIUS: f32 = 10.0;
const CONTROL_RADIUS: f32 = 7.0;
const SWITCH_W: f32 = 34.0;
const SWITCH_H: f32 = 20.0;
const SWITCH_KNOB: f32 = 14.0;
/// The width a slider row's track and value take together.
pub(super) const SLIDER_W: f32 = 260.0;

const LABEL_FONT: Font = Font {
    weight: iced::font::Weight::Semibold,
    ..Font::DEFAULT
};

/// A one-line explanation under a label or a section title.
pub(super) fn hint<'a>(content: impl text::IntoFragment<'a>) -> text::Text<'a, Theme> {
    text(content).size(HINT_SIZE).color(th::text_muted())
}

/// A row: `label` and its `hint` on the left, `control` on the right.
pub(super) fn setting_row<'a>(
    label: impl text::IntoFragment<'a>,
    hint_text: Option<Element<'a, Message>>,
    control: impl Into<Element<'a, Message>>,
) -> Element<'a, Message> {
    row![label_block(label, hint_text), control.into()]
        .spacing(20)
        .align_y(Alignment::Center)
        .into()
}

/// A label with its hint beneath, filling the room a row leaves.
pub(super) fn label_block<'a>(
    label: impl text::IntoFragment<'a>,
    hint_text: Option<Element<'a, Message>>,
) -> Element<'a, Message> {
    let mut block = column![text(label)
        .size(LABEL_SIZE)
        .font(LABEL_FONT)
        .color(th::text())]
    .spacing(2)
    .width(Length::Fill);
    if let Some(hint_text) = hint_text {
        block = block.push(hint_text);
    }
    block.into()
}

/// A card of `rows` with a hairline between each.
pub(super) fn group<'a>(
    rows: impl IntoIterator<Item = Element<'a, Message>>,
) -> Element<'a, Message> {
    let mut content = column![].spacing(14);
    for (index, item) in rows.into_iter().enumerate() {
        if index > 0 {
            content = content.push(divider());
        }
        content = content.push(item);
    }
    container(content)
        .padding(16)
        .width(Length::Fill)
        .style(|_theme: &Theme| container::Style {
            background: Some(th::bg_elevated().into()),
            border: iced::Border {
                color: th::hairline(th::bg_elevated()),
                width: 1.0,
                radius: GROUP_RADIUS.into(),
            },
            ..Default::default()
        })
        .into()
}

fn divider<'a>() -> Element<'a, Message> {
    container(Space::new(Length::Fill, 1.0))
        .style(|_theme: &Theme| container::Style {
            background: Some(th::hairline(th::bg_elevated()).into()),
            ..Default::default()
        })
        .into()
}

/// A filled button, lit in the accent when it is the row's main action.
pub(super) fn action<'a>(
    label: impl text::IntoFragment<'a>,
    message: Option<Message>,
    primary: bool,
) -> Element<'a, Message> {
    button(text(label).size(CONTROL_TEXT_SIZE))
        .on_press_maybe(message)
        .padding([6, 11])
        .style(move |_theme: &Theme, status| button::Style {
            border: iced::Border {
                radius: CONTROL_RADIUS.into(),
                ..Default::default()
            },
            ..control_style::styled(primary.then(th::accent), status)
        })
        .into()
}

/// An on and off switch: a pill with its knob at the lit end while on.
pub(super) fn switch<'a>(on: bool, message: Message) -> Element<'a, Message> {
    let knob = container(Space::new(SWITCH_KNOB, SWITCH_KNOB)).style(move |_theme: &Theme| {
        container::Style {
            background: Some(if on { th::bg_dark() } else { th::text_dim() }.into()),
            border: iced::Border {
                radius: (SWITCH_KNOB / 2.0).into(),
                ..Default::default()
            },
            ..Default::default()
        }
    });
    let track = container(knob)
        .width(SWITCH_W)
        .height(SWITCH_H)
        .padding(3)
        .align_x(if on {
            iced::alignment::Horizontal::Right
        } else {
            iced::alignment::Horizontal::Left
        })
        .align_y(Alignment::Center);
    button(track)
        .on_press(message)
        .padding(0)
        .style(move |_theme: &Theme, status| button::Style {
            background: Some(
                if on {
                    th::accent()
                } else {
                    control_style::fill(None, status)
                }
                .into(),
            ),
            border: iced::Border {
                radius: (SWITCH_H / 2.0).into(),
                ..Default::default()
            },
            ..Default::default()
        })
        .into()
}

/// A filled pick list `width` wide.
pub(super) fn pick<'a, T>(
    options: impl Borrow<[T]> + 'a,
    selected: Option<T>,
    on_selected: impl Fn(T) -> Message + 'a,
    width: f32,
) -> Element<'a, Message>
where
    T: ToString + PartialEq + Clone + 'a,
{
    pick_list(options, selected, on_selected)
        .placeholder("No device")
        .width(Length::Fixed(width))
        .padding([6, 10])
        .text_size(CONTROL_TEXT_SIZE)
        .style(move |theme: &Theme, status| pick_list::Style {
            border: iced::Border {
                radius: CONTROL_RADIUS.into(),
                ..Default::default()
            },
            ..control_style::list(th::accent())(theme, status)
        })
        .menu_style(|_theme: &Theme| iced::widget::overlay::menu::Style {
            background: th::bg_elevated().into(),
            border: iced::Border {
                color: th::hairline(th::bg_elevated()),
                width: 1.0,
                radius: CONTROL_RADIUS.into(),
            },
            text_color: th::text(),
            selected_text_color: th::accent(),
            selected_background: th::bg_hover().into(),
        })
        .into()
}

/// A slider over `range` with its value read out beside it.
pub(super) fn slider_control<'a>(
    range: std::ops::RangeInclusive<f32>,
    value: f32,
    step: f32,
    on_change: impl Fn(f32) -> Message + 'a,
    readout: String,
) -> Element<'a, Message> {
    row![
        slider(range, value, on_change)
            .step(step)
            .width(Length::Fill)
            .style(slider_style),
        text(readout)
            .size(CONTROL_TEXT_SIZE)
            .font(crate::typography::PERFORM_TECH)
            .color(th::text())
            .width(Length::Fixed(52.0))
            .align_x(iced::alignment::Horizontal::Right),
    ]
    .spacing(10)
    .align_y(Alignment::Center)
    .width(Length::Fixed(SLIDER_W))
    .into()
}

fn slider_style(_theme: &Theme, status: slider::Status) -> slider::Style {
    slider::Style {
        rail: slider::Rail {
            backgrounds: (th::accent().into(), th::control().into()),
            width: 4.0,
            border: iced::Border {
                radius: 2.0.into(),
                ..Default::default()
            },
        },
        handle: slider::Handle {
            shape: slider::HandleShape::Circle { radius: 7.0 },
            background: if matches!(status, slider::Status::Dragged) {
                th::accent().into()
            } else {
                th::text().into()
            },
            border_width: 0.0,
            border_color: iced::Color::TRANSPARENT,
        },
    }
}

/// A line of status under a group, quiet unless it reports a problem.
pub(super) fn status<'a>(
    content: impl text::IntoFragment<'a>,
    problem: bool,
) -> Element<'a, Message> {
    text(content)
        .size(HINT_SIZE)
        .color(if problem {
            th::danger()
        } else {
            th::text_dim()
        })
        .into()
}
