use iced::widget::{button, canvas, column, container, row, text, Space};
use iced::{Color, Element, Length, Theme};

use crate::icons;
use crate::message::Message;
use crate::state::UiEffect;
use crate::theme as th;
use crate::widgets::effect_knob::EffectKnobWidget;
use vibez_core::id::TrackId;
use vibez_plugin_host::gui::PluginGuiKey;

const CARD_WIDTH: f32 = 220.0;

/// Render an Ableton-style device card for the detail panel.
pub fn view_effect_slot<'a>(
    track_id: TrackId,
    effect: &'a UiEffect,
    track_color: Color,
) -> Element<'a, Message> {
    let is_bypassed = effect.bypass;
    let has_params = !effect.descriptors.is_empty();
    let has_gui = effect.has_plugin_gui;

    let dot_color = if is_bypassed {
        th::TEXT_MUTED
    } else {
        track_color
    };

    // ── Title bar: [●] Name …          [On] [▲] [▼] [×] ──
    let dot = button(text("\u{25CF}").size(9).color(dot_color))
        .on_press(Message::ToggleEffectBypass(track_id, effect.id))
        .padding([2, 3])
        .style(move |_theme: &Theme, status| {
            let bg = match status {
                button::Status::Hovered => Some(th::BG_HOVER.into()),
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
    let name_color = if is_bypassed { th::TEXT_DIM } else { th::TEXT };

    let name_elem = text(display_name).size(11).color(name_color);

    // Name takes remaining width
    let name_section = container(name_elem).width(Length::Fill);

    // Fixed-size controls on the right
    // Edit button (open plugin GUI) — only for effects with a native GUI
    let edit_btn: Option<iced::widget::Button<'_, Message>> = if has_gui {
        let gui_key = PluginGuiKey::Effect {
            track_id,
            effect_id: effect.id,
        };
        Some(
            button(text("Edit").size(9).color(th::TEXT_DIM))
                .on_press(Message::OpenPluginGui(gui_key))
                .padding([2, 5])
                .style(|_theme: &Theme, status| {
                    let (bg, tc) = match status {
                        button::Status::Hovered => (Some(th::BG_HOVER.into()), th::ACCENT),
                        _ => (None, th::TEXT_DIM),
                    };
                    button::Style {
                        background: bg,
                        text_color: tc,
                        border: iced::Border {
                            color: th::BORDER,
                            width: 1.0,
                            radius: 3.0.into(),
                        },
                        ..Default::default()
                    }
                }),
        )
    } else {
        None
    };

    let bypass_label = if is_bypassed { "Off" } else { "On" };
    let bypass_color = if is_bypassed {
        th::TEXT_MUTED
    } else {
        th::SUCCESS
    };
    let bypass_btn = button(text(bypass_label).size(9).color(bypass_color))
        .on_press(Message::ToggleEffectBypass(track_id, effect.id))
        .padding([2, 5])
        .style(move |_theme: &Theme, status| {
            let bg = match status {
                button::Status::Hovered => Some(th::BG_HOVER.into()),
                _ => None,
            };
            button::Style {
                background: bg,
                text_color: bypass_color,
                border: iced::Border {
                    color: if is_bypassed {
                        th::BORDER
                    } else {
                        th::darken(th::SUCCESS, 0.5)
                    },
                    width: 1.0,
                    radius: 3.0.into(),
                },
                ..Default::default()
            }
        });

    let move_up: Element<'a, Message> = action_btn(
        icons::CHEVRON_UP,
        th::TEXT_DIM,
        th::TEXT,
        Message::MoveEffectUp(track_id, effect.id),
    )
    .into();
    let move_down: Element<'a, Message> = action_btn(
        icons::CHEVRON_DOWN,
        th::TEXT_DIM,
        th::TEXT,
        Message::MoveEffectDown(track_id, effect.id),
    )
    .into();
    let remove: Element<'a, Message> = action_btn(
        icons::X,
        th::TEXT_DIM,
        th::DANGER,
        Message::RemoveEffect(track_id, effect.id),
    )
    .into();

    let mut title_row = row![dot, name_section]
        .spacing(3)
        .align_y(iced::Alignment::Center);
    if let Some(eb) = edit_btn {
        title_row = title_row.push(eb);
    }
    title_row = title_row
        .push(bypass_btn)
        .push(move_up)
        .push(move_down)
        .push(remove);

    let title_bar = container(title_row)
    .padding([4, 6])
    .width(Length::Fill)
    .style(|_theme: &Theme| container::Style {
        background: Some(th::BG_SURFACE.into()),
        ..Default::default()
    });

    // ── Body ─────────────────────────────────────────────────
    let body: Element<'a, Message> = if has_params && !has_gui {
        let knob_color = if is_bypassed {
            th::TEXT_MUTED
        } else {
            track_color
        };
        let mut param_rows = column![].spacing(6);
        let mut current_row = row![].spacing(8);
        let mut count = 0;

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
            let knob_canvas: Element<'a, Message> = canvas(knob)
                .width(Length::Fixed(32.0))
                .height(Length::Fixed(32.0))
                .into();

            let label = text(descriptor.name).size(9).color(th::TEXT_DIM);
            let value_text = format_param_value(value, descriptor.unit);
            let value_label = text(value_text).size(8).color(th::TEXT_MUTED);

            let param_col = column![knob_canvas, label, value_label]
                .spacing(1)
                .align_x(iced::Alignment::Center);

            current_row = current_row.push(param_col);
            count += 1;

            if count % 4 == 0 {
                param_rows = param_rows.push(current_row);
                current_row = row![].spacing(8);
            }
        }

        if count % 4 != 0 {
            param_rows = param_rows.push(current_row);
        }

        container(param_rows)
            .padding([6, 7])
            .width(Length::Fill)
            .into()
    } else {
        Space::new(Length::Fill, Length::Fixed(2.0)).into()
    };

    // ── Card ─────────────────────────────────────────────────
    let card = column![title_bar, body].width(Length::Fixed(CARD_WIDTH));

    container(card)
        .style(|_theme: &Theme| container::Style {
            background: Some(th::BG_ELEVATED.into()),
            border: iced::Border {
                color: th::BORDER,
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
                button::Status::Hovered => (Some(th::BG_HOVER.into()), hover_color),
                button::Status::Pressed => (Some(th::BG_DARK.into()), hover_color),
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

fn format_param_value(value: f32, unit: &str) -> String {
    if unit.is_empty() {
        format!("{value:.2}")
    } else if unit == "Hz" && value >= 1000.0 {
        format!("{:.1}k{unit}", value / 1000.0)
    } else {
        format!("{value:.1}{unit}")
    }
}
