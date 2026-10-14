//! Responsive native Gain controls with per-device level and history displays.

use std::collections::VecDeque;
use std::time::Instant;

use iced::mouse;
use iced::widget::{button, canvas, column, container, row, scrollable, text, text_input, tooltip};
use iced::{Color, Element, Length, Point, Rectangle, Renderer, Size, Theme};
use vibez_core::id::{EffectId, TrackId};
use vibez_engine::metering::MeterValues;

use crate::domains::gain::{gain_to_db, GainHistoryPoint, GainMsg, GainPanelState};
use crate::message::Message;
use crate::state::UiEffect;
use crate::theme as th;
use crate::widgets::effect_knob::{EffectKnobWidget, KnobTarget};
use crate::widgets::on_blur::on_blur;

#[allow(clippy::too_many_arguments)]
pub fn view<'a>(
    track_id: TrackId,
    effect: &'a UiEffect,
    accent: Color,
    state: &'a GainPanelState,
    preset_bar: Element<'a, Message>,
    width: f32,
    height: f32,
) -> Element<'a, Message> {
    let compact = width < 480.0 || height < 240.0;
    let primary_height = if state.advanced {
        let advanced_space = if width >= 410.0 { 178.0 } else { 268.0 };
        (height - advanced_space).clamp(128.0, 170.0)
    } else if compact {
        128.0
    } else {
        170.0
    };
    let knob_size = if primary_height < 150.0 { 56.0 } else { 80.0 };
    let control_accent = if effect.bypass {
        th::text_muted()
    } else {
        accent
    };
    let mut gain_knob = EffectKnobWidget::new(
        track_id,
        effect.id,
        0,
        effect.params.first().copied().unwrap_or(1.0),
        0.0,
        2.0,
        1.0,
        control_accent,
    );
    gain_knob.target = KnobTarget::EffectDb(effect.id);

    let gain_control = column![
        text("Gain").size(12).color(th::text()),
        canvas(gain_knob).width(knob_size).height(knob_size),
        value_input(track_id, effect, state, 0, accent),
        action_button(
            "Unity",
            Message::set_effect_param(track_id, effect.id, 0, 1.0),
            accent,
            false,
        ),
    ]
    .spacing(4)
    .align_x(iced::Alignment::Center);

    let mut main = row![
        container(meter_section("Input", state.input, None, primary_height))
            .width(Length::FillPortion(3)),
        vertical_rule(primary_height),
        container(gain_control).center_x(Length::FillPortion(3)),
        vertical_rule(primary_height),
        container(meter_section(
            "Output",
            state.output,
            Some((effect.id, state.clip_latched)),
            primary_height,
        ))
        .width(Length::FillPortion(3)),
    ]
    .spacing(if compact { 6.0 } else { 12.0 })
    .align_y(iced::Alignment::Start);

    if width >= 620.0 {
        let history = column![
            text("Output history · dBFS").size(12).color(th::text()),
            canvas(LevelHistory {
                points: &state.history,
                now: state.last_meter_at.unwrap_or_else(Instant::now),
                accent: control_accent,
            })
            .width(Length::Fill)
            .height(primary_height - 22.0),
        ]
        .spacing(5);
        main = main
            .push(vertical_rule(primary_height))
            .push(container(history).width(Length::FillPortion(5)));
    }

    let advanced_label = if state.advanced {
        "▾ Advanced"
    } else {
        "▸ Advanced"
    };
    let disclosure = button(text(advanced_label).size(12))
        .on_press(Message::GainControl(GainMsg::ToggleAdvanced(effect.id)))
        .padding([3, 4])
        .style(|_theme: &Theme, status| button::Style {
            background: matches!(status, button::Status::Hovered | button::Status::Pressed)
                .then(|| th::bg_hover().into()),
            text_color: th::text(),
            ..Default::default()
        });
    let mut content = column![
        container(main).padding([8, 10]),
        horizontal_rule(),
        container(disclosure).padding([4, 8]),
    ]
    .width(Length::Fill);

    if state.advanced {
        let trims = row![
            trim_control(track_id, effect, state, 1, "L Trim", control_accent),
            trim_control(track_id, effect, state, 2, "R Trim", control_accent),
        ]
        .spacing(14);
        let polarity = row![
            polarity_control(track_id, effect, 3, "L Polarity", accent),
            polarity_control(track_id, effect, 4, "R Polarity", accent),
        ]
        .spacing(14);
        let advanced: Element<'a, Message> = if width >= 410.0 {
            row![trims, vertical_rule(80.0), polarity]
                .spacing(16)
                .align_y(iced::Alignment::Center)
                .into()
        } else {
            column![trims, polarity].spacing(10).into()
        };
        content = content.push(container(advanced).padding([4, 14]));
    }

    column![
        preset_bar,
        horizontal_rule(),
        scrollable(content).height(Length::Fill),
    ]
    .height(Length::Fixed(height.max(1.0)))
    .width(Length::Fixed(width.max(1.0)))
    .into()
}

fn value_input<'a>(
    track_id: TrackId,
    effect: &UiEffect,
    state: &GainPanelState,
    index: usize,
    accent: Color,
) -> Element<'a, Message> {
    let effect_id = effect.id;
    let value = effect
        .params
        .get(index)
        .copied()
        .unwrap_or(if index == 0 { 1.0 } else { 0.0 });
    let shown = state.input_text[index].clone().unwrap_or_else(|| {
        if index == 0 {
            db_text(value)
        } else {
            format!("{value:.1}")
        }
    });
    let invalid = state.input_error[index];
    let submit = Message::GainControl(GainMsg::SubmitInput(track_id, effect_id, index));
    let field = text_input("dB", &shown)
        .on_input(move |text| {
            Message::GainControl(GainMsg::SetInput(track_id, effect_id, index, text))
        })
        .on_submit(submit.clone())
        .size(14)
        .padding([2, 3])
        .width(Length::Fixed(62.0))
        .style(move |_theme: &Theme, status| {
            let focused = matches!(status, text_input::Status::Focused);
            text_input::Style {
                background: th::bg_dark().into(),
                border: iced::Border {
                    color: if invalid {
                        th::danger()
                    } else if focused {
                        accent
                    } else {
                        th::border()
                    },
                    width: 1.0,
                    radius: 2.0.into(),
                },
                icon: th::text_dim(),
                placeholder: th::text_muted(),
                value: th::text(),
                selection: th::with_alpha(accent, 0.35),
            }
        });
    let field_row = row![
        on_blur(field, state.input_text[index].is_some(), submit),
        text("dB").size(11).color(th::text_dim()),
    ]
    .spacing(3)
    .align_y(iced::Alignment::Center);
    let field: Element<'a, Message> = if invalid {
        column![field_row, text("Invalid dB").size(9).color(th::danger())]
            .spacing(2)
            .into()
    } else {
        field_row.into()
    };
    let help = if index == 0 {
        "-60 to +6.02 dB, or -inf for silence. Enter to apply."
    } else {
        "-24 to +24 dB. Enter to apply."
    };
    tooltip(field, text(help).size(10), tooltip::Position::Bottom).into()
}

fn trim_control<'a>(
    track_id: TrackId,
    effect: &UiEffect,
    state: &GainPanelState,
    index: usize,
    label: &'static str,
    accent: Color,
) -> Element<'a, Message> {
    let knob = EffectKnobWidget::new(
        track_id,
        effect.id,
        index,
        effect.params.get(index).copied().unwrap_or(0.0),
        -24.0,
        24.0,
        0.0,
        accent,
    );
    column![
        text(label).size(11).color(th::text_dim()),
        canvas(knob).width(38.0).height(38.0),
        value_input(track_id, effect, state, index, accent),
    ]
    .spacing(3)
    .align_x(iced::Alignment::Center)
    .into()
}

fn polarity_control<'a>(
    track_id: TrackId,
    effect: &UiEffect,
    index: usize,
    label: &'static str,
    accent: Color,
) -> Element<'a, Message> {
    let inverted = effect.params.get(index).copied().unwrap_or(0.0) >= 0.5;
    column![
        text(label).size(11).color(th::text_dim()),
        action_button(
            if inverted { "Inverted" } else { "Normal" },
            Message::set_effect_param(track_id, effect.id, index, if inverted { 0.0 } else { 1.0 }),
            accent,
            inverted,
        ),
    ]
    .spacing(8)
    .align_x(iced::Alignment::Center)
    .into()
}

fn action_button<'a>(
    label: impl Into<String>,
    message: Message,
    accent: Color,
    active: bool,
) -> Element<'a, Message> {
    button(text(label.into()).size(11))
        .on_press(message)
        .padding([3, 9])
        .style(move |_theme: &Theme, status| button::Style {
            background: Some(
                if matches!(status, button::Status::Hovered | button::Status::Pressed) {
                    th::bg_hover().into()
                } else {
                    th::bg_surface().into()
                },
            ),
            text_color: if active { accent } else { th::text() },
            border: iced::Border {
                color: if active { accent } else { th::border() },
                width: 1.0,
                radius: 2.0.into(),
            },
            ..Default::default()
        })
        .into()
}

fn meter_section<'a>(
    label: &'static str,
    levels: MeterValues,
    clip: Option<(EffectId, bool)>,
    height: f32,
) -> Element<'a, Message> {
    let mut heading = row![text(label).size(12).color(th::text())].spacing(8);
    if let Some((effect_id, latched)) = clip {
        let clip_button = button(text("● Clip").size(11).color(if latched {
            th::meter_red()
        } else {
            th::text_muted()
        }))
        .on_press(Message::GainControl(GainMsg::ClearClip(effect_id)))
        .padding(0)
        .style(|_, _| button::Style {
            background: None,
            ..Default::default()
        });
        heading = heading.push(tooltip(
            clip_button,
            text("Reset clip indicator").size(10),
            tooltip::Position::Bottom,
        ));
    }
    column![
        heading,
        canvas(StereoLevelMeter { levels })
            .width(Length::Fill)
            .height(height - 22.0),
    ]
    .spacing(5)
    .into()
}

fn vertical_rule<'a>(height: f32) -> Element<'a, Message> {
    container(iced::widget::Space::new(1.0, height))
        .style(|_| container::Style {
            background: Some(th::divider().into()),
            ..Default::default()
        })
        .into()
}

fn horizontal_rule<'a>() -> Element<'a, Message> {
    container(iced::widget::Space::new(Length::Fill, 1.0))
        .style(|_| container::Style {
            background: Some(th::divider().into()),
            ..Default::default()
        })
        .into()
}

fn db_text(amplitude: f32) -> String {
    if amplitude <= 0.0 || !amplitude.is_finite() {
        "-∞".into()
    } else {
        format!("{:.1}", gain_to_db(amplitude))
    }
}

fn level_fraction(amplitude: f32) -> f32 {
    ((gain_to_db(amplitude).max(-48.0) + 48.0) / 48.0).clamp(0.0, 1.0)
}

struct StereoLevelMeter {
    levels: MeterValues,
}

impl canvas::Program<Message> for StereoLevelMeter {
    type State = ();

    fn draw(
        &self,
        _: &(),
        renderer: &Renderer,
        _: &Theme,
        bounds: Rectangle,
        _: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let beside = bounds.width >= 148.0;
        let meter_height = (bounds.height - if beside { 4.0 } else { 40.0 }).max(12.0);
        let bar_width = 12.0;
        for (channel, (peak, rms)) in [
            (self.levels.peak_l, self.levels.rms_l),
            (self.levels.peak_r, self.levels.rms_r),
        ]
        .into_iter()
        .enumerate()
        {
            let x = channel as f32 * 18.0 + 1.0;
            frame.fill_rectangle(
                Point::new(x, 1.0),
                Size::new(bar_width, meter_height),
                th::display_bg(),
            );
            frame.stroke(
                &canvas::Path::rectangle(Point::new(x, 1.0), Size::new(bar_width, meter_height)),
                canvas::Stroke::default()
                    .with_color(th::border())
                    .with_width(1.0),
            );
            let level = level_fraction(rms);
            for (bottom, top, color) in [
                (0.0, 0.75, th::meter_green()),
                (0.75, 0.9375, th::meter_yellow()),
                (0.9375, 1.0, th::meter_red()),
            ] {
                let filled = (level.min(top) - bottom).max(0.0);
                if filled > 0.0 {
                    frame.fill_rectangle(
                        Point::new(x + 1.0, 1.0 + meter_height * (1.0 - bottom - filled)),
                        Size::new(bar_width - 2.0, filled * meter_height),
                        color,
                    );
                }
            }
            if peak > 0.0 {
                let y = 1.0 + meter_height * (1.0 - level_fraction(peak));
                frame.fill_rectangle(
                    Point::new(x, y),
                    Size::new(bar_width, 2.0),
                    if peak >= 1.0 {
                        th::meter_red()
                    } else {
                        th::meter_yellow()
                    },
                );
            }
        }
        for db in [0, -12, -24, -36, -48] {
            let y = 1.0 + meter_height * (-db as f32 / 48.0);
            frame.fill_text(canvas::Text {
                content: db.to_string(),
                position: Point::new(38.0, y),
                color: th::text_dim(),
                size: 10.0.into(),
                vertical_alignment: if db == -48 {
                    iced::alignment::Vertical::Bottom
                } else {
                    iced::alignment::Vertical::Top
                },
                ..canvas::Text::default()
            });
        }
        let (x, y) = if beside {
            (64.0, meter_height * 0.32)
        } else {
            (1.0, meter_height + 6.0)
        };
        for (line, label, value) in [
            (0.0, "L", self.levels.peak_l),
            (16.0, "R", self.levels.peak_r),
        ] {
            frame.fill_text(canvas::Text {
                content: format!("{label}  {} dBFS", db_text(value)),
                position: Point::new(x, y + line),
                color: th::text(),
                size: 11.0.into(),
                ..canvas::Text::default()
            });
        }
        if beside {
            frame.fill_text(canvas::Text {
                content: "RMS / peak".into(),
                position: Point::new(x, y + 38.0),
                color: th::text_dim(),
                size: 10.0.into(),
                ..canvas::Text::default()
            });
        }
        vec![frame.into_geometry()]
    }
}

struct LevelHistory<'a> {
    points: &'a VecDeque<GainHistoryPoint>,
    now: Instant,
    accent: Color,
}

impl canvas::Program<Message> for LevelHistory<'_> {
    type State = ();

    fn draw(
        &self,
        _: &(),
        renderer: &Renderer,
        _: &Theme,
        bounds: Rectangle,
        _: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let left = 28.0;
        let width = (bounds.width - left - 2.0).max(1.0);
        let height = (bounds.height - 22.0).max(1.0);
        frame.fill_rectangle(
            Point::new(left, 1.0),
            Size::new(width, height),
            th::display_bg(),
        );
        for db in [0, -12, -24, -36, -48] {
            let y = 1.0 + height * (-db as f32 / 48.0);
            frame.stroke(
                &canvas::Path::line(Point::new(left, y), Point::new(left + width, y)),
                canvas::Stroke::default()
                    .with_color(th::divider())
                    .with_width(1.0),
            );
            frame.fill_text(canvas::Text {
                content: db.to_string(),
                position: Point::new(0.0, y),
                color: th::text_dim(),
                size: 10.0.into(),
                vertical_alignment: if db == -48 {
                    iced::alignment::Vertical::Bottom
                } else {
                    iced::alignment::Vertical::Top
                },
                ..canvas::Text::default()
            });
        }
        for second in 0..=10 {
            let x = left + second as f32 / 10.0 * width;
            frame.stroke(
                &canvas::Path::line(Point::new(x, 1.0), Point::new(x, height + 1.0)),
                canvas::Stroke::default()
                    .with_color(th::with_alpha(th::divider(), 0.6))
                    .with_width(1.0),
            );
        }
        for peak in [true, false] {
            let path = canvas::Path::new(|builder| {
                let mut started = false;
                for point in self.points {
                    let age = self.now.saturating_duration_since(point.at).as_secs_f32();
                    if age > 10.0 {
                        continue;
                    }
                    let value = if peak { point.peak } else { point.rms };
                    let position = Point::new(
                        left + (1.0 - age / 10.0) * width,
                        1.0 + (1.0 - level_fraction(value)) * height,
                    );
                    if started {
                        builder.line_to(position);
                    } else {
                        builder.move_to(position);
                        started = true;
                    }
                }
            });
            frame.stroke(
                &path,
                canvas::Stroke::default()
                    .with_color(th::with_alpha(self.accent, if peak { 0.35 } else { 1.0 }))
                    .with_width(if peak { 1.0 } else { 1.5 }),
            );
        }
        frame.fill_text(canvas::Text {
            content: "RMS · peak".into(),
            position: Point::new(left, height + 4.0),
            color: th::text_dim(),
            size: 10.0.into(),
            ..canvas::Text::default()
        });
        frame.fill_text(canvas::Text {
            content: "10 s".into(),
            position: Point::new(bounds.width - 25.0, height + 4.0),
            color: th::text_dim(),
            size: 10.0.into(),
            ..canvas::Text::default()
        });
        vec![frame.into_geometry()]
    }
}
