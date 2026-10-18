//! Native Limiter body: the input and reduction over the last seconds
//! beside the output level and how much is taken off now, over the main
//! knobs. Medium and wider cards open Advanced beside the main knobs;
//! compact cards give it a row of its own, and short cards set both on
//! lines.

use std::time::{Duration, Instant};

use iced::widget::{canvas, column, container, row, text, Space};
use iced::{Alignment, Color, Element, Length, Size};
use vibez_core::id::TrackId;
use vibez_dsp::limiter::{param, LIMITER_PARAMS};

use crate::domains::device_card::{CardMsg, CardPanelState};
use crate::domains::param_entry::ValueEntry;
use crate::message::Message;
use crate::state::UiEffect;
use crate::theme as th;
use crate::widgets::device_controls::{
    disclosure, heading, plot_band, rule, switch_width, vertical_rule, Param, LABEL_SIZE, METER_W,
};
use crate::widgets::device_layout::{pack, KNOB_ROW_H, LINE_H};
use crate::widgets::limiter_display::{HistoryPlot, ReductionMeter};

const PAD: f32 = 10.0;
const PRESET_ROW_H: f32 = 28.0;
const KNOB: f32 = 32.0;
const HEADING_H: f32 = 14.0;
/// The plots under their headings, the axis labels dropped below 50 px.
const PLOT_MIN_H: f32 = HEADING_H + 4.0 + 38.0;
const REDUCTION_W: f32 = 64.0;
const LINE_GAP: f32 = 8.0;

pub const COMPACT_W: f32 = 420.0;
/// The output meter joins, and Advanced opens beside the main knobs.
pub const MEDIUM_W: f32 = 640.0;
/// Advanced fits beside the main controls even on a line.
pub const FULL_W: f32 = 900.0;
pub const MAX_W: f32 = 1_200.0;

/// The widths this card lays itself out for.
pub const WIDTHS: crate::widgets::device_chrome::CardWidths =
    crate::widgets::device_chrome::CardWidths {
        compact: COMPACT_W,
        medium: MEDIUM_W,
        expanded: FULL_W,
        max: MAX_W,
    };

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

impl Tier {
    /// How far back the history reaches on a card this wide.
    pub fn span(self) -> Duration {
        Duration::from_secs(match self {
            Tier::Compact => 4,
            Tier::Medium => 6,
            Tier::Full => 12,
        })
    }
}

/// How many lines the controls take on a card this wide when set on
/// lines: the main ones alone, Advanced's alone, and all of them together.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Lines {
    pub main: usize,
    pub advanced: usize,
    pub all: usize,
}

fn lines_h(lines: usize) -> f32 {
    lines as f32 * (LINE_H + 1.0) - 1.0
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Arrangement {
    pub tier: Tier,
    /// The plot band's height, zero when there is no room for it.
    pub plots: f32,
    /// The main controls: a row of knobs, or lines when short.
    pub main: f32,
    /// Advanced under the main controls, zero when it shares their row or
    /// lines, or is closed.
    pub extra: f32,
    /// Advanced shares the main controls' row or lines.
    pub beside: bool,
    pub advanced_only: bool,
}

pub fn arrange(size: Size, advanced: bool, lines: Lines) -> Arrangement {
    let tier = if size.width >= FULL_W {
        Tier::Full
    } else if size.width >= MEDIUM_W {
        Tier::Medium
    } else {
        Tier::Compact
    };
    let below = (size.height - PRESET_ROW_H - 1.0).max(0.0);
    let rows = |main: f32, extra: f32, beside: bool| {
        let used = main + if extra > 0.0 { extra + 1.0 } else { 0.0 };
        let rest = below - used - 1.0;
        Arrangement {
            tier,
            plots: if rest >= PLOT_MIN_H { rest } else { 0.0 },
            main,
            extra,
            beside,
            advanced_only: false,
        }
    };
    let knobs_fit = below >= KNOB_ROW_H + 1.0 + PLOT_MIN_H;
    // On lines the controls take what they need, short of the body.
    let room = ((below + 1.0) / (LINE_H + 1.0)) as usize;
    let on_lines = |count: usize| lines_h(count.min(room).max(1));
    if !advanced {
        return if knobs_fit {
            rows(KNOB_ROW_H, 0.0, false)
        } else {
            rows(on_lines(lines.main), 0.0, false)
        };
    }
    if tier != Tier::Compact && knobs_fit {
        return rows(KNOB_ROW_H, 0.0, true);
    }
    if below >= (KNOB_ROW_H + 1.0) * 2.0 + PLOT_MIN_H {
        return rows(KNOB_ROW_H, KNOB_ROW_H, false);
    }
    let apart = lines_h(lines.main) + 1.0 + lines_h(lines.advanced);
    if below >= apart {
        return rows(lines_h(lines.main), lines_h(lines.advanced), false);
    }
    if below >= lines_h(lines.all) {
        // Together, the main controls lead and Advanced follows on.
        return rows(lines_h(lines.all), 0.0, true);
    }
    Arrangement {
        tier,
        plots: 0.0,
        main: 0.0,
        extra: on_lines(lines.all),
        beside: false,
        advanced_only: true,
    }
}

pub struct LimiterView<'a> {
    pub track_id: TrackId,
    pub effect: &'a UiEffect,
    pub accent: Color,
    pub panel: &'a CardPanelState,
    pub entry: Option<&'a ValueEntry>,
    pub now: Instant,
}

impl<'a> LimiterView<'a> {
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
            .unwrap_or(LIMITER_PARAMS[index].default)
    }

    fn on(&self, index: usize) -> bool {
        self.value(index) >= 0.5
    }

    /// The ceiling's unit: true peak when the limiter measures between
    /// samples.
    fn ceiling_unit(&self) -> &'static str {
        if self.on(param::TRUE_PEAK) {
            "dBTP"
        } else {
            "dB"
        }
    }

    fn main_controls(&self, knobs: bool) -> Vec<Control<'a>> {
        [
            param::INPUT_DB,
            param::CEILING_DB,
            param::RELEASE_MS,
            param::STYLE,
        ]
        .into_iter()
        .map(|index| {
            if knobs {
                Control::Knob(index)
            } else {
                Control::Line(index)
            }
        })
        .collect()
    }

    fn advanced_controls(&self, knobs: bool) -> Vec<Control<'a>> {
        let mut controls: Vec<Control<'a>> = [param::LOOKAHEAD_MS, param::STEREO_LINK]
            .into_iter()
            .map(|index| {
                if knobs {
                    Control::Knob(index)
                } else {
                    Control::Line(index)
                }
            })
            .collect();
        for (index, name) in [(param::QUALITY, "Quality"), (param::TRUE_PEAK, "True Peak")] {
            controls.push(if knobs {
                Control::HeadedSwitch(index, name)
            } else {
                Control::Switch(index, name)
            });
        }
        controls
    }

    fn widths(&self, controls: &[Control<'a>]) -> Vec<f32> {
        controls.iter().map(|control| control.width(self)).collect()
    }

    /// How many lines the controls take across `width`.
    fn lines(&self, width: f32) -> Lines {
        let main = self.widths(&self.main_controls(false));
        let advanced = self.widths(&self.advanced_controls(false));
        let all: Vec<f32> = main.iter().chain(&advanced).copied().collect();
        Lines {
            main: pack(&main, width, LINE_GAP).len(),
            advanced: pack(&advanced, width, LINE_GAP).len(),
            all: pack(&all, width, LINE_GAP).len(),
        }
    }

    pub fn view(self, preset_bar: Element<'a, Message>, size: Size) -> Element<'a, Message> {
        let inner = size.width - PAD * 2.0;
        let layout = arrange(size, self.panel.advanced, self.lines(inner));
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
            // What the closed card's line could not hold leads Advanced.
            let shown = pack(&self.widths(&self.main_controls(false)), inner, LINE_GAP)
                .first()
                .map_or(0, |line| line.end);
            let mut controls: Vec<Control<'a>> =
                self.main_controls(false).into_iter().skip(shown).collect();
            controls.extend(self.advanced_controls(false));
            body = body.push(self.lines_of(controls, layout.extra, inner));
            return body.width(Length::Fill).height(Length::Fill).into();
        }
        if layout.plots > 0.0 {
            body = body.push(self.plots(layout)).push(rule());
        }
        let knobs = layout.main >= KNOB_ROW_H;
        let mut main = self.main_controls(knobs);
        if layout.beside {
            if knobs {
                main.push(Control::Rule);
            }
            main.extend(self.advanced_controls(knobs));
        }
        body = body.push(if knobs {
            self.knob_row(main)
        } else {
            self.lines_of(main, layout.main, inner)
        });
        if layout.extra > 0.0 {
            let knobs = layout.extra >= KNOB_ROW_H;
            let advanced = self.advanced_controls(knobs);
            body = body.push(rule()).push(if knobs {
                self.knob_row(advanced)
            } else {
                self.lines_of(advanced, layout.extra, inner)
            });
        }
        body.width(Length::Fill).height(Length::Fill).into()
    }

    fn knob_row(&self, controls: Vec<Control<'a>>) -> Element<'a, Message> {
        let mut knobs = row![].spacing(6).align_y(Alignment::Center);
        for control in &controls {
            knobs = knobs.push(control.element(self));
        }
        container(knobs.width(Length::Fill))
            .padding([4.0, PAD])
            .height(KNOB_ROW_H)
            .center_y(KNOB_ROW_H)
            .into()
    }

    /// `controls` set on as many lines as fit `height`.
    fn lines_of(
        &self,
        controls: Vec<Control<'a>>,
        height: f32,
        width: f32,
    ) -> Element<'a, Message> {
        let lines = pack(&self.widths(&controls), width, LINE_GAP);
        let fits = (((height + 1.0) / (LINE_H + 1.0)) as usize).max(1);
        let mut column = column![].spacing(1);
        for line in lines.into_iter().take(fits) {
            let mut items = row![].spacing(LINE_GAP).align_y(Alignment::Center);
            for control in &controls[line] {
                items = items.push(control.element(self));
            }
            column = column.push(container(items).height(LINE_H).center_y(LINE_H));
        }
        container(column)
            .padding([0.0, PAD])
            .height(height)
            .center_y(height)
            .into()
    }

    fn plots(&self, layout: Arrangement) -> Element<'a, Message> {
        let ceiling = format!(
            "{:.1} {}",
            self.value(param::CEILING_DB),
            self.ceiling_unit()
        );
        let history = column![
            row![
                heading("Input / Gain Reduction"),
                Space::with_width(Length::Fill),
                text("Ceiling").size(LABEL_SIZE).color(self.accent()),
                text(ceiling).size(LABEL_SIZE).color(self.accent()),
            ]
            .spacing(6)
            .align_y(Alignment::Center)
            .height(HEADING_H),
            canvas(HistoryPlot {
                history: &self.panel.dynamics,
                now: self.now,
                span: layout.tier.span(),
                ceiling_db: self.value(param::CEILING_DB),
                accent: self.accent(),
            })
            .width(Length::Fill)
            .height(Length::Fill),
        ]
        .spacing(4)
        .width(Length::Fill);
        let mut content = row![history].spacing(8);
        if layout.tier != Tier::Compact {
            let output = self.panel.output;
            let peak = vibez_core::level::amplitude_to_db(output.peak_l.max(output.peak_r));
            let readout = if peak > -120.0 {
                format!("{peak:.1}")
            } else {
                "-inf".into()
            };
            content = content.push(vertical_rule()).push(
                column![
                    row![
                        heading("Output"),
                        Space::with_width(Length::Fill),
                        text(readout).size(LABEL_SIZE).color(th::text()),
                    ]
                    .height(HEADING_H)
                    .align_y(Alignment::Center),
                    canvas(crate::widgets::level_meter::LevelMeter { levels: output })
                        .width(crate::widgets::level_meter::WIDTH)
                        .height(Length::Fill),
                ]
                .spacing(4)
                .width(METER_W),
            );
        }
        content = content.push(vertical_rule()).push(
            column![
                row![
                    heading("GR"),
                    Space::with_width(Length::Fill),
                    text(format!("{:.1} dB", self.panel.reduction_db))
                        .size(LABEL_SIZE)
                        .color(th::text()),
                ]
                .height(HEADING_H)
                .align_y(Alignment::Center),
                canvas(ReductionMeter {
                    reduction_db: self.panel.reduction_db,
                    accent: self.accent(),
                })
                .width(Length::Fill)
                .height(Length::Fill),
            ]
            .spacing(4)
            .width(REDUCTION_W),
        );
        plot_band(content, layout.plots, PAD)
    }
}

/// One control in a row of them.
enum Control<'a> {
    Knob(usize),
    Line(usize),
    Switch(usize, &'a str),
    /// A switch under its name, reading On or Off.
    HeadedSwitch(usize, &'a str),
    Rule,
}

impl<'a> Control<'a> {
    /// How wide this sets on a line.
    fn width(&self, view: &LimiterView<'a>) -> f32 {
        match *self {
            Control::Line(index) | Control::Knob(index) => view.param(index).line_width(),
            Control::Switch(_, name) | Control::HeadedSwitch(_, name) => switch_width(name),
            Control::Rule => 1.0,
        }
    }

    fn element(&self, view: &LimiterView<'a>) -> Element<'a, Message> {
        match *self {
            // Style's names are longer than any knob's value, so it keeps
            // the room they need.
            Control::Knob(param::STYLE) => {
                let param = view.param(param::STYLE);
                container(param.column(KNOB))
                    .width(param.value_width().max(KNOB) + 8.0)
                    .align_x(Alignment::Center)
                    .into()
            }
            Control::Knob(index) => container(view.param(index).column(KNOB))
                .width(Length::Fill)
                .max_width(110.0)
                .align_x(Alignment::Center)
                .into(),
            Control::Line(index) => view.param(index).line_item(),
            Control::Switch(index, name) => view.param(index).switch(name),
            Control::HeadedSwitch(index, name) => {
                let state = if view.on(index) { "On" } else { "Off" };
                container(column![heading(name), view.param(index).switch(state)].spacing(4))
                    .width(Length::Fill)
                    .max_width(90.0)
                    .align_x(Alignment::Center)
                    .into()
            }
            Control::Rule => vertical_rule(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Below the preset row in the default 280 px detail panel and in its
    /// 180 px minimum, as measured in the app.
    const DEFAULT_BELOW: f32 = 135.0;
    const MINIMUM_BELOW: f32 = 35.0;
    /// About how wide each control sets on a line, at its widest values:
    /// Input, Ceiling, Release, Style, then Lookahead, Stereo Link,
    /// Quality and True Peak.
    const MAIN_WIDTHS: [f32; 4] = [90.0, 105.0, 90.0, 120.0];
    const ADVANCED_WIDTHS: [f32; 4] = [100.0, 105.0, 62.0, 72.0];

    fn lines(width: f32) -> Lines {
        let inner = width - PAD * 2.0;
        let all: Vec<f32> = MAIN_WIDTHS
            .iter()
            .chain(&ADVANCED_WIDTHS)
            .copied()
            .collect();
        Lines {
            main: pack(&MAIN_WIDTHS, inner, LINE_GAP).len(),
            advanced: pack(&ADVANCED_WIDTHS, inner, LINE_GAP).len(),
            all: pack(&all, inner, LINE_GAP).len(),
        }
    }

    fn layout(width: f32, height: f32, advanced: bool) -> Arrangement {
        arrange(Size::new(width, height), advanced, lines(width))
    }

    fn at(width: f32, below: f32, advanced: bool) -> Arrangement {
        layout(width, below + PRESET_ROW_H + 1.0, advanced)
    }

    fn used(layout: Arrangement) -> f32 {
        let mut used = PRESET_ROW_H + 1.0;
        if layout.advanced_only {
            return used + layout.extra;
        }
        if layout.plots > 0.0 {
            used += layout.plots + 1.0;
        }
        used += layout.main;
        if layout.extra > 0.0 {
            used += layout.extra + 1.0;
        }
        used
    }

    #[test]
    fn every_supported_size_fits_without_scrolling() {
        for width in [COMPACT_W, 460.0, MEDIUM_W, FULL_W, MAX_W] {
            let lowest = (MINIMUM_BELOW + PRESET_ROW_H + 1.0) as usize;
            for height in (lowest..=460).step_by(2).map(|height| height as f32) {
                for advanced in [false, true] {
                    let layout = layout(width, height, advanced);
                    let context = format!("{width}x{height} {advanced}: {layout:?}");
                    assert!(used(layout) <= height + 0.01, "{context}");
                    assert!(
                        layout.plots == 0.0 || layout.plots >= PLOT_MIN_H,
                        "{context}"
                    );
                    if !layout.advanced_only {
                        assert!(layout.main >= LINE_H, "{context}");
                    }
                }
            }
        }
    }

    #[test]
    fn the_default_panel_keeps_the_plot_and_the_knobs_with_advanced_open_or_not() {
        for width in [MEDIUM_W, FULL_W] {
            for advanced in [false, true] {
                let layout = at(width, DEFAULT_BELOW, advanced);
                assert!(
                    layout.plots > 0.0 && layout.main == KNOB_ROW_H,
                    "{width} {advanced}: {layout:?}"
                );
                assert_eq!(layout.beside, advanced);
            }
        }
        let compact = at(COMPACT_W, DEFAULT_BELOW, false);
        assert!(
            compact.plots > 0.0 && compact.main == KNOB_ROW_H,
            "{compact:?}"
        );
        // Compact opens Advanced on lines of its own and keeps the plot.
        let open = at(COMPACT_W, DEFAULT_BELOW, true);
        assert!(open.plots > 0.0 && open.extra == LINE_H, "{open:?}");
        assert_eq!(open.main, lines_h(lines(COMPACT_W).main));
    }

    #[test]
    fn the_minimum_panel_keeps_a_line_of_controls() {
        for width in [COMPACT_W, MEDIUM_W, FULL_W] {
            let closed = at(width, MINIMUM_BELOW, false);
            assert_eq!(closed.main, LINE_H, "{width}: {closed:?}");
            let open = at(width, MINIMUM_BELOW, true);
            assert!(open.advanced_only || open.beside, "{width}: {open:?}");
        }
    }

    #[test]
    fn the_history_reaches_further_back_on_wider_cards() {
        assert!(Tier::Compact.span() < Tier::Medium.span());
        assert_eq!(
            Tier::Full.span(),
            crate::domains::device_card::DYNAMICS_HISTORY
        );
    }
}
