//! Native Drive body: input and output levels either side of its transfer
//! curve and a sine through it, above the knobs and the character. Narrow
//! cards leave out the sine; the shortest put the knobs on a line.

use iced::widget::{canvas, column, container, row, text, Space};
use iced::{Alignment, Color, Element, Length, Size};
use vibez_core::id::TrackId;
use vibez_dsp::drive::{character, param};

use crate::domains::device_card::{CardMsg, CardPanelState};
use crate::domains::drive::{CHARACTER_NAMES, QUALITY_NAMES};
use crate::domains::param_entry::ValueEntry;
use crate::message::Message;
use crate::state::UiEffect;
use crate::theme as th;
use crate::widgets::device_controls::{
    disclosure, dropdown, heading, level_column, plot_band, rule, segmented, vertical_rule, Param,
    LABEL_SIZE, METER_W,
};
use crate::widgets::device_layout::{stack, KNOB_ROW_H};
use crate::widgets::drive_display::{TransferCurve, Waveform};

const PAD: f32 = 10.0;
const PRESET_ROW_H: f32 = 28.0;
const KNOB: f32 = 32.0;
const KNOB_COL_W: f32 = 68.0;
const PLOT_MIN_H: f32 = 72.0;
const PLOT_MIN_W: f32 = 200.0;
const CHARACTER_W: f32 = 96.0;
/// One labelled value and knob on a single-line knob row.
const LINE_ITEM_W: f32 = 124.0;
const LINE_CHARACTER_W: f32 = 64.0;

pub const COMPACT_W: f32 = 420.0;
/// The sine fits beside the transfer curve.
pub const WAVEFORM_W: f32 = METER_W * 2.0 + PLOT_MIN_W * 2.0 + 3.0 * 8.0 + PAD * 2.0;
pub const MAX_W: f32 = 1000.0;

pub fn card_width(available: f32) -> f32 {
    if available >= COMPACT_W {
        available.min(MAX_W)
    } else {
        COMPACT_W
    }
}

pub fn preset_bar_width(size: Size) -> f32 {
    size.width - PAD * 2.0 - 90.0
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Arrangement {
    pub waveform: bool,
    pub plots: f32,
    pub knob_row: f32,
    pub advanced: Option<f32>,
    pub advanced_only: bool,
}

pub fn arrange(size: Size, advanced: bool) -> Arrangement {
    let below = (size.height - PRESET_ROW_H - 1.0).max(0.0);
    let stack = stack(below, advanced.then_some(KNOB_ROW_H), PLOT_MIN_H);
    Arrangement {
        waveform: size.width >= WAVEFORM_W,
        plots: stack.plots,
        knob_row: stack.knob_row,
        advanced: stack.advanced,
        advanced_only: stack.advanced_only,
    }
}

/// A character, named for the dropdown.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Character(usize);

impl std::fmt::Display for Character {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(CHARACTER_NAMES[self.0])
    }
}

pub struct DriveView<'a> {
    pub track_id: TrackId,
    pub effect: &'a UiEffect,
    pub accent: Color,
    pub panel: &'a CardPanelState,
    pub entry: Option<&'a ValueEntry>,
}

impl<'a> DriveView<'a> {
    fn accent(&self) -> Color {
        if self.effect.bypass {
            th::text_muted()
        } else {
            self.accent
        }
    }

    fn param(&self, index: usize) -> Param<'a> {
        Param::new(self.track_id, self.effect, index, self.accent(), self.entry)
    }

    fn choice(&self, index: usize) -> usize {
        self.effect
            .params
            .get(index)
            .copied()
            .unwrap_or(vibez_dsp::drive::DRIVE_PARAMS[index].default)
            .round()
            .max(0.0) as usize
    }

    fn input_peak(&self) -> f32 {
        self.panel.input.peak_l.max(self.panel.input.peak_r)
    }

    pub fn view(self, preset_bar: Element<'a, Message>, size: Size) -> Element<'a, Message> {
        let layout = arrange(size, self.panel.advanced);
        let top = row![
            preset_bar,
            Space::with_width(Length::Fill),
            disclosure(
                self.panel.advanced,
                Message::Card(CardMsg::ToggleAdvanced(self.effect.id)),
                self.accent(),
            )
        ]
        .spacing(6)
        .align_y(Alignment::Center);
        let mut body = column![
            container(top)
                .padding([0.0, PAD])
                .height(PRESET_ROW_H)
                .align_y(Alignment::Center),
            rule(),
        ];
        if layout.advanced_only {
            let height = layout.advanced.unwrap_or(0.0);
            body = body.push(self.advanced(height));
            return body.width(Length::Fill).height(Length::Fill).into();
        }
        if layout.plots > 0.0 {
            body = body.push(self.plots(layout)).push(rule());
        }
        body = body.push(self.knob_row(layout, size.width));
        if let Some(height) = layout.advanced {
            body = body.push(rule()).push(self.advanced(height));
        }
        body.width(Length::Fill).height(Length::Fill).into()
    }

    fn plots(&self, layout: Arrangement) -> Element<'a, Message> {
        let mut content = row![
            level_column("Input", self.panel.input),
            vertical_rule(),
            column![
                heading("Transfer curve (dB)"),
                canvas(TransferCurve {
                    params: &self.effect.params,
                    input_peak: self.input_peak(),
                    accent: self.accent(),
                })
                .width(Length::Fill)
                .height(Length::Fill),
            ]
            .spacing(2)
            .width(Length::Fill),
        ]
        .spacing(8);
        if layout.waveform {
            content = content.push(vertical_rule()).push(
                column![
                    heading("Waveform (in/out)"),
                    canvas(Waveform {
                        params: &self.effect.params,
                        input_peak: self.input_peak(),
                        accent: self.accent(),
                    })
                    .width(Length::Fill)
                    .height(Length::Fill),
                ]
                .spacing(2)
                .width(Length::Fill),
            );
        }
        plot_band(
            content
                .push(vertical_rule())
                .push(level_column("Output", self.panel.output)),
            layout.plots,
            PAD,
        )
    }

    const MAIN: [usize; 4] = [param::AMOUNT, param::TONE, param::OUTPUT, param::MIX];

    fn character(&self) -> Element<'a, Message> {
        let (track_id, effect_id) = (self.track_id, self.effect.id);
        container(dropdown(
            (0..character::COUNT).map(Character).collect(),
            Character(self.choice(param::CHARACTER).min(character::COUNT - 1)),
            move |choice| {
                Message::set_effect_param(track_id, effect_id, param::CHARACTER, choice.0 as f32)
            },
        ))
        .width(CHARACTER_W)
        .into()
    }

    fn knob_row(&self, layout: Arrangement, width: f32) -> Element<'a, Message> {
        if layout.knob_row < KNOB_ROW_H {
            // Drive comes first, so it stays while there is room for one.
            let fits = ((width - PAD * 2.0 - LINE_CHARACTER_W) / LINE_ITEM_W)
                .floor()
                .max(1.0) as usize;
            let mut line = row![].spacing(10).align_y(Alignment::Center);
            for index in [param::AMOUNT, param::OUTPUT, param::MIX, param::TONE]
                .into_iter()
                .take(fits)
            {
                line = line.push(self.param(index).line_item());
            }
            let name = CHARACTER_NAMES[self.choice(param::CHARACTER).min(character::COUNT - 1)];
            return container(
                line.push(Space::with_width(Length::Fill))
                    .push(text(name).size(LABEL_SIZE).color(th::text_dim())),
            )
            .padding([0.0, PAD])
            .height(layout.knob_row)
            .center_y(layout.knob_row)
            .into();
        }
        let mut knobs = row![].spacing(0).align_y(Alignment::Center);
        for index in Self::MAIN {
            knobs = knobs.push(
                container(self.param(index).column(KNOB))
                    .width(Length::Fill)
                    .max_width(KNOB_COL_W * 1.6)
                    .align_x(Alignment::Center),
            );
        }
        container(
            row![
                container(knobs).width(Length::Fill),
                vertical_rule(),
                column![heading("Character"), self.character()].spacing(4),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        )
        .padding([4.0, PAD])
        .height(layout.knob_row)
        .center_y(layout.knob_row)
        .into()
    }

    /// Classic is the original curve at the host rate; only the newer
    /// curves oversample.
    fn oversampling(&self) -> Element<'a, Message> {
        if self.choice(param::CHARACTER) == character::CLASSIC {
            return text("Classic runs at 1x")
                .size(LABEL_SIZE)
                .color(th::text_muted())
                .wrapping(text::Wrapping::None)
                .into();
        }
        let (track_id, effect_id) = (self.track_id, self.effect.id);
        let options: Vec<(usize, &'static str)> =
            QUALITY_NAMES.iter().copied().enumerate().collect();
        segmented(
            &options,
            self.choice(param::QUALITY),
            move |quality| {
                Message::set_effect_param(track_id, effect_id, param::QUALITY, quality as f32)
            },
            self.accent(),
        )
    }

    fn advanced(&self, height: f32) -> Element<'a, Message> {
        let filter = |enabled, frequency, label| -> Element<'a, Message> {
            if height < KNOB_ROW_H {
                row![
                    self.param(enabled).switch(label),
                    self.param(frequency).value_knob()
                ]
                .spacing(4)
                .align_y(Alignment::Center)
                .into()
            } else {
                column![
                    self.param(enabled).switch(label),
                    self.param(frequency).value_knob()
                ]
                .spacing(4)
                .align_x(Alignment::Center)
                .into()
            }
        };
        let oversampling: Element<'a, Message> = if height < KNOB_ROW_H {
            self.oversampling()
        } else {
            column![heading("Oversampling"), self.oversampling()]
                .spacing(4)
                .into()
        };
        container(
            row![
                filter(param::PRE_HP_ENABLED, param::PRE_HP_FREQUENCY, "Pre filter"),
                filter(
                    param::POST_LP_ENABLED,
                    param::POST_LP_FREQUENCY,
                    "Post filter"
                ),
                oversampling,
            ]
            .spacing(12)
            .align_y(Alignment::Center),
        )
        .padding([0.0, PAD])
        .height(height)
        .center_y(height)
        .into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::device_layout::LINE_H;

    fn used(layout: Arrangement) -> f32 {
        let mut used = PRESET_ROW_H + 1.0;
        if layout.advanced_only {
            return used + layout.advanced.unwrap();
        }
        if layout.plots > 0.0 {
            used += layout.plots + 1.0;
        }
        used += layout.knob_row;
        if let Some(height) = layout.advanced {
            used += 1.0 + height;
        }
        used
    }

    #[test]
    fn every_supported_size_fits_without_scrolling() {
        for width in [COMPACT_W, WAVEFORM_W, MAX_W] {
            for height in (56..=420).step_by(2).map(|height| height as f32) {
                for advanced in [false, true] {
                    let layout = arrange(Size::new(width, height), advanced);
                    let context = format!("{width}x{height} {advanced}: {layout:?}");
                    assert!(used(layout) <= height + 0.01, "{context}");
                    assert!(
                        layout.plots == 0.0 || layout.plots >= PLOT_MIN_H,
                        "{context}"
                    );
                    if layout.advanced_only {
                        assert!(layout.advanced.unwrap() >= LINE_H - 0.01, "{context}");
                    } else {
                        assert!(layout.knob_row >= LINE_H, "{context}");
                    }
                }
            }
        }
    }

    #[test]
    fn the_sine_joins_the_curve_once_both_fit() {
        assert!(!arrange(Size::new(COMPACT_W, 300.0), false).waveform);
        assert!(arrange(Size::new(WAVEFORM_W, 300.0), false).waveform);
    }

    #[test]
    fn card_width_grows_with_the_section_up_to_its_maximum() {
        assert_eq!(card_width(300.0), COMPACT_W);
        assert_eq!(card_width(700.0), 700.0);
        assert_eq!(card_width(5000.0), MAX_W);
    }
}
