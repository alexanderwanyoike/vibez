//! Native Echo body: its repeats over the next bars beside the timing, and
//! on wide cards the wow and flutter and the output level, above the main
//! knobs and the Tape Character. Wide cards show every Advanced control in
//! one row; narrower ones page them behind tabs, and compact cards move the
//! switches onto a line under four knobs.

use iced::widget::{column, container, row, text, Space};
use iced::{Alignment, Color, Element, Length, Size};
use vibez_core::id::TrackId;
use vibez_dsp::echo::{echo_time_ms, param, ECHO_PARAMS, WOW_HZ};

use crate::domains::device_card::{CardMsg, CardPanelState};
use crate::domains::echo::{
    grid_long_name, CHARACTER_PAGE, EQ_PAGE, GRID_NAMES, MOTION_PAGE, STYLE_NAMES,
};
use crate::domains::param_entry::ValueEntry;
use crate::message::Message;
use crate::state::UiEffect;
use crate::theme as th;
use crate::widgets::device_controls::{
    disclosure, dropdown, heading, level_column, plot_band, rule, segmented, vertical_rule, Param,
    LABEL_SIZE,
};
use crate::widgets::device_layout::{stack, Stack, KNOB_ROW_H, LINE_H};
use crate::widgets::echo_display::{MotionPlot, TapsPlot};

const PAD: f32 = 10.0;
const PRESET_ROW_H: f32 = 28.0;
const KNOB: f32 = 32.0;
const KNOB_COL_W: f32 = 64.0;
const PLOT_MIN_H: f32 = 72.0;
const TABS_H: f32 = 22.0;
/// Page tabs over one row of controls.
const PAGED_H: f32 = TABS_H + 4.0 + KNOB_ROW_H;
/// A wide card's Advanced row, with room for the Feedback EQ heading.
const FULL_ADVANCED_H: f32 = KNOB_ROW_H + 14.0;
/// The Sync, Grid and Mode column beside the plot.
const TIMING_W: f32 = 168.0;
/// The wow and flutter trace and the output level beside the plot.
const SIDE_W: f32 = 300.0;
const LINE_ITEM_W: f32 = 124.0;

pub const COMPACT_W: f32 = 420.0;
/// The timing column fits beside the plot.
pub const MEDIUM_W: f32 = 640.0;
/// Wow and flutter fit beside the plot, and every Advanced control in one
/// row.
pub const FULL_W: f32 = 900.0;
pub const MAX_W: f32 = 1_200.0;

pub fn card_width(available: f32) -> f32 {
    available.clamp(COMPACT_W, MAX_W)
}

pub fn preset_bar_width(size: Size) -> f32 {
    size.width - PAD * 2.0 - 90.0
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    Compact,
    Medium,
    Full,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Arrangement {
    pub tier: Tier,
    /// The compact card's switches under its knobs, zero elsewhere.
    pub switches: f32,
    pub stack: Stack,
}

pub fn arrange(size: Size, advanced: bool) -> Arrangement {
    let tier = if size.width >= FULL_W {
        Tier::Full
    } else if size.width >= MEDIUM_W {
        Tier::Medium
    } else {
        Tier::Compact
    };
    let mut below = (size.height - PRESET_ROW_H - 1.0).max(0.0);
    let switches = if tier == Tier::Compact && below >= LINE_H * 2.0 + 1.0 {
        LINE_H
    } else {
        0.0
    };
    below -= if switches > 0.0 { switches + 1.0 } else { 0.0 };
    let advanced_h = if tier == Tier::Full {
        FULL_ADVANCED_H
    } else {
        PAGED_H
    };
    Arrangement {
        tier,
        switches,
        stack: stack(below, advanced.then_some(advanced_h), PLOT_MIN_H),
    }
}

/// A grid choice for the Grid menu.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Grid(usize);

impl std::fmt::Display for Grid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&grid_long_name(self.0))
    }
}

pub struct EchoView<'a> {
    pub track_id: TrackId,
    pub effect: &'a UiEffect,
    pub accent: Color,
    pub panel: &'a CardPanelState,
    pub entry: Option<&'a ValueEntry>,
    pub bpm: f64,
}

impl<'a> EchoView<'a> {
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
            param::LOW_CUT => param.label("Low Cut"),
            param::HIGH_CUT => param.label("High Cut"),
            _ => param,
        }
    }

    fn value(&self, index: usize) -> f32 {
        self.effect
            .params
            .get(index)
            .copied()
            .unwrap_or(ECHO_PARAMS[index].default)
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

    /// The time knob: the grid while synced, milliseconds while free.
    fn time(&self) -> Param<'a> {
        let index = if self.synced() {
            param::GRID
        } else {
            param::TIME_MS
        };
        self.param(index).label("Time")
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
            body = body.push(self.advanced(layout.tier, stack.advanced.unwrap_or(0.0), size.width));
            return body.width(Length::Fill).height(Length::Fill).into();
        }
        if stack.plots > 0.0 {
            body = body.push(self.plots(layout.tier, stack.plots)).push(rule());
        }
        body = body.push(self.main_row(layout.tier, stack.knob_row, size.width));
        if layout.switches > 0.0 {
            body = body.push(rule()).push(self.switch_line(layout.switches));
        }
        if let Some(height) = stack.advanced {
            body = body
                .push(rule())
                .push(self.advanced(layout.tier, height, size.width));
        }
        body.width(Length::Fill).height(Length::Fill).into()
    }

    fn sync_switch(&self) -> Element<'a, Message> {
        segmented(
            &[(1, "Sync"), (0, "Free")],
            usize::from(self.synced()),
            self.set(param::SYNC),
            self.accent(),
        )
    }

    fn mode_switch(&self) -> Element<'a, Message> {
        segmented(
            &[(0, "Stereo"), (1, "Ping-pong")],
            self.choice(param::MODE).min(1),
            self.set(param::MODE),
            self.accent(),
        )
    }

    fn grid_menu(&self) -> Element<'a, Message> {
        let set_grid = self.set(param::GRID);
        dropdown(
            (0..GRID_NAMES.len()).map(Grid).collect(),
            Grid(self.choice(param::GRID).min(GRID_NAMES.len() - 1)),
            move |grid| set_grid(grid.0),
        )
    }

    fn tape_character(&self) -> Element<'a, Message> {
        let styles: Vec<(usize, &'static str)> = STYLE_NAMES.iter().copied().enumerate().collect();
        segmented(
            &styles,
            self.choice(param::STYLE).min(STYLE_NAMES.len() - 1),
            self.set(param::STYLE),
            self.accent(),
        )
    }

    fn milliseconds(&self) -> String {
        let ms = echo_time_ms(&self.effect.params, self.bpm);
        if ms >= 1000.0 {
            format!("{:.2} s", ms / 1000.0)
        } else {
            format!("{ms:.0} ms")
        }
    }

    /// Sync, the grid or free time, and the stereo mode, beside the plot.
    fn timing(&self) -> Element<'a, Message> {
        let mut timing = column![heading("Sync"), self.sync_switch()].spacing(4);
        timing = if self.synced() {
            timing.push(heading("Grid")).push(self.grid_menu())
        } else {
            timing
                .push(heading("Time"))
                .push(text(self.milliseconds()).size(LABEL_SIZE).color(th::text()))
        };
        timing
            .push(heading("Mode"))
            .push(self.mode_switch())
            .width(TIMING_W)
            .into()
    }

    fn plots(&self, tier: Tier, height: f32) -> Element<'a, Message> {
        let taps = iced::widget::canvas(TapsPlot {
            params: &self.effect.params,
            bpm: self.bpm,
            accent: self.accent(),
        })
        .width(Length::Fill)
        .height(Length::Fill);
        let content: Element<'a, Message> = match tier {
            Tier::Compact => taps.into(),
            Tier::Medium => row![
                self.timing(),
                vertical_rule(),
                taps,
                vertical_rule(),
                level_column("Output", self.panel.output),
            ]
            .spacing(8)
            .into(),
            Tier::Full => {
                let depth = format!("{:.0} %", self.value(param::WOW) * 100.0);
                let motion = column![
                    heading("Wow / Flutter"),
                    iced::widget::canvas(MotionPlot {
                        params: &self.effect.params,
                        accent: self.accent(),
                    })
                    .width(Length::Fill)
                    .height(Length::Fill),
                    row![
                        heading("Rate"),
                        text(format!("{WOW_HZ:.2} Hz"))
                            .size(LABEL_SIZE)
                            .color(th::text()),
                        Space::with_width(Length::Fill),
                        heading("Depth"),
                        text(depth).size(LABEL_SIZE).color(th::text()),
                    ]
                    .spacing(6),
                ]
                .spacing(4)
                .height(Length::FillPortion(3));
                row![
                    self.timing(),
                    vertical_rule(),
                    taps,
                    vertical_rule(),
                    column![
                        motion,
                        rule(),
                        container(level_column("Output", self.panel.output))
                            .height(Length::FillPortion(2)),
                    ]
                    .spacing(4)
                    .width(SIDE_W),
                ]
                .spacing(8)
                .into()
            }
        };
        plot_band(content, height, PAD)
    }

    fn knob(&self, param: Param<'a>) -> Element<'a, Message> {
        container(param.column(KNOB))
            .width(KNOB_COL_W)
            .align_x(Alignment::Center)
            .into()
    }

    fn main_row(&self, tier: Tier, height: f32, width: f32) -> Element<'a, Message> {
        let main = [param::FEEDBACK, param::CHARACTER, param::TONE, param::MIX];
        if height < KNOB_ROW_H {
            let fits = ((width - PAD * 2.0) / LINE_ITEM_W).floor().max(0.0) as usize;
            let mut line = row![].spacing(10).align_y(Alignment::Center);
            line = line.push(self.time().line_item());
            for index in main.into_iter().take(fits.saturating_sub(1)) {
                line = line.push(self.param(index).line_item());
            }
            return container(line)
                .padding([0.0, PAD])
                .height(height)
                .center_y(height)
                .into();
        }
        let mut knobs = row![self.knob(self.time())].spacing(10);
        for index in main {
            // A compact card keeps Tone for its Tone & Motion page.
            if tier == Tier::Compact && index == param::TONE {
                continue;
            }
            knobs = knobs.push(self.knob(self.param(index)));
        }
        let mut content = row![container(knobs)
            .width(Length::Fill)
            .align_x(Alignment::Start)]
        .spacing(12)
        .align_y(Alignment::Center);
        if tier != Tier::Compact {
            content = content
                .push(vertical_rule())
                .push(column![heading("Tape Character"), self.tape_character()].spacing(4));
        }
        container(content)
            .padding([4.0, PAD])
            .height(height)
            .center_y(height)
            .into()
    }

    /// A compact card's switches, under its knobs.
    fn switch_line(&self, height: f32) -> Element<'a, Message> {
        let mut line = row![self.sync_switch()]
            .spacing(8)
            .align_y(Alignment::Center);
        if self.synced() {
            line = line.push(container(self.grid_menu()).width(84.0));
        }
        line = line.push(self.mode_switch());
        container(line)
            .padding([0.0, PAD])
            .height(height)
            .center_y(height)
            .into()
    }

    fn page(&self, page: usize, tier: Tier) -> Vec<usize> {
        match page {
            CHARACTER_PAGE => vec![
                param::WEAR,
                param::SATURATION,
                param::DIFFUSION,
                param::DUCK,
            ],
            MOTION_PAGE if tier == Tier::Compact => vec![param::TONE, param::WOW, param::FLUTTER],
            MOTION_PAGE => vec![param::WOW, param::FLUTTER],
            _ => vec![param::LOW_CUT, param::HIGH_CUT],
        }
    }

    fn advanced(&self, tier: Tier, height: f32, width: f32) -> Element<'a, Message> {
        if tier == Tier::Full && height >= FULL_ADVANCED_H {
            let mut tape = row![].spacing(8);
            for index in [
                param::WEAR,
                param::WOW,
                param::FLUTTER,
                param::SATURATION,
                param::DIFFUSION,
                param::DUCK,
            ] {
                tape = tape.push(self.knob(self.param(index)));
            }
            let eq_knob = |index| -> Element<'a, Message> {
                container(self.param(index).column(KNOB))
                    .width(KNOB_COL_W)
                    .align_x(Alignment::Center)
                    .into()
            };
            let eq = column![
                heading("Feedback EQ"),
                row![eq_knob(param::LOW_CUT), eq_knob(param::HIGH_CUT)].spacing(8),
            ]
            .spacing(0)
            .align_x(Alignment::Center);
            return container(
                row![tape, vertical_rule(), eq]
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
            &[
                (CHARACTER_PAGE, "Character"),
                (MOTION_PAGE, "Tone & Motion"),
                (EQ_PAGE, "EQ"),
            ],
            self.panel.advanced_page.min(EQ_PAGE),
            move |page| Message::Card(CardMsg::ShowAdvancedPage(effect_id, page)),
            self.accent(),
        );
        let controls = self.page(self.panel.advanced_page.min(EQ_PAGE), tier);
        if height < PAGED_H {
            // What fits beside the tabs on one line.
            let fits = ((width - PAD * 2.0 - 230.0) / LINE_ITEM_W).floor().max(0.0) as usize;
            let mut line = row![tabs].spacing(10).align_y(Alignment::Center);
            for index in controls.into_iter().take(fits) {
                line = line.push(self.param(index).line_item());
            }
            return container(line)
                .padding([0.0, PAD])
                .height(height)
                .center_y(height)
                .into();
        }
        let mut knobs = row![].spacing(8);
        if tier == Tier::Compact && self.panel.advanced_page == CHARACTER_PAGE {
            knobs =
                knobs.push(column![heading("Tape Character"), self.tape_character()].spacing(4));
        }
        for index in controls {
            knobs = knobs.push(self.knob(self.param(index)));
        }
        container(column![tabs, knobs.align_y(Alignment::Center)].spacing(4))
            .padding([4.0, PAD])
            .height(height)
            .into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_supported_size_fits_without_scrolling() {
        for width in [COMPACT_W, MEDIUM_W, FULL_W, MAX_W] {
            for height in (56..=440).step_by(2).map(|height| height as f32) {
                for advanced in [false, true] {
                    let layout = arrange(Size::new(width, height), advanced);
                    let stack = layout.stack;
                    let context = format!("{width}x{height} {advanced}: {layout:?}");
                    let mut used = PRESET_ROW_H + 1.0;
                    if layout.switches > 0.0 {
                        used += layout.switches + 1.0;
                    }
                    if stack.advanced_only {
                        used += stack.advanced.unwrap();
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
    fn each_width_takes_its_tier() {
        assert_eq!(
            arrange(Size::new(COMPACT_W, 300.0), false).tier,
            Tier::Compact
        );
        assert_eq!(
            arrange(Size::new(MEDIUM_W, 300.0), false).tier,
            Tier::Medium
        );
        let full = arrange(Size::new(FULL_W, 400.0), true);
        assert!(full.tier == Tier::Full && full.stack.advanced == Some(FULL_ADVANCED_H));
        assert!(arrange(Size::new(COMPACT_W, 300.0), false).switches > 0.0);
    }
}
