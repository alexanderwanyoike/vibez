use iced::widget::{button, column, container, row, text, Space};
use iced::{Color, Element, Length, Theme};

use crate::icons;
use crate::message::Message;
use crate::state::UiEffect;
use crate::theme as th;
use crate::widgets::effect_knob::{param_column, EffectKnobWidget};
use vibez_core::id::TrackId;
use vibez_plugin_host::gui::PluginGuiKey;

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

    let name_elem = text(display_name).size(11).color(name_color);

    // Name takes remaining width
    let name_section = container(name_elem).width(Length::Fill);

    // Fixed-size controls on the right
    // Edit button (open plugin GUI) — only for effects with a native GUI
    let make_edit = || -> Option<iced::widget::Button<'a, Message>> {
        if !has_gui {
            return None;
        }
        let gui_key = PluginGuiKey::Effect {
            track_id,
            effect_id: effect.id,
        };
        Some(
            button(text("Edit").size(9).color(th::text_dim()))
                .on_press(Message::OpenPluginGui(gui_key))
                .padding([2, 5])
                .style(|_theme: &Theme, status| {
                    let (bg, tc) = match status {
                        button::Status::Hovered => (Some(th::bg_hover().into()), th::accent()),
                        _ => (None, th::text_dim()),
                    };
                    button::Style {
                        background: bg,
                        text_color: tc,
                        border: iced::Border {
                            color: th::border(),
                            width: 1.0,
                            radius: 3.0.into(),
                        },
                        ..Default::default()
                    }
                }),
        )
    };

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

    let mut title_row = row![dot, name_section]
        .spacing(3)
        .align_y(iced::Alignment::Center);
    if is_plugin {
        // Plugins: name gets the title to itself; Edit/On/reorder
        // live in the body where they have room.
        title_row = title_row.push(remove);
    } else {
        if let Some(eb) = make_edit() {
            title_row = title_row.push(eb);
        }
        title_row = title_row
            .push(make_bypass())
            .push(make_move_up())
            .push(make_move_down())
            .push(remove);
    }

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
        // External plugin face: format badge, prominent Edit, then
        // bypass and chain-reorder controls.
        let format_label = effect
            .plugin_ref
            .as_ref()
            .map(|d| d.format.to_uppercase())
            .unwrap_or_else(|| "PLUGIN".to_string());
        let badge = container(text(format_label).size(8).color(th::accent()))
            .padding([2, 8])
            .style(|_theme: &Theme| container::Style {
                background: Some(th::bg_dark().into()),
                border: iced::Border {
                    color: th::accent_dim(),
                    width: 1.0,
                    radius: 3.0.into(),
                },
                ..Default::default()
            });

        let mut face = column![badge].spacing(10).align_x(iced::Alignment::Center);
        if let Some(eb) = make_edit() {
            face = face.push(eb.padding([5, 22]));
        }
        face = face.push(
            row![make_bypass(), make_move_up(), make_move_down()]
                .spacing(4)
                .align_y(iced::Alignment::Center),
        );

        container(face)
            .width(Length::Fill)
            .height(Length::Fixed(th::DEVICE_BODY_H))
            .align_x(iced::Alignment::Center)
            .align_y(iced::Alignment::Center)
            .into()
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
        190.0
    } else {
        (knob_count as f32 * 62.0 + 24.0).max(150.0)
    };
    let card = column![title_bar, body].width(Length::Fixed(card_w));

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
