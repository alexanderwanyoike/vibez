//! EQ console composition from native fields, knobs, band switches and real level meters.

use crate::{
    domains::{
        device_controls::DeviceControlState,
        eq::{EqMsg, EqPanel},
    },
    message::Message,
    state::UiEffect,
    theme as th,
    widgets::{
        device_chrome::section_label,
        device_controls::{disclosure, selector, value_field},
        effect_knob::{EffectKnobWidget, KnobTarget},
        level_meter::{LevelMeter, WIDTH as METER_W},
    },
};
use iced::{
    widget::{button, canvas, column, container, horizontal_space, row, text},
    Color, Element, Length,
};
use vibez_core::id::TrackId;
use vibez_dsp::eq::param;
use vibez_engine::{effect_spectrum::EffectSpectrumPosition, metering::MeterValues};

pub(super) const BAND_NAMES: [&str; 4] = ["LF", "LMF", "HMF", "HF"];

pub(super) struct Controls<'a> {
    pub track: TrackId,
    pub effect: &'a UiEffect,
    pub state: &'a DeviceControlState,
    pub panel: &'a EqPanel,
    pub accent: Color,
    pub analyser_active: bool,
}

fn choice_button<'a>(
    label: &'static str,
    active: bool,
    accent: Color,
    message: Message,
) -> iced::widget::Button<'a, Message> {
    button(text(label).size(9))
        .on_press(message)
        .padding([2, 4])
        .style(move |_, status| button::Style {
            text_color: if active { accent } else { th::text_dim() },
            background: Some(
                if active || matches!(status, button::Status::Hovered | button::Status::Pressed) {
                    th::bg_hover()
                } else {
                    th::bg_dark()
                }
                .into(),
            ),
            border: iced::Border {
                color: if active { accent } else { th::border() },
                width: 1.0,
                radius: 2.0.into(),
            },
            ..Default::default()
        })
}

fn choice<'a>(
    label: &'static str,
    active: bool,
    accent: Color,
    message: Message,
) -> Element<'a, Message> {
    choice_button(label, active, accent, message).into()
}

impl<'a> Controls<'a> {
    fn value(&self, index: usize) -> f32 {
        self.effect
            .params
            .get(index)
            .copied()
            .unwrap_or(self.effect.descriptors[index].default)
    }
    fn toggle(&self, index: usize, label: &'static str) -> Element<'a, Message> {
        let on = self.value(index) >= 0.5;
        choice(
            label,
            on,
            self.accent,
            Message::set_effect_param(
                self.track,
                self.effect.id,
                index,
                if on { 0.0 } else { 1.0 },
            ),
        )
    }
    fn band(&self, band: usize) -> Element<'a, Message> {
        choice(
            BAND_NAMES[band],
            self.panel.selected_band == band,
            self.accent,
            Message::EqControl(EqMsg::SelectBand(self.track, self.effect.id, band)),
        )
    }
    fn band_selector(&self) -> Element<'a, Message> {
        let track = self.track;
        let effect = self.effect.id;
        let accent = self.accent;
        iced::widget::pick_list(
            BAND_NAMES,
            Some(BAND_NAMES[self.panel.selected_band]),
            move |name| {
                Message::EqControl(EqMsg::SelectBand(
                    track,
                    effect,
                    BAND_NAMES
                        .iter()
                        .position(|band| *band == name)
                        .unwrap_or(0),
                ))
            },
        )
        .text_size(9)
        .padding([1, 3])
        .width(54.0)
        .style(move |_, _| iced::widget::pick_list::Style {
            text_color: accent,
            placeholder_color: th::text_dim(),
            handle_color: th::text_dim(),
            background: th::bg_dark().into(),
            border: iced::Border {
                color: th::border(),
                width: 1.0,
                radius: 2.0.into(),
            },
        })
        .menu_style(move |_| iced::widget::overlay::menu::Style {
            background: th::bg_surface().into(),
            text_color: th::text(),
            selected_text_color: accent,
            selected_background: th::bg_hover().into(),
            border: iced::Border {
                color: th::border(),
                width: 1.0,
                radius: 2.0.into(),
            },
        })
        .into()
    }
    fn listen(&self, band: usize) -> Element<'a, Message> {
        let active = self.value(param::AUDITION).round() as usize == band + 1;
        choice(
            "Listen",
            active,
            self.accent,
            Message::set_effect_param(
                self.track,
                self.effect.id,
                param::AUDITION,
                if active { 0.0 } else { (band + 1) as f32 },
            ),
        )
    }
    fn knob(&self, index: usize, size: f32) -> Element<'a, Message> {
        let d = &self.effect.descriptors[index];
        let mut knob = EffectKnobWidget::new(
            self.track,
            self.effect.id,
            index,
            self.value(index),
            d.min,
            d.max,
            d.default,
            self.accent,
        );
        if d.unit == "Hz" {
            knob.target = KnobTarget::LogEffect(self.effect.id);
        }
        canvas(knob).width(size).height(size).into()
    }
    fn field(&self, index: usize) -> Element<'a, Message> {
        row![
            value_field(self.track, self.effect, self.state, index, self.accent),
            text(self.effect.descriptors[index].unit)
                .size(8)
                .color(th::text_dim())
        ]
        .spacing(2)
        .align_y(iced::Alignment::Center)
        .into()
    }
    fn console_parameter(&self, index: usize, label: &'static str) -> Element<'a, Message> {
        column![
            section_label(label),
            value_field(self.track, self.effect, self.state, index, self.accent),
            self.knob(index, 30.0)
        ]
        .spacing(3)
        .width(Length::Fill)
        .into()
    }
    fn shape(&self, band: usize) -> Element<'a, Message> {
        selector(
            self.track,
            self.effect,
            param::SHAPE[band],
            "Shape",
            &["Shelf", "Bell"],
            self.accent,
        )
    }
    fn band_group(&self, band: usize, show_listen: bool) -> Element<'a, Message> {
        let shape = if band == 0 || band == 3 {
            self.shape(band)
        } else {
            self.console_parameter(param::SHAPE[band], "Q")
        };
        let mut heading = row![self.band(band), horizontal_space()]
            .spacing(3)
            .align_y(iced::Alignment::Center);
        if show_listen {
            heading = heading.push(self.listen(band));
        }
        heading = heading.push(self.toggle(param::ENABLE[band], "On"));
        column![
            heading,
            row![
                self.console_parameter(param::FREQUENCY[band], "Freq Hz"),
                self.console_parameter(param::GAIN[band], "Gain dB"),
                shape
            ]
            .spacing(5)
            .align_y(iced::Alignment::Start)
        ]
        .spacing(5)
        .width(Length::Fill)
        .into()
    }
    fn editor_parameter(&self, index: usize, label: &'static str) -> Element<'a, Message> {
        row![
            container(section_label(label)).width(55.0),
            self.field(index),
            self.knob(index, 28.0)
        ]
        .spacing(6)
        .align_y(iced::Alignment::Center)
        .into()
    }
    pub fn editor(&self) -> Element<'a, Message> {
        let b = self.panel.selected_band;
        let mut content = column![
            row![
                self.band(b),
                horizontal_space(),
                self.listen(b),
                self.toggle(param::ENABLE[b], "On")
            ]
            .spacing(4)
            .align_y(iced::Alignment::Center),
            self.editor_parameter(param::FREQUENCY[b], "Frequency"),
            self.editor_parameter(param::GAIN[b], "Gain"),
        ]
        .spacing(7);
        content = content.push(if b == 0 || b == 3 {
            self.shape(b)
        } else {
            self.editor_parameter(param::SHAPE[b], "Q")
        });
        container(content).width(196.0).padding([4, 8]).into()
    }
    pub fn selected(&self, knobs: bool, tabs: bool) -> Element<'a, Message> {
        let b = self.panel.selected_band;
        let mut controls = row![].spacing(8).align_y(iced::Alignment::Center);
        for (index, label) in [(param::FREQUENCY[b], "Freq"), (param::GAIN[b], "Gain")] {
            let field = column![section_label(label), self.field(index)]
                .spacing(3)
                .width(Length::Fill);
            let mut pair = row![field].spacing(6).align_y(iced::Alignment::Center);
            if knobs {
                pair = pair.push(self.knob(index, 30.0));
            }
            controls = controls.push(pair.width(Length::Fill));
        }
        let shape = if b == 0 || b == 3 {
            self.shape(b)
        } else {
            let mut pair = row![column![section_label("Q"), self.field(param::SHAPE[b])]
                .spacing(3)
                .width(Length::Fill)]
            .spacing(6)
            .align_y(iced::Alignment::Center);
            if knobs {
                pair = pair.push(self.knob(param::SHAPE[b], 30.0));
            }
            pair.into()
        };
        controls = controls.push(container(shape).width(Length::Fill));
        if tabs {
            let mut bands = row![].spacing(3);
            for band in 0..4 {
                bands = bands.push(
                    choice_button(
                        BAND_NAMES[band],
                        self.panel.selected_band == band,
                        self.accent,
                        Message::EqControl(EqMsg::SelectBand(self.track, self.effect.id, band)),
                    )
                    .width(Length::Fill),
                );
            }
            column![bands, controls].spacing(6).into()
        } else {
            row![
                self.band_selector(),
                controls,
                self.toggle(param::ENABLE[b], "On"),
                self.listen(b)
            ]
            .spacing(5)
            .align_y(iced::Alignment::Center)
            .into()
        }
    }
    fn focus_analyser(&self, compact: bool) -> Element<'a, Message> {
        let caption = if self.analyser_active {
            if compact {
                "On"
            } else {
                "Tapped"
            }
        } else {
            "Analyse"
        };
        choice(
            caption,
            self.analyser_active,
            self.accent,
            Message::EqControl(EqMsg::Focus(self.track, self.effect.id)),
        )
    }
    fn source(&self, post: bool) -> Element<'a, Message> {
        choice(
            if post { "Post" } else { "Pre" },
            self.panel.post == post,
            self.accent,
            Message::EqControl(EqMsg::Source(
                self.track,
                self.effect.id,
                if post {
                    EffectSpectrumPosition::Output
                } else {
                    EffectSpectrumPosition::Input
                },
            )),
        )
    }
    fn freeze(&self) -> Element<'a, Message> {
        choice(
            "Freeze",
            self.panel.frozen,
            self.accent,
            Message::EqControl(EqMsg::Freeze(self.track, self.effect.id)),
        )
    }
    pub fn analyser(&self) -> Element<'a, Message> {
        row![
            self.focus_analyser(false),
            self.source(false),
            self.source(true),
            self.freeze()
        ]
        .spacing(3)
        .align_y(iced::Alignment::Center)
        .into()
    }
    pub fn advanced(&self) -> Element<'a, Message> {
        row![
            self.toggle(param::HP_ENABLED, "HP"),
            self.field(param::HP_FREQUENCY),
            self.toggle(param::LP_ENABLED, "LP"),
            self.field(param::LP_FREQUENCY),
            container(section_label("Trim")).width(30.0),
            self.field(param::OUTPUT)
        ]
        .spacing(6)
        .align_y(iced::Alignment::Center)
        .into()
    }
    fn spectrum(&self) -> Element<'a, Message> {
        column![
            row![self.source(false), self.source(true)].spacing(3),
            row![self.focus_analyser(true), self.freeze()].spacing(3)
        ]
        .spacing(4)
        .into()
    }
    pub fn console(
        &self,
        input: MeterValues,
        output: MeterValues,
        expanded: bool,
        has_editor: bool,
    ) -> Element<'a, Message> {
        let mut bands = row![].spacing(8).height(Length::Fill);
        for band in 0..4 {
            if band > 0 {
                bands = bands.push(iced::widget::vertical_rule(1));
            }
            bands = bands.push(self.band_group(band, !has_editor));
        }
        let mut utilities = row![].spacing(8).height(Length::Fill);
        if expanded {
            utilities = utilities.push(meter("Input", input));
        }
        utilities = utilities.push(meter("Output", output)).push(
            container(
                column![
                    section_label("Trim"),
                    self.knob(param::OUTPUT, 32.0),
                    self.field(param::OUTPUT)
                ]
                .spacing(5),
            )
            .width(54.0),
        );
        if expanded {
            utilities = utilities.push(iced::widget::vertical_rule(1)).push(
                container(
                    column![
                        section_label("Spectrum"),
                        self.spectrum(),
                        disclosure(self.track, self.effect, self.state.advanced)
                    ]
                    .spacing(5),
                )
                .width(100.0),
            );
        }
        let utilities: Element<'a, Message> = if expanded {
            utilities.into()
        } else {
            column![
                utilities,
                disclosure(self.track, self.effect, self.state.advanced)
            ]
            .spacing(4)
            .width(106.0)
            .into()
        };
        row![bands, iced::widget::vertical_rule(1), utilities]
            .spacing(8)
            .height(Length::Fill)
            .into()
    }
    pub fn utility_row(&self) -> Element<'a, Message> {
        row![
            self.analyser(),
            horizontal_space(),
            self.toggle(param::ENABLE[self.panel.selected_band], "On"),
            self.listen(self.panel.selected_band),
            disclosure(self.track, self.effect, self.state.advanced)
        ]
        .spacing(5)
        .align_y(iced::Alignment::Center)
        .into()
    }
    pub fn short(&self) -> Element<'a, Message> {
        if self.state.advanced {
            self.advanced()
        } else {
            self.selected(false, false)
        }
    }
}

fn meter<'a>(label: &'static str, levels: MeterValues) -> Element<'a, Message> {
    let db = vibez_core::level::amplitude_to_db(levels.peak_l.max(levels.peak_r));
    let reading = if db.is_finite() {
        format!("{db:.1}")
    } else {
        "-inf".into()
    };
    column![
        section_label(label),
        canvas(LevelMeter { levels })
            .width(METER_W)
            .height(Length::Fill),
        text(reading)
            .size(8)
            .font(crate::typography::PERFORM_TECH)
            .wrapping(iced::widget::text::Wrapping::None)
    ]
    .spacing(3)
    .width(METER_W)
    .height(Length::Fill)
    .into()
}
