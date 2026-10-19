//! Native Phaser body: the response at either end of its sweep and the LFO
//! above the rate, centre, depth, feedback and mix; Advanced holds the
//! stages, stereo phase, feedback polarity and LFO shape. Narrow cards
//! leave out the LFO plot; the shortest put the knobs on a line.

use iced::widget::{column, container, row, text, Space};
use iced::{Alignment, Color, Element, Length, Size};
use vibez_core::id::TrackId;
use vibez_dsp::phaser::{param, PHASER_PARAMS};

use crate::domains::device_card::{CardMsg, CardPanelState};
use crate::domains::param_entry::ValueEntry;
use crate::domains::phaser::{DEFAULT_SYNC, POLARITY_NAMES, SHAPE_NAMES, STAGE_NAMES, SYNC_NAMES};
use crate::message::Message;
use crate::state::UiEffect;
use crate::theme as th;
use crate::widgets::device_controls::{
    disclosure, heading, plot_band, rule, segmented, toggle, vertical_rule, Param, LABEL_SIZE,
};
use crate::widgets::device_layout::{stack, Stack, KNOB_ROW_H};
use crate::widgets::phaser_display::{LfoPlot, ResponsePlot};

const PAD: f32 = 10.0;
const PRESET_ROW_H: f32 = 28.0;
const KNOB: f32 = 32.0;
const KNOB_COL_W: f32 = 68.0;
const PLOT_MIN_H: f32 = 72.0;
const LFO_W: f32 = 220.0;
const RATE_W: f32 = 112.0;
const DIVISION_W: f32 = 72.0;
const CHOICE_W: f32 = 92.0;
const STAGES_W: f32 = 136.0;
const LINE_GAP: f32 = 12.0;
/// One labelled value and knob on a single-line row.
const LINE_ITEM_W: f32 = 124.0;

pub const COMPACT_W: f32 = 420.0;
/// The LFO plot fits beside the response.
pub const LFO_PLOT_W: f32 = 640.0;
pub const MAX_W: f32 = 1000.0;

/// Every control fits on the short card's one line, and the LFO plots.
pub const FULL_W: f32 = {
    let line = PAD * 2.0 + 5.0 * LINE_ITEM_W + 4.0 * 10.0;
    if line > LFO_PLOT_W {
        line
    } else {
        LFO_PLOT_W
    }
};

/// The widths this card lays itself out for.
pub const WIDTHS: crate::widgets::device_chrome::CardWidths =
    crate::widgets::device_chrome::CardWidths {
        compact: COMPACT_W,
        medium: COMPACT_W,
        expanded: FULL_W,
        max: MAX_W,
    };

pub fn preset_bar_width(size: Size) -> f32 {
    size.width - PAD * 2.0 - 90.0
}

pub fn arrange(size: Size, advanced: bool) -> Stack {
    let below = (size.height - PRESET_ROW_H - 1.0).max(0.0);
    stack(below, advanced.then_some(KNOB_ROW_H), PLOT_MIN_H)
}

pub struct PhaserView<'a> {
    pub track_id: TrackId,
    pub effect: &'a UiEffect,
    pub accent: Color,
    pub panel: &'a CardPanelState,
    pub entry: Option<&'a ValueEntry>,
    pub bpm: f64,
}

impl<'a> PhaserView<'a> {
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
            .unwrap_or(PHASER_PARAMS[index].default)
            .round()
            .max(0.0) as usize
    }

    fn set(&self, index: usize) -> impl Fn(usize) -> Message {
        let (track_id, effect_id) = (self.track_id, self.effect.id);
        move |choice| Message::set_effect_param(track_id, effect_id, index, choice as f32)
    }

    pub fn view(self, preset_bar: Element<'a, Message>, size: Size) -> Element<'a, Message> {
        let stack = arrange(size, self.panel.advanced);
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
        if stack.advanced_only {
            let height = stack.advanced.unwrap_or(0.0);
            body = body.push(self.advanced(height, size.width));
            return body.width(Length::Fill).height(Length::Fill).into();
        }
        if stack.plots > 0.0 {
            body = body
                .push(self.plots(stack.plots, size.width >= LFO_PLOT_W))
                .push(rule());
        }
        body = body.push(self.knob_row(stack.knob_row, size.width));
        if let Some(height) = stack.advanced {
            body = body.push(rule()).push(self.advanced(height, size.width));
        }
        body.width(Length::Fill).height(Length::Fill).into()
    }

    fn plots(&self, height: f32, lfo: bool) -> Element<'a, Message> {
        let mut content = row![column![
            heading("Gain (dB)"),
            iced::widget::canvas(ResponsePlot {
                params: &self.effect.params,
                accent: self.accent(),
            })
            .width(Length::Fill)
            .height(Length::Fill),
        ]
        .spacing(2)
        .width(Length::Fill)]
        .spacing(8);
        if lfo {
            let shape = if self.choice(param::SHAPE) == 1 {
                "LFO (Triangle)"
            } else {
                "LFO (Sine)"
            };
            content = content.push(vertical_rule()).push(
                column![
                    heading(shape),
                    iced::widget::canvas(LfoPlot {
                        params: &self.effect.params,
                        bpm: self.bpm,
                        accent: self.accent(),
                    })
                    .width(Length::Fill)
                    .height(Length::Fill),
                ]
                .spacing(2)
                .width(LFO_W),
            );
        }
        plot_band(content, height, PAD)
    }

    fn sync_switch(&self) -> Element<'a, Message> {
        let synced = self.choice(param::SYNC) > 0;
        toggle(
            text("Sync").size(LABEL_SIZE).into(),
            synced,
            self.set(param::SYNC)(if synced { 0 } else { DEFAULT_SYNC }),
            self.accent(),
        )
    }

    fn division(&self) -> Element<'a, Message> {
        container(self.param(param::SYNC).choices(&SYNC_NAMES, 1))
            .width(DIVISION_W)
            .into()
    }

    /// The rate's knob with its name, value and Sync beside it, or while
    /// synced the note length, as the free rate's knob would do nothing.
    fn rate(&self) -> Element<'a, Message> {
        if self.choice(param::SYNC) > 0 {
            return column![heading("Rate"), self.division(), self.sync_switch()]
                .spacing(4)
                .align_x(Alignment::Center)
                .into();
        }
        let rate = self.param(param::RATE);
        row![
            iced::widget::canvas(rate.knob()).width(KNOB).height(KNOB),
            column![
                heading("Rate").wrapping(text::Wrapping::None),
                rate.value_entry(),
                self.sync_switch()
            ]
            .spacing(2),
        ]
        .spacing(6)
        .align_y(Alignment::Center)
        .into()
    }

    const KNOBS: [usize; 4] = [param::CENTRE, param::DEPTH, param::FEEDBACK, param::MIX];

    fn knob_row(&self, height: f32, width: f32) -> Element<'a, Message> {
        if height < KNOB_ROW_H {
            let fits = ((width - PAD * 2.0) / LINE_ITEM_W).floor().max(1.0) as usize;
            let mut line = row![].spacing(10).align_y(Alignment::Center);
            for index in [
                param::RATE,
                param::DEPTH,
                param::FEEDBACK,
                param::MIX,
                param::CENTRE,
            ]
            .into_iter()
            .take(fits)
            {
                line = line.push(if index == param::RATE && self.choice(param::SYNC) > 0 {
                    self.division()
                } else {
                    self.param(index).line_item()
                });
            }
            return container(line)
                .padding([0.0, PAD])
                .height(height)
                .center_y(height)
                .into();
        }
        let mut knobs = row![
            container(self.rate())
                .width(RATE_W)
                .align_x(Alignment::Center),
            vertical_rule()
        ]
        .spacing(0)
        .align_y(Alignment::Center);
        for index in Self::KNOBS {
            knobs = knobs.push(
                container(self.param(index).column(KNOB))
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

    fn advanced(&self, height: f32, width: f32) -> Element<'a, Message> {
        let stages = segmented(
            &STAGE_NAMES,
            self.choice(param::STAGES).min(STAGE_NAMES.len() - 1),
            self.set(param::STAGES),
            self.accent(),
        );
        let polarity =
            container(self.param(param::POLARITY).choices(&POLARITY_NAMES, 0)).width(CHOICE_W);
        let shape = container(self.param(param::SHAPE).choices(&SHAPE_NAMES, 0)).width(CHOICE_W);
        let content: Element<'a, Message> = if height < KNOB_ROW_H {
            // Controls leave from the end when the line runs out of room.
            let mut room = width - PAD * 2.0;
            let mut line = row![].spacing(LINE_GAP).align_y(Alignment::Center);
            let controls: [(Element<'a, Message>, f32); 4] = [
                (container(stages).width(STAGES_W).into(), STAGES_W),
                (self.param(param::STEREO_PHASE).line_item(), LINE_ITEM_W),
                (polarity.into(), CHOICE_W),
                (shape.into(), CHOICE_W),
            ];
            for (control, width) in controls {
                if width > room {
                    break;
                }
                room -= width + LINE_GAP;
                line = line.push(control);
            }
            line.into()
        } else {
            row![
                column![heading("Stages"), stages].spacing(4),
                self.param(param::STEREO_PHASE).column(KNOB - 6.0),
                vertical_rule(),
                column![heading("FB Polarity"), polarity].spacing(4),
                column![heading("LFO Shape"), shape].spacing(4),
            ]
            .spacing(16)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::device_layout::LINE_H;

    #[test]
    fn every_supported_size_fits_without_scrolling() {
        for width in [COMPACT_W, LFO_PLOT_W, MAX_W] {
            for height in (56..=420).step_by(2).map(|height| height as f32) {
                for advanced in [false, true] {
                    let stack = arrange(Size::new(width, height), advanced);
                    let context = format!("{width}x{height} {advanced}: {stack:?}");
                    let mut used = PRESET_ROW_H + 1.0;
                    if stack.advanced_only {
                        used += stack.advanced.unwrap();
                        assert!(stack.advanced.unwrap() >= LINE_H - 0.01, "{context}");
                    } else {
                        if stack.plots > 0.0 {
                            used += stack.plots + 1.0;
                            assert!(stack.plots >= PLOT_MIN_H, "{context}");
                        }
                        used += stack.knob_row + stack.advanced.map_or(0.0, |h| h + 1.0);
                        assert!(stack.knob_row >= LINE_H, "{context}");
                    }
                    assert!(used <= height + 0.01, "{context}");
                }
            }
        }
    }
}
