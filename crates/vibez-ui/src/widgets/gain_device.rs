//! Native Gain body: dB gain, stereo trim and polarity, metered on both sides.

use std::collections::VecDeque;
use std::time::Instant;

use iced::mouse;
use iced::widget::{button, canvas, column, container, row, text, text_input, tooltip, Space};
use iced::{Alignment, Color, Element, Length, Point, Rectangle, Renderer, Size, Theme};
use vibez_core::id::TrackId;
use vibez_engine::metering::MeterValues;

use crate::domains::gain::{gain_to_db, GainHistoryPoint, GainMsg, GainPanelState};
use crate::message::Message;
use crate::state::UiEffect;
use crate::theme as th;
use crate::widgets::device_chrome::section_label;
use crate::widgets::effect_knob::{EffectKnobWidget, KnobTarget};
use crate::widgets::level_meter::{self, LevelMeter};
use crate::widgets::on_blur::on_blur;

const PAD: f32 = 10.0;
const COLUMN_W: f32 = 104.0;
/// Meters plus readouts wide enough for "-48.0" without wrapping.
const METER_COLUMN_W: f32 = 118.0;
const HISTORY_MIN_W: f32 = 210.0;
/// Below this the history is too short to read and is left out.
const HISTORY_MIN_H: f32 = 64.0;
/// Input, Gain and Output side by side.
pub const COMPACT_W: f32 = METER_COLUMN_W * 2.0 + COLUMN_W + 2.0 + PAD * 2.0;
/// Adds the output level history.
pub const MEDIUM_W: f32 = COMPACT_W + 1.0 + HISTORY_MIN_W;
pub const MAX_W: f32 = 820.0;

const PRESET_ROW_H: f32 = 28.0;
const FOOTER_H: f32 = 22.0;
const MAIN_MAX_H: f32 = 150.0;
const MAIN_MIN_H: f32 = 96.0;
const ADVANCED_H: f32 = 84.0;

/// Card width for a share of the device section: compact until the history
/// fits, then everything the section offers up to a readable maximum.
pub fn card_width(available: f32) -> f32 {
    if available >= MEDIUM_W {
        available.min(MAX_W)
    } else {
        COMPACT_W
    }
}

/// How the body divides its measured height. Advanced either stacks under
/// the main controls or takes their place; the body never scrolls.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Arrangement {
    /// Advanced and its toggle share the preset row.
    Short {
        main: f32,
    },
    Normal {
        main: f32,
    },
    WithAdvanced {
        main: f32,
    },
    AdvancedOnly {
        main: f32,
    },
}

pub fn arrange(height: f32, advanced: bool) -> Arrangement {
    let below_preset = (height - PRESET_ROW_H - 1.0).max(0.0);
    if below_preset < MAIN_MIN_H + 1.0 + FOOTER_H {
        let main = below_preset;
        return if advanced {
            Arrangement::AdvancedOnly { main }
        } else {
            Arrangement::Short { main }
        };
    }
    let main_area = below_preset - FOOTER_H - 1.0;
    if !advanced {
        return Arrangement::Normal {
            main: main_area.min(MAIN_MAX_H),
        };
    }
    if main_area >= MAIN_MIN_H + 1.0 + ADVANCED_H {
        Arrangement::WithAdvanced {
            main: (main_area - 1.0 - ADVANCED_H).min(MAIN_MAX_H),
        }
    } else {
        Arrangement::AdvancedOnly { main: main_area }
    }
}

/// Whether the body is too short for a footer, so Advanced joins the preset row.
fn footer_fits(height: f32) -> bool {
    height - PRESET_ROW_H - 1.0 >= MAIN_MIN_H + 1.0 + FOOTER_H
}

/// Width left for the preset bar in a body of this size.
pub fn preset_bar_width(size: Size) -> f32 {
    let disclosure = if footer_fits(size.height) { 0.0 } else { 86.0 };
    size.width - PAD * 2.0 - disclosure
}

pub fn view<'a>(
    track_id: TrackId,
    effect: &'a UiEffect,
    accent: Color,
    state: &'a GainPanelState,
    preset_bar: Element<'a, Message>,
    size: Size,
) -> Element<'a, Message> {
    let control_accent = if effect.bypass {
        th::text_muted()
    } else {
        accent
    };
    let with_footer = footer_fits(size.height);
    let mut top = row![preset_bar].spacing(6).align_y(Alignment::Center);
    if !with_footer {
        top = top
            .push(Space::with_width(Length::Fill))
            .push(disclosure(effect, state.advanced));
    }
    let mut body = column![
        container(top)
            .padding([0.0, PAD])
            .height(PRESET_ROW_H)
            .align_y(Alignment::Center),
        rule(),
    ];
    let main = |height| main_controls(track_id, effect, state, control_accent, size.width, height);
    let advanced = |height| advanced_controls(track_id, effect, state, control_accent, height);
    body = match arrange(size.height, state.advanced) {
        Arrangement::Short { main: height } | Arrangement::Normal { main: height } => {
            body.push(main(height))
        }
        Arrangement::WithAdvanced { main: height } => body
            .push(main(height))
            .push(rule())
            .push(advanced(ADVANCED_H)),
        Arrangement::AdvancedOnly { main: height } => body.push(advanced(height)),
    };
    if with_footer {
        body = body
            .push(Space::with_height(Length::Fill))
            .push(rule())
            .push(
                container(disclosure(effect, state.advanced))
                    .padding([0.0, PAD - 4.0])
                    .height(FOOTER_H)
                    .align_y(Alignment::Center),
            );
    }
    body.width(Length::Fill).height(Length::Fill).into()
}

fn main_controls<'a>(
    track_id: TrackId,
    effect: &'a UiEffect,
    state: &'a GainPanelState,
    accent: Color,
    width: f32,
    height: f32,
) -> Element<'a, Message> {
    let inner = (height - 12.0).max(40.0);
    let mut main = row![
        meter_column("Input", state.input, None, inner),
        vertical_rule(),
        gain_column(track_id, effect, state, accent, inner),
        vertical_rule(),
        meter_column(
            "Output",
            state.output,
            Some((effect.id, state.clip_latched)),
            inner
        ),
    ]
    .height(inner);
    if width >= MEDIUM_W && inner >= HISTORY_MIN_H {
        main = main.push(vertical_rule()).push(
            column![
                section_label("Output history"),
                canvas(LevelHistory {
                    points: &state.history,
                    now: state.last_meter_at.unwrap_or_else(Instant::now),
                    accent,
                })
                .width(Length::Fill)
                .height(Length::Fill),
            ]
            .spacing(4)
            .padding([0.0, 8.0])
            .width(Length::Fill),
        );
    }
    container(main)
        .padding([6.0, PAD])
        .height(height)
        .width(Length::Fill)
        .into()
}

fn meter_column<'a>(
    label: &'static str,
    levels: MeterValues,
    clip: Option<(vibez_core::id::EffectId, bool)>,
    height: f32,
) -> Element<'a, Message> {
    let mut heading = row![section_label(label)]
        .spacing(6)
        .align_y(Alignment::Center);
    if let Some((effect_id, latched)) = clip {
        let clip_label = text("\u{25CF} CLIP").size(8).color(if latched {
            th::meter_red()
        } else {
            th::text_muted()
        });
        heading = heading.push(Space::with_width(Length::Fill)).push(tooltip(
            button(clip_label)
                .on_press(Message::GainControl(GainMsg::ClearClip(effect_id)))
                .padding(0)
                .style(|_, _| button::Style::default()),
            text("Click to reset").size(9),
            tooltip::Position::Top,
        ));
    }
    let readouts = column![readout("L", levels.peak_l), readout("R", levels.peak_r),].spacing(2);
    column![
        heading,
        row![
            canvas(LevelMeter { levels })
                .width(level_meter::WIDTH)
                .height(Length::Fill),
            readouts,
        ]
        .spacing(10)
        .height(Length::Fill),
    ]
    .spacing(4)
    .padding([0.0, 6.0])
    .width(METER_COLUMN_W)
    .height(height)
    .into()
}

fn readout<'a>(channel: &'static str, peak: f32) -> Element<'a, Message> {
    row![
        text(channel).size(9).color(th::text_dim()),
        text(db_text(peak))
            .size(10)
            .font(crate::typography::PERFORM_TECH)
            .wrapping(iced::widget::text::Wrapping::None)
            .color(th::text()),
    ]
    .spacing(4)
    .into()
}

fn gain_column<'a>(
    track_id: TrackId,
    effect: &'a UiEffect,
    state: &'a GainPanelState,
    accent: Color,
    height: f32,
) -> Element<'a, Message> {
    let mut knob = EffectKnobWidget::new(
        track_id,
        effect.id,
        0,
        effect.params.first().copied().unwrap_or(1.0),
        0.0,
        2.0,
        1.0,
        accent,
    );
    knob.target = KnobTarget::EffectDb(effect.id);
    let unity = small_button(
        "Unity",
        Message::set_effect_param(track_id, effect.id, 0, 1.0),
        accent,
        false,
    );
    let stacked = height >= 104.0;
    let entry = value_input(track_id, effect, state, 0, accent, !stacked);
    let knob_size = (height - 60.0).clamp(28.0, 64.0);
    let controls: Element<'a, Message> = if stacked {
        column![
            canvas(knob).width(knob_size).height(knob_size),
            entry,
            unity
        ]
        .spacing(4)
        .align_x(Alignment::Center)
        .into()
    } else {
        row![
            canvas(knob).width(knob_size).height(knob_size),
            column![entry, unity].spacing(3).align_x(Alignment::Center),
        ]
        .spacing(4)
        .align_y(Alignment::Center)
        .into()
    };
    column![section_label("Gain"), controls]
        .spacing(4)
        .align_x(Alignment::Center)
        .width(COLUMN_W)
        .height(height)
        .into()
}

fn advanced_controls<'a>(
    track_id: TrackId,
    effect: &'a UiEffect,
    state: &'a GainPanelState,
    accent: Color,
    height: f32,
) -> Element<'a, Message> {
    let roomy = height >= 70.0;
    let trims = row![
        trim_control(track_id, effect, state, 1, "L trim", accent, roomy),
        trim_control(track_id, effect, state, 2, "R trim", accent, roomy),
    ]
    .spacing(10);
    let polarity = row![
        polarity_control(track_id, effect, 3, "L polarity", accent),
        polarity_control(track_id, effect, 4, "R polarity", accent),
    ]
    .spacing(10);
    container(
        row![trims, vertical_rule(), polarity]
            .spacing(14)
            .align_y(Alignment::Center),
    )
    .padding([4.0, PAD])
    .height(height)
    .align_y(Alignment::Center)
    .into()
}

fn trim_control<'a>(
    track_id: TrackId,
    effect: &'a UiEffect,
    state: &'a GainPanelState,
    index: usize,
    label: &'static str,
    accent: Color,
    roomy: bool,
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
    let size = if roomy { 36.0 } else { 26.0 };
    let knob = canvas(knob).width(size).height(size);
    let entry = value_input(track_id, effect, state, index, accent, true);
    let content: Element<'a, Message> = if roomy {
        column![section_label(label), knob, entry]
            .spacing(3)
            .align_x(Alignment::Center)
            .into()
    } else {
        row![knob, column![section_label(label), entry].spacing(2)]
            .spacing(4)
            .align_y(Alignment::Center)
            .into()
    };
    content
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
        section_label(label),
        small_button(
            if inverted {
                "\u{00D8} Inverted"
            } else {
                "Normal"
            },
            Message::set_effect_param(track_id, effect.id, index, if inverted { 0.0 } else { 1.0 }),
            accent,
            inverted,
        ),
    ]
    .spacing(4)
    .align_x(Alignment::Center)
    .into()
}

fn disclosure<'a>(effect: &UiEffect, open: bool) -> Element<'a, Message> {
    button(
        row![
            text(if open { "\u{25BE}" } else { "\u{25B8}" })
                .size(9)
                .color(th::text_dim()),
            text("Advanced").size(10).color(th::text()),
        ]
        .spacing(4)
        .align_y(Alignment::Center),
    )
    .on_press(Message::GainControl(GainMsg::ToggleAdvanced(effect.id)))
    .padding([2, 4])
    .style(|_theme: &Theme, status| button::Style {
        background: matches!(status, button::Status::Hovered | button::Status::Pressed)
            .then(|| th::bg_hover().into()),
        border: iced::Border {
            radius: 3.0.into(),
            ..Default::default()
        },
        ..Default::default()
    })
    .into()
}

fn value_input<'a>(
    track_id: TrackId,
    effect: &UiEffect,
    state: &GainPanelState,
    index: usize,
    accent: Color,
    narrow: bool,
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
        .size(if narrow { 10 } else { 12 })
        .font(crate::typography::PERFORM_TECH)
        .padding([1, 3])
        .width(if narrow { 40.0 } else { 52.0 })
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
    let field = row![
        on_blur(field, state.input_text[index].is_some(), submit),
        text("dB").size(9).color(th::text_dim()),
    ]
    .spacing(3)
    .align_y(Alignment::Center);
    let help = match (index, invalid) {
        (0, true) => "Invalid: enter -60 to +6.02 dB, or -inf",
        (0, false) => "-60 to +6.02 dB, or -inf for silence. Enter applies.",
        (_, true) => "Invalid: enter -24 to +24 dB",
        (_, false) => "-24 to +24 dB. Enter applies.",
    };
    tooltip(field, text(help).size(9), tooltip::Position::Bottom).into()
}

fn small_button<'a>(
    label: &'static str,
    message: Message,
    accent: Color,
    active: bool,
) -> Element<'a, Message> {
    button(text(label).size(9))
        .on_press(message)
        .padding([2, 8])
        .style(move |_theme: &Theme, status| button::Style {
            background: Some(
                if matches!(status, button::Status::Hovered | button::Status::Pressed) {
                    th::bg_hover().into()
                } else {
                    th::bg_dark().into()
                },
            ),
            text_color: if active { accent } else { th::text() },
            border: iced::Border {
                color: if active { accent } else { th::border() },
                width: 1.0,
                radius: 3.0.into(),
            },
            ..Default::default()
        })
        .into()
}

fn vertical_rule<'a>() -> Element<'a, Message> {
    container(Space::new(1.0, Length::Fill))
        .style(|_| container::Style {
            background: Some(th::divider().into()),
            ..Default::default()
        })
        .into()
}

fn rule<'a>() -> Element<'a, Message> {
    container(Space::new(Length::Fill, 1.0))
        .style(|_| container::Style {
            background: Some(th::divider().into()),
            ..Default::default()
        })
        .into()
}

fn db_text(amplitude: f32) -> String {
    if amplitude <= 0.0 || !amplitude.is_finite() {
        "-inf".into()
    } else {
        format!("{:.1}", gain_to_db(amplitude))
    }
}

struct LevelHistory<'a> {
    points: &'a VecDeque<GainHistoryPoint>,
    now: Instant,
    accent: Color,
}

const HISTORY_SECONDS: f32 = 10.0;

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
        let left = 22.0;
        let width = (bounds.width - left).max(1.0);
        let height = (bounds.height - 12.0).max(1.0);
        let y_of = |db: f32| height * level_meter::scale_position(db);
        frame.fill_rectangle(
            Point::new(left, 0.0),
            Size::new(width, height),
            th::bg_dark(),
        );
        for &db in level_meter::scale_marks(height) {
            let y = y_of(db);
            frame.stroke(
                &canvas::Path::line(Point::new(left, y), Point::new(left + width, y)),
                canvas::Stroke::default()
                    .with_color(th::divider())
                    .with_width(1.0),
            );
            frame.fill_text(canvas::Text {
                content: format!("{db:.0}"),
                position: Point::new(left - 4.0, y),
                color: th::text_dim(),
                size: 8.0.into(),
                horizontal_alignment: iced::alignment::Horizontal::Right,
                vertical_alignment: if db == level_meter::SCALE_DB[0] {
                    iced::alignment::Vertical::Top
                } else if db == level_meter::FLOOR_DB {
                    iced::alignment::Vertical::Bottom
                } else {
                    iced::alignment::Vertical::Center
                },
                ..canvas::Text::default()
            });
        }
        for second in (2..10).step_by(2) {
            let x = left + second as f32 / HISTORY_SECONDS * width;
            frame.stroke(
                &canvas::Path::line(Point::new(x, 0.0), Point::new(x, height)),
                canvas::Stroke::default()
                    .with_color(th::divider())
                    .with_width(1.0),
            );
        }
        for peak in [true, false] {
            let path = canvas::Path::new(|builder| {
                let mut started = false;
                for point in self.points {
                    let age = self.now.saturating_duration_since(point.at).as_secs_f32();
                    if age > HISTORY_SECONDS {
                        continue;
                    }
                    let value = if peak { point.peak } else { point.rms };
                    let db = level_meter::amplitude_to_db(value);
                    // Silence is the absence of a line, not a line on the floor.
                    if db <= level_meter::FLOOR_DB {
                        started = false;
                        continue;
                    }
                    let position =
                        Point::new(left + (1.0 - age / HISTORY_SECONDS) * width, y_of(db));
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
        for (content, x, alignment) in [
            ("RMS / peak", left, iced::alignment::Horizontal::Left),
            ("10 s", bounds.width, iced::alignment::Horizontal::Right),
        ] {
            frame.fill_text(canvas::Text {
                content: content.into(),
                position: Point::new(x, bounds.height),
                color: th::text_dim(),
                size: 8.0.into(),
                horizontal_alignment: alignment,
                vertical_alignment: iced::alignment::Vertical::Bottom,
                ..canvas::Text::default()
            });
        }
        vec![frame.into_geometry()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_supported_height_fits_its_controls_without_scrolling() {
        for height in (60..=400).step_by(4) {
            let height = height as f32;
            for advanced in [false, true] {
                let used = match arrange(height, advanced) {
                    Arrangement::Short { main } => PRESET_ROW_H + 1.0 + main,
                    Arrangement::Normal { main } => PRESET_ROW_H + 1.0 + main + 1.0 + FOOTER_H,
                    Arrangement::WithAdvanced { main } => {
                        PRESET_ROW_H + 1.0 + main + 1.0 + ADVANCED_H + 1.0 + FOOTER_H
                    }
                    Arrangement::AdvancedOnly { main } => {
                        let footer = if footer_fits(height) {
                            1.0 + FOOTER_H
                        } else {
                            0.0
                        };
                        PRESET_ROW_H + 1.0 + main + footer
                    }
                };
                assert!(used <= height + 0.01, "{height} {advanced}: {used}");
            }
        }
    }

    #[test]
    fn advanced_stacks_only_when_both_sections_fit() {
        assert!(matches!(
            arrange(400.0, true),
            Arrangement::WithAdvanced { main } if main == MAIN_MAX_H
        ));
        assert!(matches!(
            arrange(184.0, true),
            Arrangement::AdvancedOnly { .. }
        ));
        assert!(matches!(arrange(184.0, false), Arrangement::Normal { .. }));
        assert!(matches!(arrange(100.0, false), Arrangement::Short { .. }));
    }

    #[test]
    fn card_width_is_compact_until_the_history_fits() {
        assert_eq!(card_width(200.0), COMPACT_W);
        assert_eq!(card_width(MEDIUM_W - 1.0), COMPACT_W);
        assert_eq!(card_width(MEDIUM_W), MEDIUM_W);
        assert_eq!(card_width(4000.0), MAX_W);
    }
}
