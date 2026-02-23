use iced::widget::{button, canvas, column, container, row, text};
use iced::{Color, Element, Length, Theme};

use crate::icons;
use crate::message::Message;
use crate::state::UiEffect;
use crate::theme as th;
use crate::widgets::effect_knob::EffectKnobWidget;
use vibez_core::id::TrackId;

/// Render an Ableton-style device card for the detail panel.
pub fn view_effect_slot<'a>(
    track_id: TrackId,
    effect: &UiEffect,
    track_color: Color,
) -> Element<'a, Message> {
    let dot_color = if effect.bypass {
        th::TEXT_MUTED
    } else {
        track_color
    };

    // Header: colored dot + effect name + power (bypass) + X (remove)
    let dot = text("\u{25CF}").size(10).color(dot_color);

    let name = text(effect.effect_type.name())
        .size(11)
        .color(if effect.bypass {
            th::TEXT_DIM
        } else {
            th::TEXT
        });

    let bypass_btn = {
        let icon = icons::icon(icons::POWER).size(11);
        if effect.bypass {
            button(icon.color(th::SOLO_ACTIVE))
                .on_press(Message::ToggleEffectBypass(track_id, effect.id))
                .padding([2, 4])
        } else {
            button(icon.color(th::TEXT_DIM))
                .on_press(Message::ToggleEffectBypass(track_id, effect.id))
                .padding([2, 4])
        }
    };

    let remove_btn = button(icons::icon(icons::X).size(11).color(th::TEXT_MUTED))
        .on_press(Message::RemoveEffect(track_id, effect.id))
        .padding([2, 4]);

    let header = row![dot, name, bypass_btn, remove_btn]
        .spacing(4)
        .align_y(iced::Alignment::Center);

    // Parameter knobs in a grid-like layout
    let knob_color = if effect.bypass {
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

        let label = text(descriptor.name).size(10).color(th::TEXT_DIM);
        let value_text = format_param_value(value, descriptor.unit);
        let value_label = text(value_text).size(9).color(th::TEXT_MUTED);

        let param_col = column![knob_canvas, label, value_label]
            .spacing(1)
            .align_x(iced::Alignment::Center);

        current_row = current_row.push(param_col);
        count += 1;

        // 3 knobs per row for device cards
        if count % 3 == 0 {
            param_rows = param_rows.push(current_row);
            current_row = row![].spacing(8);
        }
    }

    // Push remaining knobs
    if count % 3 != 0 {
        param_rows = param_rows.push(current_row);
    }

    // Move up/down buttons
    let move_up = button(icons::icon(icons::CHEVRON_UP).size(10).color(th::TEXT_DIM))
        .on_press(Message::MoveEffectUp(track_id, effect.id))
        .padding([2, 6]);
    let move_down = button(
        icons::icon(icons::CHEVRON_DOWN)
            .size(10)
            .color(th::TEXT_DIM),
    )
    .on_press(Message::MoveEffectDown(track_id, effect.id))
    .padding([2, 6]);
    let move_row = row![move_up, move_down].spacing(2);

    let card = column![header, param_rows, move_row]
        .spacing(6)
        .padding(8)
        .width(Length::Fixed(160.0));

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

fn format_param_value(value: f32, unit: &str) -> String {
    if unit.is_empty() {
        format!("{value:.2}")
    } else if unit == "Hz" && value >= 1000.0 {
        format!("{:.1}k{unit}", value / 1000.0)
    } else {
        format!("{value:.1}{unit}")
    }
}
