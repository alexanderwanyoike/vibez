//! Native Bitcrush body: input and output levels either side of a scope of
//! what it holds of a sine, above the bits, rate, drive and mix. The
//! shortest cards put the knobs on a line.

use iced::widget::{column, container, row, text, Space};
use iced::{Alignment, Color, Element, Length, Size};
use vibez_core::id::TrackId;
use vibez_dsp::bitcrush::param;

use crate::domains::bitcrush::ROUNDING_NAMES;
use crate::domains::device_card::{CardMsg, CardPanelState};
use crate::domains::param_entry::ValueEntry;
use crate::message::Message;
use crate::state::UiEffect;
use crate::theme as th;
use crate::widgets::bitcrush_display::Scope;
use crate::widgets::device_controls::{
    disclosure, heading, level_column, plot_band, rule, segmented, vertical_rule, Param, LABEL_SIZE,
};
use crate::widgets::device_layout::{stack, KNOB_ROW_H};

const PAD: f32 = 10.0;
const PRESET_ROW_H: f32 = 28.0;
const KNOB: f32 = 32.0;
const KNOB_COL_W: f32 = 68.0;
const PLOT_MIN_H: f32 = 72.0;
/// One labelled value and knob on a single-line knob row.
const LINE_ITEM_W: f32 = 124.0;

pub const COMPACT_W: f32 = 420.0;
pub const MAX_W: f32 = 1000.0;

/// Every main control fits on the short card's one line.
pub const FULL_W: f32 = PAD * 2.0 + 4.0 * LINE_ITEM_W + 3.0 * 10.0;

/// The widths this card lays itself out for.
pub const WIDTHS: crate::widgets::device_chrome::CardWidths =
    crate::widgets::device_chrome::CardWidths {
        compact: COMPACT_W,
        medium: COMPACT_W,
        expanded: FULL_W,
        max: MAX_W,
    };

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

pub fn arrange(size: Size, advanced: bool) -> crate::widgets::device_layout::Stack {
    let below = (size.height - PRESET_ROW_H - 1.0).max(0.0);
    stack(below, advanced.then_some(KNOB_ROW_H), PLOT_MIN_H)
}

pub struct BitcrushView<'a> {
    pub track_id: TrackId,
    pub effect: &'a UiEffect,
    pub accent: Color,
    pub panel: &'a CardPanelState,
    pub entry: Option<&'a ValueEntry>,
    pub host_rate: u32,
}

impl<'a> BitcrushView<'a> {
    fn accent(&self) -> Color {
        if self.effect.bypass {
            th::text_muted()
        } else {
            self.accent
        }
    }

    fn param(&self, index: usize) -> Param<'a> {
        Param::new(self.track_id, self.effect, index, self.accent(), self.entry)
            .host_rate(self.host_rate)
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
            body = body.push(self.plots(layout.plots)).push(rule());
        }
        body = body.push(self.knob_row(layout.knob_row, size.width));
        if let Some(height) = layout.advanced {
            body = body.push(rule()).push(self.advanced(height));
        }
        body.width(Length::Fill).height(Length::Fill).into()
    }

    fn plots(&self, height: f32) -> Element<'a, Message> {
        let input_peak = self.panel.input.peak_l.max(self.panel.input.peak_r);
        let legend = row![
            Space::with_width(Length::Fill),
            text("Input").size(LABEL_SIZE).color(th::text_dim()),
            text("Bitcrushed").size(LABEL_SIZE).color(self.accent()),
        ]
        .spacing(8);
        plot_band(
            row![
                level_column("Input", self.panel.input),
                vertical_rule(),
                column![
                    row![heading("Waveform"), legend].align_y(Alignment::Center),
                    iced::widget::canvas(Scope {
                        params: &self.effect.params,
                        host_rate: self.host_rate,
                        input_peak,
                        accent: self.accent(),
                    })
                    .width(Length::Fill)
                    .height(Length::Fill),
                ]
                .spacing(2)
                .width(Length::Fill),
                vertical_rule(),
                level_column("Output", self.panel.output),
            ]
            .spacing(8),
            height,
            PAD,
        )
    }

    const MAIN: [usize; 4] = [param::BITS, param::SR_REDUCE, param::DRIVE_DB, param::MIX];

    fn knob_row(&self, height: f32, width: f32) -> Element<'a, Message> {
        if height < KNOB_ROW_H {
            // Bits come first, so they stay while there is room for one.
            let fits = ((width - PAD * 2.0) / LINE_ITEM_W).floor().max(1.0) as usize;
            let mut line = row![].spacing(10).align_y(Alignment::Center);
            for index in Self::MAIN.into_iter().take(fits) {
                line = line.push(self.param(index).label(short_name(index)).line_item());
            }
            return container(line)
                .padding([0.0, PAD])
                .height(height)
                .center_y(height)
                .into();
        }
        let mut knobs = row![].spacing(0).align_y(Alignment::Center);
        for index in Self::MAIN {
            knobs = knobs.push(
                container(self.param(index).label(short_name(index)).column(KNOB))
                    .width(Length::Fill)
                    .max_width(KNOB_COL_W * 1.6)
                    .align_x(Alignment::Center),
            );
        }
        container(knobs)
            .padding([4.0, PAD])
            .height(height)
            .center_y(height)
            .into()
    }

    fn rounding(&self) -> Element<'a, Message> {
        let (track_id, effect_id) = (self.track_id, self.effect.id);
        let options: Vec<(usize, &'static str)> =
            ROUNDING_NAMES.iter().copied().enumerate().collect();
        let current = self
            .effect
            .params
            .get(param::ROUNDING)
            .copied()
            .unwrap_or(0.0)
            .round()
            .max(0.0) as usize;
        segmented(
            &options,
            current,
            move |rounding| {
                Message::set_effect_param(track_id, effect_id, param::ROUNDING, rounding as f32)
            },
            self.accent(),
        )
    }

    fn advanced(&self, height: f32) -> Element<'a, Message> {
        let post = |line: bool| -> Element<'a, Message> {
            let switch = self.param(param::POST_LP_ENABLED).switch("Post filter");
            let value = self.param(param::POST_LP_HZ).value_knob();
            if line {
                row![switch, value]
                    .spacing(4)
                    .align_y(Alignment::Center)
                    .into()
            } else {
                column![switch, value]
                    .spacing(4)
                    .align_x(Alignment::Center)
                    .into()
            }
        };
        let content: Element<'a, Message> = if height < KNOB_ROW_H {
            // Rounding is the rarest change, so the line leaves it out.
            row![
                self.param(param::DITHER).switch("Dither"),
                self.param(param::JITTER).line_item(),
                post(true),
            ]
            .spacing(12)
            .align_y(Alignment::Center)
            .into()
        } else {
            row![
                column![heading("Rounding"), self.rounding()].spacing(4),
                self.param(param::DITHER).switch("Dither"),
                self.param(param::JITTER).column(KNOB - 6.0),
                post(false),
            ]
            .spacing(12)
            .align_y(Alignment::Center)
            .into()
        };
        container(content)
            .padding([0.0, PAD])
            .height(height)
            .center_y(height)
            .into()
    }
}

/// The concept's names, shorter than the descriptors'.
fn short_name(index: usize) -> &'static str {
    match index {
        param::SR_REDUCE => "Rate",
        _ => vibez_dsp::bitcrush::BITCRUSH_PARAMS[index].name,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::device_layout::LINE_H;

    #[test]
    fn every_supported_size_fits_without_scrolling() {
        for width in [COMPACT_W, MAX_W] {
            for height in (56..=420).step_by(2).map(|height| height as f32) {
                for advanced in [false, true] {
                    let layout = arrange(Size::new(width, height), advanced);
                    let context = format!("{width}x{height} {advanced}: {layout:?}");
                    let mut used = PRESET_ROW_H + 1.0;
                    if layout.advanced_only {
                        used += layout.advanced.unwrap();
                        assert!(layout.advanced.unwrap() >= LINE_H - 0.01, "{context}");
                    } else {
                        if layout.plots > 0.0 {
                            used += layout.plots + 1.0;
                            assert!(layout.plots >= PLOT_MIN_H, "{context}");
                        }
                        used += layout.knob_row + layout.advanced.map_or(0.0, |h| h + 1.0);
                        assert!(layout.knob_row >= LINE_H, "{context}");
                    }
                    assert!(used <= height + 0.01, "{context}");
                }
            }
        }
    }

    #[test]
    fn card_width_grows_with_the_section_up_to_its_maximum() {
        assert_eq!(card_width(300.0), COMPACT_W);
        assert_eq!(card_width(700.0), 700.0);
        assert_eq!(card_width(5000.0), MAX_W);
    }
}
