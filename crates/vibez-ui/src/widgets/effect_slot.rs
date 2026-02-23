use iced::widget::{button, canvas, column, container, row, text};
use iced::{Element, Length, Theme};

use crate::message::Message;
use crate::state::UiEffect;
use crate::theme as vibez_theme;
use crate::widgets::effect_knob::EffectKnobWidget;
use vibez_core::id::TrackId;

/// Render an effect card for the detail panel.
pub fn view_effect_slot<'a>(track_id: TrackId, effect: &UiEffect) -> Element<'a, Message> {
    // Header: effect name + BYP + X
    let name = text(effect.effect_type.name())
        .size(11)
        .color(vibez_theme::TEXT);

    let bypass_color = if effect.bypass {
        vibez_theme::SOLO_ACTIVE
    } else {
        vibez_theme::TEXT_DIM
    };
    let bypass_btn = button(text("BYP").size(9).color(bypass_color))
        .on_press(Message::ToggleEffectBypass(track_id, effect.id))
        .padding([2, 4]);

    let remove_btn = button(text("X").size(9).color(vibez_theme::DANGER))
        .on_press(Message::RemoveEffect(track_id, effect.id))
        .padding([2, 4]);

    let header = row![name, bypass_btn, remove_btn]
        .spacing(4)
        .align_y(iced::Alignment::Center);

    // Parameter knobs
    let mut param_col = column![].spacing(4);

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
        );
        let knob_canvas: Element<'a, Message> = canvas(knob)
            .width(Length::Fixed(28.0))
            .height(Length::Fixed(28.0))
            .into();

        let label = text(descriptor.name).size(9).color(vibez_theme::TEXT_DIM);
        let value_text = format_param_value(value, descriptor.unit);
        let value_label = text(value_text).size(8).color(vibez_theme::TEXT_DIM);

        let param_row = row![knob_canvas, column![label, value_label].spacing(1),]
            .spacing(4)
            .align_y(iced::Alignment::Center);

        param_col = param_col.push(param_row);
    }

    // Move up/down buttons
    let move_up = button(text("^").size(9).color(vibez_theme::TEXT_DIM))
        .on_press(Message::MoveEffectUp(track_id, effect.id))
        .padding([2, 6]);
    let move_down = button(text("v").size(9).color(vibez_theme::TEXT_DIM))
        .on_press(Message::MoveEffectDown(track_id, effect.id))
        .padding([2, 6]);
    let move_row = row![move_up, move_down].spacing(2);

    let card = column![header, param_col, move_row]
        .spacing(4)
        .padding(6)
        .width(Length::Fixed(140.0));

    container(card)
        .style(|_theme: &Theme| container::Style {
            background: Some(vibez_theme::BG_SURFACE.into()),
            border: iced::Border {
                color: vibez_theme::TEXT_DIM,
                width: 0.5,
                radius: 2.0.into(),
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
