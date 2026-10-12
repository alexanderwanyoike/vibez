use iced::widget::{button, column, container, row, text, Space};
use iced::{Color, Element, Length, Theme};

use crate::icons;
use crate::message::Message;
use crate::state::UiEffect;
use crate::theme as th;
use crate::widgets::effect_knob::{param_column, EffectKnobWidget};
use vibez_core::id::TrackId;
use vibez_plugin_host::gui::PluginGuiKey;

#[path = "sidechain.rs"]
mod sidechain;
pub use sidechain::view as view_sidechain;

/// Render an Ableton-style device card for the detail panel.
///
/// `custom_body` swaps the generic knob-row body for a purpose-built
/// one (the channel EQ's display panel) while keeping the shared
/// title-bar chrome; the second element is the card width.
pub fn view_effect_slot<'a>(
    track_id: TrackId,
    effect: &'a UiEffect,
    track_color: Color,
    custom_body: Option<(Element<'a, Message>, f32)>,
    sidechain: Option<Element<'a, Message>>,
    sample_rate: u32,
) -> Element<'a, Message> {
    let is_bypassed = effect.bypass;
    let has_params = !effect.descriptors.is_empty();
    let has_gui = effect.has_plugin_gui;
    let is_plugin = effect.plugin_name.is_some();

    let dot_color = if is_bypassed {
        th::text_muted()
    } else {
        track_color
    };

    // ── Title bar: [●] Name …          [On] [▲] [▼] [×] ──
    let dot = button(text("\u{25CF}").size(9).color(dot_color))
        .on_press(Message::toggle_effect_bypass(track_id, effect.id))
        .padding([2, 3])
        .style(move |_theme: &Theme, status| {
            let bg = match status {
                button::Status::Hovered => Some(th::bg_hover().into()),
                _ => None,
            };
            button::Style {
                background: bg,
                text_color: dot_color,
                border: iced::Border {
                    radius: 2.0.into(),
                    ..Default::default()
                },
                ..Default::default()
            }
        });

    let display_name = effect
        .plugin_name
        .as_deref()
        .unwrap_or_else(|| effect.effect_type.name());
    let name_color = if is_bypassed {
        th::text_dim()
    } else {
        th::text()
    };

    let name_section = crate::widgets::device_chrome::title_name(display_name, name_color);

    let bypass_label = if is_bypassed { "Off" } else { "On" };
    let bypass_color = if is_bypassed {
        th::text_muted()
    } else {
        th::success()
    };
    let make_bypass = move || {
        button(text(bypass_label).size(9).color(bypass_color))
            .on_press(Message::toggle_effect_bypass(track_id, effect.id))
            .padding([2, 5])
            .style(move |_theme: &Theme, status| {
                let bg = match status {
                    button::Status::Hovered => Some(th::bg_hover().into()),
                    _ => None,
                };
                button::Style {
                    background: bg,
                    text_color: bypass_color,
                    border: iced::Border {
                        color: if is_bypassed {
                            th::border()
                        } else {
                            th::darken(th::success(), 0.5)
                        },
                        width: 1.0,
                        radius: 3.0.into(),
                    },
                    ..Default::default()
                }
            })
    };

    let make_move_up = || -> Element<'a, Message> {
        action_btn(
            icons::CHEVRON_UP,
            th::text_dim(),
            th::text(),
            Message::move_effect_up(track_id, effect.id),
        )
        .into()
    };
    let make_move_down = || -> Element<'a, Message> {
        action_btn(
            icons::CHEVRON_DOWN,
            th::text_dim(),
            th::text(),
            Message::move_effect_down(track_id, effect.id),
        )
        .into()
    };
    let remove: Element<'a, Message> = action_btn(
        icons::X,
        th::text_dim(),
        th::danger(),
        Message::remove_effect(track_id, effect.id),
    )
    .into();

    let title_row = row![
        dot,
        name_section,
        make_bypass(),
        make_move_up(),
        make_move_down(),
        remove
    ]
    .spacing(3)
    .align_y(iced::Alignment::Center);

    let title_bar = container(title_row)
        .padding([4, 6])
        .width(Length::Fill)
        .style(|_theme: &Theme| container::Style {
            background: Some(th::bg_surface().into()),
            ..Default::default()
        });

    // ── Body ─────────────────────────────────────────────────
    let custom_w = custom_body.as_ref().map(|(_, w)| *w);
    let body: Element<'a, Message> = if let Some((custom, _)) = custom_body {
        container(custom)
            .padding([8, 10])
            .height(Length::Fixed(th::DEVICE_BODY_H))
            .into()
    } else if has_params && !has_gui {
        let knob_color = if is_bypassed {
            th::text_muted()
        } else {
            track_color
        };
        // One knob row in the shared column format: the devices
        // panel scrolls horizontally and stacked rows clip.
        let mut knob_row = row![].spacing(6);
        for (i, descriptor) in effect.descriptors.iter().enumerate() {
            let value = effect.params.get(i).copied().unwrap_or(descriptor.default);
            let knob = EffectKnobWidget::new(
                track_id,
                effect.id,
                i,
                value,
                descriptor.min,
                descriptor.max,
                descriptor.default,
                knob_color,
            );
            knob_row = knob_row.push(param_column(
                knob,
                descriptor.name.to_string(),
                crate::widgets::effect_knob::format_value(value, descriptor.unit),
            ));
        }

        container(knob_row)
            .padding([8, 10])
            .height(Length::Fixed(th::DEVICE_BODY_H))
            .into()
    } else if is_plugin {
        crate::widgets::device_chrome::plugin_face(
            effect
                .plugin_ref
                .as_ref()
                .map(|device| device.format.as_str()),
            has_gui.then_some(PluginGuiKey::Effect {
                track_id,
                effect_id: effect.id,
            }),
        )
    } else {
        container(Space::new(Length::Fixed(120.0), Length::Fixed(2.0)))
            .height(Length::Fixed(th::DEVICE_BODY_H))
            .into()
    };

    // ── Card ─────────────────────────────────────────────────
    // Fixed width computed from the knob count: a Fill title strip
    // inside a shrink column collapses the card chrome in iced.
    let knob_count = if has_params && !has_gui {
        effect.descriptors.len()
    } else {
        0
    };
    let card_w = if let Some(w) = custom_w {
        w
    } else if is_plugin {
        crate::widgets::device_chrome::PLUGIN_CARD_W
    } else {
        (knob_count as f32 * 62.0 + 24.0).max(150.0)
    };
    // Routing reads left to right: what keys the device, then the device.
    let (body, card_w): (Element<'a, Message>, f32) = match sidechain {
        Some(sidechain) => (
            row![
                sidechain,
                crate::widgets::device_chrome::divider(),
                container(body).width(Length::Fixed(card_w))
            ]
            .align_y(iced::Alignment::Center)
            .into(),
            card_w + sidechain::WIDTH + 1.0,
        ),
        None => (body, card_w),
    };
    let latency =
        crate::domains::device_latency::latency_label(effect.latency_samples, sample_rate);
    let card = column![
        title_bar,
        container(text(latency).size(9).color(th::text_dim())).padding([2, 6]),
        body
    ]
    .width(Length::Fixed(card_w));

    container(card)
        .style(|_theme: &Theme| container::Style {
            background: Some(th::bg_elevated().into()),
            border: iced::Border {
                color: th::border(),
                width: 1.0,
                radius: 4.0.into(),
            },
            ..Default::default()
        })
        .into()
}

/// Device card action button.
fn action_btn(
    icon_char: char,
    color: Color,
    hover_color: Color,
    msg: Message,
) -> iced::widget::Button<'static, Message> {
    button(icons::icon(icon_char).size(12).color(color))
        .on_press(msg)
        .padding([3, 4])
        .style(move |_theme: &Theme, status| {
            let (bg, tc) = match status {
                button::Status::Hovered => (Some(th::bg_hover().into()), hover_color),
                button::Status::Pressed => (Some(th::bg_dark().into()), hover_color),
                _ => (None, color),
            };
            button::Style {
                background: bg,
                text_color: tc,
                border: iced::Border {
                    radius: 3.0.into(),
                    ..Default::default()
                },
                ..Default::default()
            }
        })
}
