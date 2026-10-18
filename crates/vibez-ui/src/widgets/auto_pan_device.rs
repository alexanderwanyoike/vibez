//! Native Auto-Pan body: each side's gain over two cycles and where the
//! output sits between the speakers, above the mode, rate, depth and shape;
//! Advanced holds the phase between the sides, the offset, retrigger and
//! smoothing. The shortest cards put the controls on a line.

use iced::widget::{column, container, row, text, Space};
use iced::{Alignment, Color, Element, Length, Size};
use vibez_core::id::TrackId;
use vibez_dsp::auto_pan::{param, AUTO_PAN_PARAMS};

use crate::domains::auto_pan::{
    stereo_position, DEFAULT_SYNC, MODE_NAMES, RETRIGGER_NAMES, SHAPE_NAMES, SYNC_NAMES,
};
use crate::domains::device_card::{CardMsg, CardPanelState};
use crate::domains::param_entry::ValueEntry;
use crate::message::Message;
use crate::state::UiEffect;
use crate::theme as th;
use crate::widgets::auto_pan_display::{GainPlot, StereoPosition};
use crate::widgets::device_controls::{
    disclosure, heading, plot_band, rule, toggle, vertical_rule, Param, LABEL_SIZE,
};
use crate::widgets::device_layout::{stack, Stack, KNOB_ROW_H};

const PAD: f32 = 10.0;
const PRESET_ROW_H: f32 = 28.0;
const KNOB: f32 = 32.0;
const KNOB_COL_W: f32 = 68.0;
const PLOT_MIN_H: f32 = 72.0;
const POSITION_W: f32 = 150.0;
const RATE_W: f32 = 112.0;
const DIVISION_W: f32 = 72.0;
const CHOICE_W: f32 = 96.0;
/// One labelled value and knob on a single-line row.
const LINE_ITEM_W: f32 = 124.0;
const LINE_GAP: f32 = 12.0;

pub const COMPACT_W: f32 = 420.0;
pub const MAX_W: f32 = 1000.0;

/// Every control fits on the short card's one line.
pub const FULL_W: f32 = PAD * 2.0 + 2.0 * LINE_ITEM_W + 2.0 * CHOICE_W + 3.0 * LINE_GAP;

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

pub fn arrange(size: Size, advanced: bool) -> Stack {
    let below = (size.height - PRESET_ROW_H - 1.0).max(0.0);
    stack(below, advanced.then_some(KNOB_ROW_H), PLOT_MIN_H)
}

pub struct AutoPanView<'a> {
    pub track_id: TrackId,
    pub effect: &'a UiEffect,
    pub accent: Color,
    pub panel: &'a CardPanelState,
    pub entry: Option<&'a ValueEntry>,
    pub bpm: f64,
}

impl<'a> AutoPanView<'a> {
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

    fn synced(&self) -> bool {
        self.effect
            .params
            .get(param::SYNC)
            .copied()
            .unwrap_or(AUTO_PAN_PARAMS[param::SYNC].default)
            >= 0.5
    }

    /// A dropdown under its heading.
    fn choice(
        &self,
        name: &'static str,
        index: usize,
        names: &'static [&'static str],
    ) -> Element<'a, Message> {
        column![
            heading(name),
            container(self.param(index).choices(names, 0)).width(CHOICE_W)
        ]
        .spacing(4)
        .into()
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
            body = body.push(self.plots(stack.plots)).push(rule());
        }
        body = body.push(self.knob_row(stack.knob_row, size.width));
        if let Some(height) = stack.advanced {
            body = body.push(rule()).push(self.advanced(height, size.width));
        }
        body.width(Length::Fill).height(Length::Fill).into()
    }

    fn plots(&self, height: f32) -> Element<'a, Message> {
        plot_band(
            row![
                column![
                    heading("Gain (dB)"),
                    iced::widget::canvas(GainPlot {
                        params: &self.effect.params,
                        bpm: self.bpm,
                        accent: self.accent(),
                    })
                    .width(Length::Fill)
                    .height(Length::Fill),
                ]
                .spacing(2)
                .width(Length::Fill),
                vertical_rule(),
                column![
                    heading("Stereo Position"),
                    iced::widget::canvas(StereoPosition {
                        position: stereo_position(self.panel.output),
                        accent: self.accent(),
                    })
                    .width(Length::Fill)
                    .height(Length::Fill),
                ]
                .spacing(2)
                .width(POSITION_W),
            ]
            .spacing(8),
            height,
            PAD,
        )
    }

    fn sync_switch(&self) -> Element<'a, Message> {
        let synced = self.synced();
        toggle(
            text("Sync").size(LABEL_SIZE).into(),
            synced,
            Message::set_effect_param(
                self.track_id,
                self.effect.id,
                param::SYNC,
                if synced { 0.0 } else { DEFAULT_SYNC as f32 },
            ),
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
        if self.synced() {
            return column![heading("Rate"), self.division(), self.sync_switch()]
                .spacing(4)
                .align_x(Alignment::Center)
                .into();
        }
        let rate = self.param(param::RATE);
        let value = rate.value_entry();
        row![
            iced::widget::canvas(rate.knob()).width(KNOB).height(KNOB),
            column![
                heading("Rate").wrapping(text::Wrapping::None),
                value,
                self.sync_switch()
            ]
            .spacing(2),
        ]
        .spacing(6)
        .align_y(Alignment::Center)
        .into()
    }

    fn knob_row(&self, height: f32, width: f32) -> Element<'a, Message> {
        if height < KNOB_ROW_H {
            return self.line(
                height,
                width,
                vec![
                    if self.synced() {
                        (self.division(), DIVISION_W)
                    } else {
                        (self.param(param::RATE).line_item(), LINE_ITEM_W)
                    },
                    (self.param(param::DEPTH).line_item(), LINE_ITEM_W),
                    (
                        container(self.param(param::MODE).choices(&MODE_NAMES, 0))
                            .width(CHOICE_W)
                            .into(),
                        CHOICE_W,
                    ),
                    (
                        container(self.param(param::SHAPE).choices(&SHAPE_NAMES, 0))
                            .width(CHOICE_W)
                            .into(),
                        CHOICE_W,
                    ),
                ],
            );
        }
        let cell = |control: Element<'a, Message>| -> Element<'a, Message> {
            container(control)
                .width(Length::Fill)
                .max_width(KNOB_COL_W * 1.8)
                .align_x(Alignment::Center)
                .into()
        };
        let knobs = row![
            cell(self.choice("Mode", param::MODE, &MODE_NAMES)),
            vertical_rule(),
            container(self.rate())
                .width(RATE_W)
                .align_x(Alignment::Center),
            cell(self.param(param::DEPTH).column(KNOB)),
            cell(self.choice("Shape", param::SHAPE, &SHAPE_NAMES)),
        ]
        .spacing(0)
        .align_y(Alignment::Center);
        container(knobs)
            .padding([4.0, PAD])
            .height(height)
            .center_y(height)
            .into()
    }

    /// Controls on one line, leaving from the end when it runs out of room.
    fn line(
        &self,
        height: f32,
        width: f32,
        controls: Vec<(Element<'a, Message>, f32)>,
    ) -> Element<'a, Message> {
        let mut room = width - PAD * 2.0;
        let mut line = row![].spacing(LINE_GAP).align_y(Alignment::Center);
        for (control, width) in controls {
            if width > room {
                break;
            }
            room -= width + LINE_GAP;
            line = line.push(control);
        }
        container(line)
            .padding([0.0, PAD])
            .height(height)
            .center_y(height)
            .into()
    }

    fn advanced(&self, height: f32, width: f32) -> Element<'a, Message> {
        if height < KNOB_ROW_H {
            return self.line(
                height,
                width,
                vec![
                    (self.param(param::PHASE).line_item(), LINE_ITEM_W),
                    (self.param(param::OFFSET).line_item(), LINE_ITEM_W),
                    (self.param(param::SMOOTHING).line_item(), LINE_ITEM_W),
                    (
                        container(self.param(param::RETRIGGER).choices(&RETRIGGER_NAMES, 0))
                            .width(CHOICE_W)
                            .into(),
                        CHOICE_W,
                    ),
                ],
            );
        }
        let content = row![
            self.param(param::PHASE).column(KNOB - 6.0),
            self.param(param::OFFSET).column(KNOB - 6.0),
            vertical_rule(),
            self.choice("Retrigger", param::RETRIGGER, &RETRIGGER_NAMES),
            self.param(param::SMOOTHING).column(KNOB - 6.0),
        ]
        .spacing(16)
        .align_y(Alignment::Center);
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
        for width in [COMPACT_W, MAX_W] {
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
