//! Native Reverb body: its impulse response and output level above the
//! pre-delay, decay, damping, width and mix; Advanced holds the diffusion,
//! the wet cuts and freeze. The shortest cards put the knobs on a line.

use iced::widget::{column, container, row, Space};
use iced::{Alignment, Color, Element, Length, Size};
use vibez_core::id::TrackId;
use vibez_dsp::reverb::{param, REVERB_PARAMS};

use crate::domains::device_card::{CardMsg, CardPanelState};
use crate::domains::param_entry::ValueEntry;
use crate::domains::reverb::{DEFAULT_SYNC, PREDELAY_SYNC_NAMES};
use crate::message::Message;
use crate::state::UiEffect;
use crate::theme as th;
use crate::widgets::device_controls::{
    disclosure, dropdown, heading, level_column, plot_band, rule, toggle, vertical_rule, Param,
    LABEL_SIZE,
};
use crate::widgets::device_layout::{stack, Stack, KNOB_ROW_H};
use crate::widgets::reverb_display::ImpulsePlot;

const PAD: f32 = 10.0;
const PRESET_ROW_H: f32 = 28.0;
const KNOB: f32 = 32.0;
const KNOB_COL_W: f32 = 68.0;
const PLOT_MIN_H: f32 = 72.0;
const DIVISION_W: f32 = 72.0;
/// The pre-delay's knob with its name, value and Sync beside it.
const PREDELAY_W: f32 = 112.0;
/// One labelled value and knob on a single-line row.
const LINE_ITEM_W: f32 = 124.0;

pub const COMPACT_W: f32 = 420.0;
pub const MAX_W: f32 = 1000.0;

/// Every control fits on the short card's one line.
pub const FULL_W: f32 = PAD * 2.0 + 5.0 * LINE_ITEM_W + 4.0 * 10.0;

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

/// A synced pre-delay, named for the dropdown.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Division(usize);

impl std::fmt::Display for Division {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(PREDELAY_SYNC_NAMES[self.0])
    }
}

pub struct ReverbView<'a> {
    pub track_id: TrackId,
    pub effect: &'a UiEffect,
    pub accent: Color,
    pub panel: &'a CardPanelState,
    pub entry: Option<&'a ValueEntry>,
    pub bpm: f64,
}

impl<'a> ReverbView<'a> {
    fn accent(&self) -> Color {
        if self.effect.bypass {
            th::text_muted()
        } else {
            self.accent
        }
    }

    fn param(&self, index: usize) -> Param<'a> {
        let param = Param::new(self.track_id, self.effect, index, self.accent(), self.entry);
        match index {
            param::ROOM_SIZE => param.label("Decay"),
            param::PREDELAY_MS => param.label("Pre-delay"),
            _ => param,
        }
    }

    fn sync(&self) -> usize {
        self.effect
            .params
            .get(param::PREDELAY_SYNC)
            .copied()
            .unwrap_or(REVERB_PARAMS[param::PREDELAY_SYNC].default)
            .round()
            .clamp(0.0, (PREDELAY_SYNC_NAMES.len() - 1) as f32) as usize
    }

    fn set_sync(&self, sync: usize) -> Message {
        Message::set_effect_param(
            self.track_id,
            self.effect.id,
            param::PREDELAY_SYNC,
            sync as f32,
        )
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
            body = body.push(self.advanced(height));
            return body.width(Length::Fill).height(Length::Fill).into();
        }
        if stack.plots > 0.0 {
            body = body.push(self.plots(stack.plots)).push(rule());
        }
        body = body.push(self.knob_row(stack.knob_row, size.width));
        if let Some(height) = stack.advanced {
            body = body.push(rule()).push(self.advanced(height));
        }
        body.width(Length::Fill).height(Length::Fill).into()
    }

    fn plots(&self, height: f32) -> Element<'a, Message> {
        plot_band(
            row![
                column![
                    heading("Impulse response"),
                    iced::widget::canvas(ImpulsePlot {
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
                level_column("Output", self.panel.output),
            ]
            .spacing(8),
            height,
            PAD,
        )
    }

    fn sync_switch(&self) -> Element<'a, Message> {
        let synced = self.sync() > 0;
        toggle(
            iced::widget::text("Sync").size(LABEL_SIZE).into(),
            synced,
            self.set_sync(if synced { 0 } else { DEFAULT_SYNC }),
            self.accent(),
        )
    }

    /// The pre-delay as a knob, or as a note value while synced.
    fn predelay(&self) -> Element<'a, Message> {
        let sync = self.sync();
        if sync == 0 {
            // The concept names this knob beside it, leaving room for Sync.
            let predelay = self.param(param::PREDELAY_MS);
            return row![
                iced::widget::canvas(predelay.knob())
                    .width(KNOB)
                    .height(KNOB),
                column![
                    heading("Pre-delay").wrapping(iced::widget::text::Wrapping::None),
                    predelay.value_entry(),
                    self.sync_switch()
                ]
                .spacing(2),
            ]
            .spacing(6)
            .align_y(Alignment::Center)
            .into();
        }
        let (track_id, effect_id) = (self.track_id, self.effect.id);
        column![
            heading("Pre-delay"),
            container(dropdown(
                (1..PREDELAY_SYNC_NAMES.len()).map(Division).collect(),
                Division(sync),
                move |division| {
                    Message::set_effect_param(
                        track_id,
                        effect_id,
                        param::PREDELAY_SYNC,
                        division.0 as f32,
                    )
                },
            ))
            .width(DIVISION_W),
            self.sync_switch(),
        ]
        .spacing(4)
        .align_x(Alignment::Center)
        .into()
    }

    const KNOBS: [usize; 4] = [param::ROOM_SIZE, param::DAMPING, param::WIDTH, param::MIX];

    fn knob_row(&self, height: f32, width: f32) -> Element<'a, Message> {
        if height < KNOB_ROW_H {
            // Decay and mix come first, so they stay while there is room.
            let fits = ((width - PAD * 2.0) / LINE_ITEM_W).floor().max(1.0) as usize;
            let mut line = row![].spacing(10).align_y(Alignment::Center);
            for index in [
                param::ROOM_SIZE,
                param::MIX,
                param::PREDELAY_MS,
                param::DAMPING,
                param::WIDTH,
            ]
            .into_iter()
            .take(fits)
            {
                line = line.push(self.param(index).line_item());
            }
            return container(line)
                .padding([0.0, PAD])
                .height(height)
                .center_y(height)
                .into();
        }
        let column = |control: Element<'a, Message>| -> Element<'a, Message> {
            container(control)
                .width(Length::Fill)
                .max_width(KNOB_COL_W * 1.6)
                .align_x(Alignment::Center)
                .into()
        };
        let mut knobs = row![
            container(self.predelay())
                .width(PREDELAY_W)
                .align_x(Alignment::Center),
            vertical_rule()
        ]
        .spacing(0)
        .align_y(Alignment::Center);
        for index in Self::KNOBS {
            knobs = knobs.push(column(self.param(index).column(KNOB)));
        }
        container(knobs)
            .padding([4.0, PAD])
            .height(height)
            .center_y(height)
            .into()
    }

    fn advanced(&self, height: f32) -> Element<'a, Message> {
        let freeze = column![
            heading("Freeze"),
            self.param(param::FREEZE).switch("Hold tail")
        ]
        .spacing(4);
        let content: Element<'a, Message> = if height < KNOB_ROW_H {
            row![
                self.param(param::DIFFUSION).line_item(),
                self.param(param::LOW_CUT_HZ).line_item(),
                self.param(param::HIGH_CUT_HZ).line_item(),
                self.param(param::FREEZE).switch("Freeze"),
            ]
            .spacing(12)
            .align_y(Alignment::Center)
            .into()
        } else {
            row![
                self.param(param::DIFFUSION).column(KNOB - 6.0),
                vertical_rule(),
                self.param(param::LOW_CUT_HZ).column(KNOB - 6.0),
                self.param(param::HIGH_CUT_HZ).column(KNOB - 6.0),
                vertical_rule(),
                freeze,
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
