//! The control bar's audio stream health and CPU load indicators.

use iced::widget::{container, row, text, tooltip};
use iced::{Color, Element, Length, Theme};

use crate::icons;
use crate::message::Message;
use crate::state::AudioStreamHealth;
use crate::theme as th;

const AUDIO_CPU_INDICATOR_WIDTH: f32 = 62.0;
const AUDIO_CPU_VALUE_WIDTH: f32 = 28.0;

pub(super) fn audio_stream_indicator(health: &AudioStreamHealth) -> Element<'_, Message> {
    let (color, description) = match health {
        AudioStreamHealth::Running => (th::success(), health.description().to_string()),
        AudioStreamHealth::Rebuilding => (th::meter_yellow(), health.description().to_string()),
        AudioStreamHealth::Error(cause) => (th::danger(), format!("Audio stream error: {cause}")),
    };
    let control = container(
        row![
            icons::icon(icons::CIRCLE_DOT).size(10).color(color),
            text("AUDIO").size(9).color(color),
        ]
        .spacing(4)
        .align_y(iced::Alignment::Center),
    )
    .padding([4, 6])
    .style(move |_theme: &Theme| container::Style {
        background: Some(th::bg_elevated().into()),
        border: iced::Border {
            color,
            width: 1.0,
            radius: 3.0.into(),
        },
        ..Default::default()
    });
    let hint = container(text(description).size(10).color(th::text()))
        .padding([5, 7])
        .style(|_theme: &Theme| container::Style {
            background: Some(th::bg_elevated().into()),
            border: iced::Border {
                color: th::border_light(),
                width: 1.0,
                radius: 3.0.into(),
            },
            ..Default::default()
        });
    tooltip(control, hint, tooltip::Position::Bottom)
        .gap(6)
        .padding(0)
        .into()
}

fn audio_cpu_value(displayed: u32, color: Color) -> Element<'static, Message> {
    container(
        text(format!("{displayed}%"))
            .font(crate::typography::PERFORM_TECH)
            .size(10)
            .color(color),
    )
    .width(Length::Fixed(AUDIO_CPU_VALUE_WIDTH))
    .align_x(iced::alignment::Horizontal::Right)
    .into()
}

pub(super) fn audio_cpu_indicator(load_percent: f32) -> Element<'static, Message> {
    let color = if load_percent >= 85.0 {
        th::danger()
    } else if load_percent >= 60.0 {
        th::meter_yellow()
    } else {
        th::success()
    };
    let displayed = load_percent.round().clamp(0.0, 999.0) as u32;
    let value = audio_cpu_value(displayed, color);
    let control = container(
        row![text("CPU").size(9).color(th::text_dim()), value,]
            .spacing(4)
            .align_y(iced::Alignment::Center),
    )
    .width(Length::Fixed(AUDIO_CPU_INDICATOR_WIDTH))
    .padding([4, 6])
    .style(move |_theme: &Theme| container::Style {
        background: Some(th::bg_elevated().into()),
        border: iced::Border {
            color,
            width: 1.0,
            radius: 3.0.into(),
        },
        ..Default::default()
    });
    let hint = container(
        text("Audio processing time used per buffer. Red means the audio deadline is at risk.")
            .size(10)
            .color(th::text()),
    )
    .padding([5, 7])
    .style(|_theme: &Theme| container::Style {
        background: Some(th::bg_elevated().into()),
        border: iced::Border {
            color: th::border_light(),
            width: 1.0,
            radius: 3.0.into(),
        },
        ..Default::default()
    });
    tooltip(control, hint, tooltip::Position::Bottom)
        .gap(6)
        .padding(0)
        .into()
}

#[cfg(test)]
mod tests {
    use super::{
        audio_cpu_indicator, audio_cpu_value, AUDIO_CPU_INDICATOR_WIDTH, AUDIO_CPU_VALUE_WIDTH,
    };
    use iced::{Color, Length};

    #[test]
    fn cpu_value_box_absorbs_every_supported_digit_count() {
        for displayed in [5, 13, 100, 999] {
            let value = audio_cpu_value(displayed, Color::WHITE);
            assert_eq!(
                value.as_widget().size().width,
                Length::Fixed(AUDIO_CPU_VALUE_WIDTH)
            );
        }
    }

    #[test]
    fn cpu_indicator_keeps_its_fixed_toolbar_footprint() {
        assert_eq!(
            audio_cpu_indicator(13.0).as_widget().size().width,
            Length::Fixed(AUDIO_CPU_INDICATOR_WIDTH)
        );
    }
}
