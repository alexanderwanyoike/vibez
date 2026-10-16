//! Controls every built-in device body is built from: parameter knobs with
//! a readable value you can click to type into, segmented choices, and
//! headings. Sizes are in logical pixels, which Interface Scale multiplies.

use iced::widget::{button, canvas, column, container, pick_list, row, text, text_input, Space};
use iced::{Alignment, Color, Element, Length, Theme};
use vibez_core::id::TrackId;

use crate::domains::device_params::{DeviceParams, DeviceSlot};
use crate::domains::param_entry::{param_format, EntryMsg, ValueEntry};
use crate::message::Message;
use crate::theme as th;
use crate::widgets::effect_knob::{EffectKnobWidget, KnobTarget};
use crate::widgets::on_blur::on_blur;

pub const HEADING_SIZE: f32 = 10.0;
pub const LABEL_SIZE: f32 = 10.0;
pub const VALUE_SIZE: f32 = 11.0;
/// Height of a knob's label and value lines together, for layouts that
/// size the knob from the space left.
pub const KNOB_TEXT_H: f32 = 34.0;
pub const KNOB_MIN: f32 = 28.0;
pub const KNOB_MAX: f32 = 56.0;
/// The knob beside a value on a single-line row.
pub const SMALL_KNOB: f32 = 18.0;
/// A level meter with its scale, under a heading.
pub const METER_W: f32 = 58.0;

/// Only one value is typed into at a time, so every entry shares this id.
pub fn entry_id() -> text_input::Id {
    text_input::Id::new("device-value-entry")
}

/// Knob diameter for a control column `height` tall.
pub fn knob_size(height: f32) -> f32 {
    (height - KNOB_TEXT_H).clamp(KNOB_MIN, KNOB_MAX)
}

/// A group heading inside a device body, such as "Input" or "Output (dB)".
pub fn heading<'a>(label: impl text::IntoFragment<'a>) -> text::Text<'a, Theme> {
    text(label).size(HEADING_SIZE).color(th::text_dim())
}

/// One built-in effect parameter, as the device body shows it.
#[derive(Clone, Copy)]
pub struct Param<'a> {
    pub track_id: TrackId,
    pub device: DeviceParams<'a>,
    pub index: usize,
    pub accent: Color,
    pub entry: Option<&'a ValueEntry>,
    target: Option<KnobTarget>,
    label: Option<&'a str>,
    host_rate: Option<u32>,
}

impl<'a> Param<'a> {
    pub fn new(
        track_id: TrackId,
        device: impl Into<DeviceParams<'a>>,
        index: usize,
        accent: Color,
        entry: Option<&'a ValueEntry>,
    ) -> Self {
        Self {
            track_id,
            device: device.into(),
            index,
            accent,
            entry,
            target: None,
            label: None,
            host_rate: None,
        }
    }

    /// The sample rate the device runs at, for values that read as a
    /// share of it.
    pub fn host_rate(mut self, rate: u32) -> Self {
        self.host_rate = Some(rate);
        self
    }

    /// Replaces the knob's linear travel, as Gain does for its dB curve.
    pub fn knob_target(mut self, target: KnobTarget) -> Self {
        self.target = Some(target);
        self
    }

    /// Replaces the descriptor's name where the device says it shorter.
    pub fn label(mut self, label: &'a str) -> Self {
        self.label = Some(label);
        self
    }

    fn value(&self) -> f32 {
        self.device.value(self.index)
    }

    fn descriptor(&self) -> Option<&'static vibez_core::effect::ParamDescriptor> {
        self.device.descriptors.get(self.index)
    }

    pub fn knob(&self) -> EffectKnobWidget {
        let (min, max, default) = self
            .descriptor()
            .map_or((0.0, 1.0, 0.0), |d| (d.min, d.max, d.default));
        let mut knob = match self.device.slot {
            DeviceSlot::Effect(effect_id) => EffectKnobWidget::new(
                self.track_id,
                effect_id,
                self.index,
                self.value(),
                min,
                max,
                default,
                self.accent,
            ),
            DeviceSlot::Instrument => EffectKnobWidget::for_instrument(
                self.track_id,
                self.index,
                self.value(),
                min,
                max,
                default,
                self.accent,
            ),
        };
        if let Some(target) = self.target {
            knob.target = target;
        }
        knob
    }

    pub fn name(&self) -> &'a str {
        self.label
            .or_else(|| self.descriptor().map(|d| d.name))
            .unwrap_or("")
    }

    /// The value with its unit. Clicking it starts typing a new one.
    pub fn value_entry(&self) -> Element<'a, Message> {
        let Some(descriptor) = self.descriptor() else {
            return text("").into();
        };
        let format = param_format(self.device.kind, self.index, descriptor, self.host_rate);
        let accent = self.accent;
        if let Some(entry) = self
            .entry
            .filter(|entry| entry.is_for(self.device.slot, self.index))
        {
            let invalid = entry.invalid;
            let field = text_input("", &entry.text)
                .id(entry_id())
                .on_input(|text| Message::ParamEntry(EntryMsg::Edit(text)))
                .on_submit(Message::ParamEntry(EntryMsg::Submit))
                .size(VALUE_SIZE)
                .font(crate::typography::PERFORM_TECH)
                .padding([0, 3])
                .width(Length::Fixed(64.0))
                .align_x(Alignment::Center)
                .style(move |_theme: &Theme, _status| text_input::Style {
                    background: th::bg_dark().into(),
                    border: iced::Border {
                        color: if invalid { th::danger() } else { accent },
                        width: 1.0,
                        radius: 2.0.into(),
                    },
                    icon: th::text_dim(),
                    placeholder: th::text_muted(),
                    value: th::text(),
                    selection: th::with_alpha(accent, 0.35),
                });
            return on_blur(field, true, Message::ParamEntry(EntryMsg::Submit)).into();
        }
        let begin = Message::ParamEntry(EntryMsg::Begin {
            track_id: self.track_id,
            slot: self.device.slot,
            index: self.index,
            text: format.edit_text(self.value()),
        });
        button(
            text(format.format(self.value()))
                .size(VALUE_SIZE)
                .font(crate::typography::PERFORM_TECH)
                .wrapping(text::Wrapping::None),
        )
        .on_press(begin)
        .padding([0, 2])
        .style(move |_theme: &Theme, status| button::Style {
            background: None,
            text_color: if matches!(status, button::Status::Hovered | button::Status::Pressed) {
                accent
            } else {
                th::text()
            },
            ..Default::default()
        })
        .into()
    }

    /// The value with a small knob beside it.
    pub fn value_knob(&self) -> Element<'a, Message> {
        row![
            self.value_entry(),
            canvas(self.knob()).width(SMALL_KNOB).height(SMALL_KNOB),
        ]
        .spacing(2)
        .align_y(Alignment::Center)
        .into()
    }

    /// Name, value and small knob on one line, for the shortest cards.
    pub fn line_item(&self) -> Element<'a, Message> {
        row![
            text(self.name())
                .size(LABEL_SIZE)
                .color(th::text_dim())
                .wrapping(text::Wrapping::None),
            self.value_knob(),
        ]
        .spacing(4)
        .align_y(Alignment::Center)
        .into()
    }

    /// A switch named `label`, lit while this parameter is on.
    pub fn switch(&self, label: &'a str) -> Element<'a, Message> {
        let on = self.value() >= 0.5;
        toggle(
            text(label)
                .size(LABEL_SIZE)
                .wrapping(text::Wrapping::None)
                .into(),
            on,
            self.device
                .slot
                .set(self.track_id, self.index, if on { 0.0 } else { 1.0 }),
            self.accent,
        )
    }

    /// A dropdown of this parameter's choices, `names[first..]`, each set as
    /// its index.
    pub fn choices(&self, names: &'static [&'static str], first: usize) -> Element<'a, Message> {
        let (track_id, slot, index) = (self.track_id, self.device.slot, self.index);
        let options: Vec<Named> = (first..names.len()).map(|n| Named(n, names[n])).collect();
        let selected = chosen(self.value(), first, names.len());
        dropdown(options, Named(selected, names[selected]), move |named| {
            slot.set(track_id, index, named.0 as f32)
        })
    }

    /// Knob, name and value stacked, as most device controls read.
    pub fn column(&self, knob_size: f32) -> Element<'a, Message> {
        column![
            canvas(self.knob()).width(knob_size).height(knob_size),
            text(self.name())
                .size(LABEL_SIZE)
                .color(th::text_dim())
                .wrapping(text::Wrapping::None),
            self.value_entry(),
        ]
        .spacing(2)
        .align_x(Alignment::Center)
        .into()
    }

    /// Name on the left, value and knob on the right, for narrow panels
    /// that list one parameter per line.
    pub fn row(&self, knob_size: f32) -> Element<'a, Message> {
        row![
            text(self.name())
                .size(LABEL_SIZE)
                .color(th::text_dim())
                .wrapping(text::Wrapping::None),
            Space::with_width(Length::Fill),
            self.value_entry(),
            canvas(self.knob()).width(knob_size).height(knob_size),
        ]
        .spacing(6)
        .align_y(Alignment::Center)
        .into()
    }
}

/// A level meter under its heading.
/// A choice and its name, for a dropdown.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Named(usize, &'static str);

impl std::fmt::Display for Named {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.1)
    }
}

/// The choice a stored value names: the nearest, within `first..count`.
fn chosen(value: f32, first: usize, count: usize) -> usize {
    (value.round().max(0.0) as usize).clamp(first, count - 1)
}

pub fn level_column<'a>(
    name: &'a str,
    levels: vibez_engine::metering::MeterValues,
) -> Element<'a, Message> {
    column![
        heading(name),
        canvas(crate::widgets::level_meter::LevelMeter { levels })
            .width(crate::widgets::level_meter::WIDTH)
            .height(Length::Fill),
    ]
    .spacing(2)
    .width(METER_W)
    .into()
}

/// A small button that is lit with the track accent while `on`.
pub fn toggle<'a>(
    label: Element<'a, Message>,
    on: bool,
    message: Message,
    accent: Color,
) -> Element<'a, Message> {
    button(label)
        .on_press(message)
        .padding([2, 6])
        .style(move |_theme: &Theme, status| {
            let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
            button::Style {
                background: Some(if hovered {
                    th::bg_hover().into()
                } else {
                    th::bg_dark().into()
                }),
                text_color: if on { accent } else { th::text_dim() },
                border: iced::Border {
                    color: if on { accent } else { th::border() },
                    width: 1.0,
                    radius: 3.0.into(),
                },
                ..Default::default()
            }
        })
        .into()
}

/// A row of mutually exclusive choices, the chosen one filled with the
/// track accent.
pub fn segmented<'a, T: Copy + PartialEq + 'a>(
    options: &[(T, &'a str)],
    selected: T,
    on_select: impl Fn(T) -> Message,
    accent: Color,
) -> Element<'a, Message> {
    let count = options.len();
    let mut choices = row![].spacing(0);
    for (position, (value, label)) in options.iter().copied().enumerate() {
        let chosen = value == selected;
        let radius = iced::border::Radius {
            top_left: if position == 0 { 3.0 } else { 0.0 },
            bottom_left: if position == 0 { 3.0 } else { 0.0 },
            top_right: if position + 1 == count { 3.0 } else { 0.0 },
            bottom_right: if position + 1 == count { 3.0 } else { 0.0 },
        };
        choices = choices.push(
            button(text(label).size(LABEL_SIZE).wrapping(text::Wrapping::None))
                .on_press(on_select(value))
                .padding([3, 10])
                .style(move |_theme: &Theme, status| {
                    let hovered =
                        matches!(status, button::Status::Hovered | button::Status::Pressed);
                    button::Style {
                        background: Some(if chosen {
                            accent.into()
                        } else if hovered {
                            th::bg_hover().into()
                        } else {
                            th::bg_dark().into()
                        }),
                        text_color: if chosen { th::bg_dark() } else { th::text() },
                        border: iced::Border {
                            color: if chosen { accent } else { th::border() },
                            width: 1.0,
                            radius,
                        },
                        ..Default::default()
                    }
                }),
        );
    }
    choices.into()
}

/// A compact choice from a list, for options too many for [`segmented`].
pub fn dropdown<'a, T: Clone + ToString + PartialEq + 'a>(
    options: Vec<T>,
    selected: T,
    on_select: impl Fn(T) -> Message + 'a,
) -> Element<'a, Message> {
    pick_list(options, Some(selected), on_select)
        .width(Length::Fill)
        .padding([3, 5])
        .text_size(9)
        .style(|_theme: &Theme, status| pick_list::Style {
            text_color: th::text(),
            placeholder_color: th::text_muted(),
            handle_color: th::text_dim(),
            background: th::bg_dark().into(),
            border: iced::Border {
                color: if matches!(
                    status,
                    pick_list::Status::Opened | pick_list::Status::Hovered
                ) {
                    th::accent_dim()
                } else {
                    th::border()
                },
                width: 1.0,
                radius: 3.0.into(),
            },
        })
        .menu_style(|_theme: &Theme| iced::widget::overlay::menu::Style {
            background: th::bg_surface().into(),
            border: iced::Border {
                color: th::border(),
                width: 1.0,
                radius: 3.0.into(),
            },
            text_color: th::text(),
            selected_text_color: th::accent(),
            selected_background: th::bg_hover().into(),
        })
        .into()
}

/// The "Advanced" switch that opens a device's less used controls.
pub fn disclosure<'a>(open: bool, message: Message, accent: Color) -> Element<'a, Message> {
    toggle(
        row![
            text(if open { "\u{25BE}" } else { "\u{25B8}" })
                .size(9)
                .color(th::text_dim()),
            text("Advanced").size(LABEL_SIZE).color(th::text()),
        ]
        .spacing(4)
        .align_y(Alignment::Center)
        .into(),
        open,
        message,
        accent,
    )
}

/// A divider between the sections of a device body.
pub fn rule<'a>() -> Element<'a, Message> {
    container(Space::new(Length::Fill, 1.0))
        .style(|_| container::Style {
            background: Some(th::divider().into()),
            ..Default::default()
        })
        .into()
}

/// The dark band a device's plots and meters sit on, full width and
/// `height` tall, with `pad` either side.
pub fn plot_band<'a>(
    content: impl Into<Element<'a, Message>>,
    height: f32,
    pad: f32,
) -> Element<'a, Message> {
    container(content)
        .padding([6.0, pad])
        .height(height)
        .width(Length::Fill)
        .style(|_| container::Style {
            background: Some(th::bg_dark().into()),
            ..Default::default()
        })
        .into()
}

pub fn vertical_rule<'a>() -> Element<'a, Message> {
    container(Space::new(1.0, Length::Fill))
        .style(|_| container::Style {
            background: Some(th::divider().into()),
            ..Default::default()
        })
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stored_value_names_its_nearest_choice() {
        assert_eq!(chosen(2.0, 0, 5), 2);
        assert_eq!(chosen(0.4, 0, 5), 0);
        assert_eq!(chosen(0.6, 0, 5), 1);
        assert_eq!(chosen(9.0, 0, 5), 4);
        assert_eq!(chosen(0.0, 1, 5), 1);
    }

    #[test]
    fn knobs_grow_with_the_column_and_stay_aimable() {
        assert_eq!(knob_size(0.0), KNOB_MIN);
        assert_eq!(knob_size(KNOB_TEXT_H + 40.0), 40.0);
        assert_eq!(knob_size(400.0), KNOB_MAX);
    }
}
