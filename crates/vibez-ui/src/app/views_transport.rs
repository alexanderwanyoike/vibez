//! Transport bar: play/stop, tempo, position, loop, and metering.
//! Split from views_shell.rs; inherent methods on [`super::App`].

use iced::widget::{
    button, canvas, column, container, horizontal_space, pick_list, row, text, text_input,
};
use iced::{Element, Length, Theme};

use crate::domains::perform::PerformMsg;
use crate::domains::transport::TransportMsg;
use crate::domains::view::ViewMsg;

use crate::icons;
use crate::message::Message;
use crate::state::AppState;
use crate::theme as th;
use crate::widgets::swing_knob::{parse_swing_percent, SwingKnobWidget};
use crate::widgets::vu_meter::VuMeterWidget;

use super::*;

impl App {
    pub(super) fn view_transport(&self) -> Element<'_, Message> {
        // Skip back button
        let skip_back_btn = button(icons::icon(icons::SKIP_BACK).size(16).color(th::text()))
            .on_press(Message::Transport(TransportMsg::Stop))
            .padding([8, 12])
            .style(|_theme: &Theme, _status| button::Style {
                background: Some(th::bg_elevated().into()),
                text_color: th::text(),
                border: iced::Border {
                    color: th::border(),
                    width: 1.0,
                    radius: 4.0.into(),
                },
                ..Default::default()
            });

        // Play/Pause button
        let play_pause_btn = if self.state.transport.playing {
            button(icons::icon(icons::PAUSE).size(16).color(th::accent()))
                .on_press(Message::Transport(TransportMsg::Stop))
                .padding([8, 14])
                .style(|_theme: &Theme, _status| button::Style {
                    background: Some(th::bg_elevated().into()),
                    text_color: th::accent(),
                    border: iced::Border {
                        color: th::accent_dim(),
                        width: 1.0,
                        radius: 4.0.into(),
                    },
                    ..Default::default()
                })
        } else {
            button(icons::icon(icons::PLAY).size(16).color(th::success()))
                .on_press(Message::Transport(TransportMsg::Play))
                .padding([8, 14])
                .style(|_theme: &Theme, _status| button::Style {
                    background: Some(th::bg_elevated().into()),
                    text_color: th::success(),
                    border: iced::Border {
                        color: th::border(),
                        width: 1.0,
                        radius: 4.0.into(),
                    },
                    ..Default::default()
                })
        };

        // Loop toggle button
        let loop_btn = if self.state.transport.loop_enabled {
            button(icons::icon(icons::REPEAT).size(16).color(th::accent()))
                .on_press(Message::Transport(TransportMsg::ToggleArrangementLoop))
                .padding([8, 12])
                .style(|_theme: &Theme, _status| button::Style {
                    background: Some(th::bg_elevated().into()),
                    text_color: th::accent(),
                    border: iced::Border {
                        color: th::accent_dim(),
                        width: 1.0,
                        radius: 4.0.into(),
                    },
                    ..Default::default()
                })
        } else {
            button(icons::icon(icons::REPEAT).size(16).color(th::text_dim()))
                .on_press(Message::Transport(TransportMsg::ToggleArrangementLoop))
                .padding([8, 12])
                .style(|_theme: &Theme, _status| button::Style {
                    background: Some(th::bg_elevated().into()),
                    text_color: th::text_dim(),
                    border: iced::Border {
                        color: th::border(),
                        width: 1.0,
                        radius: 4.0.into(),
                    },
                    ..Default::default()
                })
        };

        let transport_buttons = row![skip_back_btn, play_pause_btn, loop_btn].spacing(4);

        // Time display
        let time_text = text(format!(
            "{} / {}",
            AppState::format_time(self.state.position_seconds()),
            AppState::format_time(self.state.duration_seconds()),
        ))
        .size(14)
        .color(th::text());

        // BPM
        let bpm_input = text_input("BPM", &self.state.transport.bpm_text)
            .on_input(|t| Message::Transport(TransportMsg::BpmChanged(t)))
            .on_submit(Message::Transport(TransportMsg::BpmSubmit))
            .width(Length::Fixed(55.0))
            .size(14);

        let bpm_nudge = |icon: char, delta: f64| {
            button(icons::icon(icon).size(8).color(th::text_dim()))
                .on_press(Message::Transport(TransportMsg::NudgeBpm(delta)))
                .padding([0, 4])
                .style(|_theme: &Theme, status| {
                    let bg = match status {
                        button::Status::Hovered | button::Status::Pressed => {
                            Some(th::bg_hover().into())
                        }
                        _ => None,
                    };
                    button::Style {
                        background: bg,
                        text_color: th::text_dim(),
                        border: iced::Border {
                            radius: 2.0.into(),
                            ..Default::default()
                        },
                        ..Default::default()
                    }
                })
        };
        let bpm_spinner = column![
            bpm_nudge(icons::CHEVRON_UP, 1.0),
            bpm_nudge(icons::CHEVRON_DOWN, -1.0),
        ]
        .spacing(1);

        let bpm_label = text("BPM").size(12).color(th::text_dim());

        let project_swing = self.state.perform.project_swing();
        let project_swing_submit = parse_swing_percent(self.state.perform.project_swing_input())
            .map(|swing| Message::Perform(PerformMsg::SetProjectSwing(swing.get())))
            .unwrap_or_else(|| {
                Message::Perform(PerformMsg::ProjectSwingInput(format!(
                    "{:.0}",
                    project_swing.get() * 100.0
                )))
            });
        let swing_input = text_input("%", self.state.perform.project_swing_input())
            .on_input(|value| Message::Perform(PerformMsg::ProjectSwingInput(value)))
            .on_submit(project_swing_submit)
            .width(Length::Fixed(42.0))
            .padding([2, 4])
            .size(11);
        let swing_knob: Element<'_, Message> = canvas(SwingKnobWidget::project(project_swing))
            .width(Length::Fixed(30.0))
            .height(Length::Fixed(30.0))
            .into();
        let swing_control = container(
            row![
                swing_knob,
                column![
                    text("PROJECT SWING").size(8).color(th::text_muted()),
                    swing_input,
                ]
                .spacing(1),
            ]
            .spacing(6)
            .align_y(iced::Alignment::Center),
        )
        .padding([2, 6])
        .style(|_theme: &Theme| container::Style {
            background: Some(th::bg_surface().into()),
            border: iced::Border {
                color: th::border(),
                width: 1.0,
                radius: 3.0.into(),
            },
            ..Default::default()
        });

        let grid_picker = pick_list(
            crate::state::SnapGrid::all(),
            Some(
                self.state
                    .view
                    .grid_config()
                    .effective_grid(self.active_editor_pixels_per_beat()),
            ),
            |grid| Message::View(ViewMsg::SetSnapGrid(grid)),
        )
        .width(Length::Fixed(86.0))
        .padding([3, 8])
        .text_size(11)
        .style(|_theme: &Theme, status| {
            let highlighted = matches!(
                status,
                pick_list::Status::Hovered | pick_list::Status::Opened
            );
            pick_list::Style {
                text_color: th::text(),
                placeholder_color: th::text_dim(),
                handle_color: if highlighted {
                    th::accent()
                } else {
                    th::text_dim()
                },
                background: th::bg_surface().into(),
                border: iced::Border {
                    color: if highlighted {
                        th::accent_dim()
                    } else {
                        th::border()
                    },
                    width: 1.0,
                    radius: 3.0.into(),
                },
            }
        })
        .menu_style(|_theme: &Theme| iced::widget::overlay::menu::Style {
            background: th::bg_elevated().into(),
            border: iced::Border {
                color: th::border_light(),
                width: 1.0,
                radius: 3.0.into(),
            },
            text_color: th::text(),
            selected_text_color: th::accent(),
            selected_background: th::bg_hover().into(),
        });
        let grid_toggle = |label: &'static str, active: bool, message: ViewMsg| {
            let color = if active { th::accent() } else { th::text_dim() };
            button(text(label).size(9).color(color))
                .on_press(Message::View(message))
                .padding([4, 6])
                .style(move |_theme: &Theme, _status| button::Style {
                    background: Some(
                        if active {
                            th::bg_elevated()
                        } else {
                            th::bg_surface()
                        }
                        .into(),
                    ),
                    text_color: color,
                    border: iced::Border {
                        color: if active {
                            th::accent_dim()
                        } else {
                            th::border()
                        },
                        width: 1.0,
                        radius: 3.0.into(),
                    },
                    ..Default::default()
                })
        };
        let grid_controls = row![
            grid_picker,
            grid_toggle(
                "SNAP",
                self.state.view.snap_enabled,
                ViewMsg::ToggleSnapToGrid,
            ),
            grid_toggle(
                "T",
                self.state.view.snap_grid.is_triplet(),
                ViewMsg::ToggleTripletGrid,
            ),
            grid_toggle(
                "AUTO",
                self.state.view.adaptive_grid,
                ViewMsg::ToggleAdaptiveGrid,
            ),
        ]
        .spacing(3)
        .align_y(iced::Alignment::Center);

        // Master VU meter
        let master_meter = VuMeterWidget {
            peak_l: self.state.peak_l,
            peak_r: self.state.peak_r,
        };
        let master_meter_canvas: Element<'_, Message> = canvas(master_meter)
            .width(Length::Fixed(24.0))
            .height(Length::Fixed(28.0))
            .into();

        let volume_icon = icons::icon(icons::VOLUME_2).size(14).color(th::text_dim());

        let transport = row![
            transport_buttons,
            horizontal_space(),
            time_text,
            horizontal_space(),
            volume_icon,
            master_meter_canvas,
            row![bpm_input, bpm_spinner]
                .spacing(2)
                .align_y(iced::Alignment::Center),
            bpm_label,
            swing_control,
            grid_controls,
        ]
        .spacing(12)
        .padding(10)
        .align_y(iced::Alignment::Center);

        container(transport)
            .width(Length::Fill)
            .style(|_theme: &Theme| container::Style {
                background: Some(th::bg_surface().into()),
                border: iced::Border {
                    color: th::border(),
                    width: 1.0,
                    radius: 0.0.into(),
                },
                ..Default::default()
            })
            .into()
    }
}
