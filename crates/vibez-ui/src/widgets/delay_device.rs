//! Native Delay body: its echoes over the next beats and the output level
//! above the timing, feedback and mix. Wide cards keep the stereo mode
//! beside them and show both Advanced groups; the shortest put everything
//! on a line.

use iced::widget::{column, container, row, text, Space};
use iced::{Alignment, Color, Element, Length, Size};
use vibez_core::id::TrackId;
use vibez_dsp::delay::{line_times_ms, param, DELAY_PARAMS};

use crate::domains::delay::{DIVISION_NAMES, FEEDBACK_PAGE, MODIFIER_NAMES, STEREO_PAGE};
use crate::domains::device_card::{CardMsg, CardPanelState};
use crate::domains::param_entry::ValueEntry;
use crate::message::Message;
use crate::state::UiEffect;
use crate::theme as th;
use crate::widgets::delay_display::EchoPlot;
use crate::widgets::device_controls::{
    disclosure, dropdown, heading, level_column, plot_band, rule, segmented, vertical_rule, Param,
    LABEL_SIZE,
};
use crate::widgets::device_layout::{stack, Stack, KNOB_ROW_H};

const PAD: f32 = 10.0;
const PRESET_ROW_H: f32 = 28.0;
const KNOB: f32 = 32.0;
const KNOB_COL_W: f32 = 68.0;
const PLOT_MIN_H: f32 = 72.0;
const TABS_H: f32 = 22.0;
/// Page tabs over one row of controls.
const ADVANCED_H: f32 = TABS_H + 4.0 + KNOB_ROW_H;
const DIVISION_W: f32 = 64.0;
/// One labelled value and knob on a single-line row.
const LINE_ITEM_W: f32 = 124.0;
const LINE_SYNC_W: f32 = 210.0;

pub const COMPACT_W: f32 = 420.0;
/// The stereo mode fits beside the knobs, and both Advanced groups side by
/// side.
pub const FULL_W: f32 = 760.0;
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
    pub full: bool,
    pub stack: Stack,
}

pub fn arrange(size: Size, advanced: bool) -> Arrangement {
    let below = (size.height - PRESET_ROW_H - 1.0).max(0.0);
    let full = size.width >= FULL_W;
    let advanced_h = if full { KNOB_ROW_H } else { ADVANCED_H };
    Arrangement {
        full,
        stack: stack(below, advanced.then_some(advanced_h), PLOT_MIN_H),
    }
}

/// A choice from a name list, for the dropdowns.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Named(usize, &'static [&'static str]);

impl std::fmt::Display for Named {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.1[self.0])
    }
}

pub struct DelayView<'a> {
    pub track_id: TrackId,
    pub effect: &'a UiEffect,
    pub accent: Color,
    pub panel: &'a CardPanelState,
    pub entry: Option<&'a ValueEntry>,
    pub bpm: f64,
}

impl<'a> DelayView<'a> {
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

    fn value(&self, index: usize) -> f32 {
        self.effect
            .params
            .get(index)
            .copied()
            .unwrap_or(DELAY_PARAMS[index].default)
    }

    fn choice(&self, index: usize) -> usize {
        self.value(index).round().max(0.0) as usize
    }

    fn set(&self, index: usize) -> impl Fn(usize) -> Message {
        let (track_id, effect_id) = (self.track_id, self.effect.id);
        move |choice| Message::set_effect_param(track_id, effect_id, index, choice as f32)
    }

    fn synced(&self) -> bool {
        self.value(param::SYNC) >= 0.5
    }

    fn linked(&self) -> bool {
        self.value(param::LINK) >= 0.5
    }

    pub fn view(self, preset_bar: Element<'a, Message>, size: Size) -> Element<'a, Message> {
        let layout = arrange(size, self.panel.advanced);
        let stack = layout.stack;
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
            body = body.push(self.advanced(layout.full, height));
            return body.width(Length::Fill).height(Length::Fill).into();
        }
        if stack.plots > 0.0 {
            body = body.push(self.plots(stack.plots)).push(rule());
        }
        body = body.push(self.main_row(layout.full, stack.knob_row, size.width));
        if let Some(height) = stack.advanced {
            body = body.push(rule()).push(self.advanced(layout.full, height));
        }
        body.width(Length::Fill).height(Length::Fill).into()
    }

    fn plots(&self, height: f32) -> Element<'a, Message> {
        plot_band(
            row![
                iced::widget::canvas(EchoPlot {
                    params: &self.effect.params,
                    bpm: self.bpm,
                    accent: self.accent(),
                })
                .width(Length::Fill)
                .height(Length::Fill),
                vertical_rule(),
                level_column("Output", self.panel.output),
            ]
            .spacing(8),
            height,
            PAD,
        )
    }

    fn milliseconds(&self, right: bool) -> String {
        let ms = line_times_ms(&self.effect.params, self.bpm)[usize::from(right)];
        if ms >= 1000.0 {
            format!("{:.2} s", ms / 1000.0)
        } else {
            format!("{ms:.0} ms")
        }
    }

    fn sync_switch(&self) -> Element<'a, Message> {
        segmented(
            &[(1, "Sync"), (0, "Free")],
            usize::from(self.synced()),
            self.set(param::SYNC),
            self.accent(),
        )
    }

    /// A synced line's division, or its free time.
    fn division_or_time(&self, time: usize, division: usize) -> Element<'a, Message> {
        if !self.synced() {
            return self.param(time).value_knob();
        }
        let set_division = self.set(division);
        container(dropdown(
            (0..DIVISION_NAMES.len())
                .map(|index| Named(index, &DIVISION_NAMES))
                .collect(),
            Named(
                self.choice(division).min(DIVISION_NAMES.len() - 1),
                &DIVISION_NAMES,
            ),
            move |named| set_division(named.0),
        ))
        .width(DIVISION_W)
        .into()
    }

    fn modifier(&self, modifier: usize) -> Element<'a, Message> {
        let modifiers: Vec<(usize, &'static str)> =
            MODIFIER_NAMES.iter().copied().enumerate().collect();
        segmented(
            &modifiers,
            self.choice(modifier).min(MODIFIER_NAMES.len() - 1),
            self.set(modifier),
            self.accent(),
        )
    }

    /// One line's time: its name and length, then how it is set.
    fn line_time(
        &self,
        name: &'a str,
        right: bool,
        controls: Element<'a, Message>,
        modifier: usize,
    ) -> Element<'a, Message> {
        let mut column = column![
            row![
                heading(name),
                text(self.milliseconds(right))
                    .size(LABEL_SIZE)
                    .color(th::text_dim()),
            ]
            .spacing(6),
            controls,
        ]
        .spacing(4);
        if self.synced() {
            column = column.push(self.modifier(modifier));
        }
        column.into()
    }

    fn timing(&self) -> Element<'a, Message> {
        self.line_time(
            "Time",
            false,
            row![
                self.sync_switch(),
                self.division_or_time(param::TIME_MS, param::LEFT_DIVISION),
            ]
            .spacing(6)
            .align_y(Alignment::Center)
            .into(),
            param::LEFT_MODIFIER,
        )
    }

    fn mode(&self) -> Element<'a, Message> {
        column![
            heading("Mode"),
            segmented(
                &[(0, "Stereo"), (1, "Ping-pong")],
                self.choice(param::ROUTING).min(1),
                self.set(param::ROUTING),
                self.accent(),
            ),
        ]
        .spacing(4)
        .into()
    }

    fn link(&self) -> Element<'a, Message> {
        column![
            heading("L/R Link"),
            self.param(param::LINK).switch("Linked")
        ]
        .spacing(4)
        .into()
    }

    fn main_row(&self, full: bool, height: f32, width: f32) -> Element<'a, Message> {
        if height < KNOB_ROW_H {
            let fits = ((width - PAD * 2.0 - LINE_SYNC_W) / LINE_ITEM_W)
                .floor()
                .max(0.0) as usize;
            let mut line = row![
                self.sync_switch(),
                text(self.milliseconds(false))
                    .size(LABEL_SIZE)
                    .font(crate::typography::PERFORM_TECH),
            ]
            .spacing(10)
            .align_y(Alignment::Center);
            for index in [param::FEEDBACK, param::MIX].into_iter().take(fits) {
                line = line.push(self.param(index).line_item());
            }
            return container(line)
                .padding([0.0, PAD])
                .height(height)
                .center_y(height)
                .into();
        }
        let knob = |index| -> Element<'a, Message> {
            container(self.param(index).column(KNOB))
                .width(Length::Fill)
                .max_width(KNOB_COL_W * 1.6)
                .align_x(Alignment::Center)
                .into()
        };
        let mut content = row![
            self.timing(),
            vertical_rule(),
            knob(param::FEEDBACK),
            knob(param::MIX),
        ]
        .spacing(10)
        .align_y(Alignment::Center);
        if full {
            content = content
                .push(vertical_rule())
                .push(self.mode())
                .push(vertical_rule())
                .push(self.link());
        }
        container(content)
            .padding([4.0, PAD])
            .height(height)
            .center_y(height)
            .into()
    }

    /// The right line's own time while unlinked, and the stereo mode when
    /// the main row has no room for it.
    fn stereo_group(&self, full: bool) -> Element<'a, Message> {
        let mut group = row![].spacing(12).align_y(Alignment::Center);
        if !full {
            group = group.push(self.mode()).push(self.link());
        }
        if !self.linked() {
            group = group.push(self.line_time(
                "Right",
                true,
                self.division_or_time(param::RIGHT_TIME_MS, param::RIGHT_DIVISION),
                param::RIGHT_MODIFIER,
            ));
        } else if full {
            group = group.push(
                text("Unlink L/R to set the right line's own time")
                    .size(LABEL_SIZE)
                    .color(th::text_muted()),
            );
        }
        group.into()
    }

    /// What happens to the repeats: filtering, ducking under the input and
    /// how a time change moves.
    fn feedback_group(&self, line: bool) -> Element<'a, Message> {
        let filter = |enabled, frequency, label| -> Element<'a, Message> {
            let switch = self.param(enabled).switch(label);
            let value = self.param(frequency).value_knob();
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
        let time_change = segmented(
            &[(0, "Fade"), (1, "Glide")],
            self.choice(param::TIME_CHANGE).min(1),
            self.set(param::TIME_CHANGE),
            self.accent(),
        );
        let mut group = row![
            filter(param::FEEDBACK_HP_ENABLED, param::FEEDBACK_HP_HZ, "Low cut"),
            filter(
                param::FEEDBACK_LP_ENABLED,
                param::FEEDBACK_LP_HZ,
                "High cut"
            ),
        ]
        .spacing(12)
        .align_y(Alignment::Center);
        group = if line {
            group
                .push(self.param(param::DUCK).line_item())
                .push(time_change)
        } else {
            group
                .push(self.param(param::DUCK).column(KNOB - 6.0))
                .push(column![heading("Time change"), time_change].spacing(4))
        };
        group.into()
    }

    fn advanced(&self, full: bool, height: f32) -> Element<'a, Message> {
        if full && height >= KNOB_ROW_H {
            return container(
                row![
                    self.feedback_group(false),
                    vertical_rule(),
                    self.stereo_group(true),
                ]
                .spacing(12)
                .align_y(Alignment::Center),
            )
            .padding([4.0, PAD])
            .height(height)
            .center_y(height)
            .into();
        }
        let effect_id = self.effect.id;
        let tabs = segmented(
            &[(STEREO_PAGE, "Stereo"), (FEEDBACK_PAGE, "Feedback")],
            self.panel.advanced_page,
            move |page| Message::Card(CardMsg::ShowAdvancedPage(effect_id, page)),
            self.accent(),
        );
        let feedback = self.panel.advanced_page == FEEDBACK_PAGE;
        if height < ADVANCED_H {
            let page = if feedback {
                self.feedback_group(true)
            } else {
                self.stereo_group(full)
            };
            return container(row![tabs, page].spacing(10).align_y(Alignment::Center))
                .padding([0.0, PAD])
                .height(height)
                .center_y(height)
                .into();
        }
        let page = if feedback {
            self.feedback_group(false)
        } else {
            self.stereo_group(full)
        };
        container(column![tabs, page].spacing(4))
            .padding([4.0, PAD])
            .height(height)
            .into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::device_layout::LINE_H;

    #[test]
    fn every_supported_size_fits_without_scrolling() {
        for width in [COMPACT_W, FULL_W, MAX_W] {
            for height in (56..=420).step_by(2).map(|height| height as f32) {
                for advanced in [false, true] {
                    let stack = arrange(Size::new(width, height), advanced).stack;
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

    #[test]
    fn wide_cards_show_both_advanced_groups_in_one_row() {
        let full = arrange(Size::new(FULL_W, 400.0), true);
        assert!(full.full && full.stack.advanced == Some(KNOB_ROW_H));
        let compact = arrange(Size::new(COMPACT_W, 400.0), true);
        assert!(!compact.full && compact.stack.advanced == Some(ADVANCED_H));
    }
}
