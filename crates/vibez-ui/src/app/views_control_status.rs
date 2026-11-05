//! The control bar's right side: grid snap, the output meter, audio health
//! and CPU load.

use iced::widget::{canvas, column, container, pick_list, row, text};
use iced::{Alignment, Color, Element, Length, Size};

use crate::domains::view::{GridTarget, ViewMsg};

use crate::icons;
use crate::message::Message;
use crate::state::{AudioStreamHealth, GridCommand};
use crate::theme as th;
use crate::typography;
use crate::widgets::hint::with_hint;
use crate::widgets::vu_meter::HorizontalVuMeterWidget;

use super::views_control_bar_style as style;
use super::*;

const SNAP_BUTTON_WIDTH: f32 = 30.0;
const SNAP_BUTTON_HEIGHT: f32 = 26.0;
const GRID_MENU_WIDTH: f32 = 92.0;
const OUTPUT_METER_WIDTH: f32 = 56.0;
const OUTPUT_METER_HEIGHT: f32 = 9.0;
const AUDIO_CPU_INDICATOR_WIDTH: f32 = 54.0;
const AUDIO_CPU_VALUE_WIDTH: f32 = 28.0;

fn audio_stream_indicator(health: &AudioStreamHealth) -> Element<'_, Message> {
    let (color, description) = match health {
        AudioStreamHealth::Running => (th::success(), health.description().to_string()),
        AudioStreamHealth::Rebuilding => (th::meter_yellow(), health.description().to_string()),
        AudioStreamHealth::Error(cause) => (th::danger(), format!("Audio stream error: {cause}")),
    };
    let control = row![
        icons::icon(icons::CIRCLE_DOT).size(8).color(color),
        text("Audio").size(10).color(th::text_dim()),
    ]
    .spacing(4)
    .align_y(Alignment::Center);
    with_hint(control, description)
}

fn audio_cpu_value(displayed: u32, color: Color) -> Element<'static, Message> {
    container(
        text(format!("{displayed}%"))
            .font(typography::PERFORM_TECH)
            .size(10)
            .color(color),
    )
    .width(Length::Fixed(AUDIO_CPU_VALUE_WIDTH))
    .align_x(iced::alignment::Horizontal::Right)
    .into()
}

fn audio_cpu_indicator(load_percent: f32) -> Element<'static, Message> {
    let color = if load_percent >= 85.0 {
        th::danger()
    } else if load_percent >= 60.0 {
        th::meter_yellow()
    } else {
        th::success()
    };
    let displayed = load_percent.round().clamp(0.0, 999.0) as u32;
    let control = container(
        row![
            text("CPU").size(10).color(th::text_dim()),
            audio_cpu_value(displayed, color),
        ]
        .spacing(4)
        .align_y(Alignment::Center),
    )
    .width(Length::Fixed(AUDIO_CPU_INDICATOR_WIDTH))
    .align_x(iced::alignment::Horizontal::Right);
    with_hint(
        control,
        "Audio processing time used per buffer. Red means the audio deadline is at risk.",
    )
}

fn snap_button<'a>(
    content: impl Into<Element<'a, Message>>,
    on: bool,
    command: GridCommand,
) -> Element<'a, Message> {
    style::segment_button(
        content,
        Size::new(SNAP_BUTTON_WIDTH, SNAP_BUTTON_HEIGHT),
        on.then(th::accent),
        Message::View(ViewMsg::Grid(GridTarget::Arrange, command)),
    )
}

impl App {
    pub(super) fn view_control_status(&self) -> Element<'_, Message> {
        let meter: Element<'_, Message> = canvas(HorizontalVuMeterWidget::new(
            self.state.peak_l,
            self.state.peak_r,
            th::meter_green(),
        ))
        .width(Length::Fixed(OUTPUT_METER_WIDTH))
        .height(Length::Fixed(OUTPUT_METER_HEIGHT))
        .into();
        let health = column![
            audio_stream_indicator(&self.state.audio_stream_health),
            audio_cpu_indicator(self.state.audio_cpu_load_percent),
        ]
        .spacing(2)
        .align_x(Alignment::End);

        row![self.view_snap_group(), meter, health]
            .spacing(10)
            .align_y(Alignment::Center)
            .into()
    }

    fn view_snap_group(&self) -> Element<'_, Message> {
        let grid = self.state.view.arrange_grid;
        let pixels_per_beat = self.timeline_pixels_per_beat();
        let grid_menu = pick_list(
            grid.choices(pixels_per_beat),
            Some(grid.choice(pixels_per_beat)),
            |choice| {
                Message::View(ViewMsg::Grid(
                    GridTarget::Arrange,
                    GridCommand::Choose(choice),
                ))
            },
        )
        .width(Length::Fixed(GRID_MENU_WIDTH))
        .padding([5, 8])
        .text_size(12)
        .style(style::group_pick_list)
        .menu_style(style::group_pick_list_menu);

        let group = row![
            snap_button(
                icons::icon(icons::MAGNET).size(14),
                grid.snap_enabled,
                GridCommand::ToggleSnap,
            ),
            grid_menu,
            snap_button(
                text("3").font(typography::PERFORM_LABEL).size(12),
                grid.selected.is_triplet(),
                GridCommand::ToggleTriplet,
            ),
        ]
        .spacing(2)
        .align_y(Alignment::Center);
        container(group)
            .padding(style::GROUP_INSET)
            .style(style::group)
            .into()
    }
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
