//! Native Gate body: input and output levels, the history of what the gate
//! heard and did, and its state above a row of knobs. Wide cards show both
//! Advanced groups and a legend; the shortest put the knobs on a line.

use std::time::Instant;

use iced::widget::{canvas, column, container, row, text, Space};
use iced::{Alignment, Color, Element, Length, Size};
use vibez_core::effect::GatePhase;
use vibez_core::id::TrackId;
use vibez_dsp::gate::param;

use crate::domains::gate::{AdvancedPage, GateMsg, GatePanelState};
use crate::domains::param_entry::ValueEntry;
use crate::message::Message;
use crate::state::UiEffect;
use crate::theme as th;
use crate::widgets::device_controls::{
    disclosure, heading, level_column, plot_band, rule, segmented, vertical_rule, Param, LABEL_SIZE,
};
use crate::widgets::device_layout::{stack, KNOB_ROW_H};
use crate::widgets::gate_display::GateHistory;

const PAD: f32 = 10.0;
const PRESET_ROW_H: f32 = 28.0;
const KNOB: f32 = 32.0;
const KNOB_COL_W: f32 = 68.0;
const PLOT_MIN_H: f32 = 72.0;
const TABS_H: f32 = 22.0;
/// Page tabs over one row of controls.
const ADVANCED_H: f32 = TABS_H + 4.0 + KNOB_ROW_H;
/// The legend with the state under it.
const LEGEND_W: f32 = 110.0;
/// Width of one advanced group's row.
const GROUP_W: f32 = 320.0;
/// One labelled value and knob on a single-line knob row.
const LINE_ITEM_W: f32 = 124.0;
const LINE_STATE_W: f32 = 70.0;

pub const COMPACT_W: f32 = 420.0;
/// Both advanced groups side by side, and the legend beside the history.
pub const FULL_W: f32 = GROUP_W * 2.0 + 1.0 + PAD * 2.0 + 16.0;
pub const MAX_W: f32 = 1000.0;

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

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Arrangement {
    /// Both advanced groups and the legend fit.
    pub full: bool,
    pub plots: f32,
    pub knob_row: f32,
    pub advanced: Option<f32>,
    pub advanced_only: bool,
}

pub fn arrange(size: Size, advanced: bool) -> Arrangement {
    let below = (size.height - PRESET_ROW_H - 1.0).max(0.0);
    let full = size.width >= FULL_W;
    let advanced_h = if full { KNOB_ROW_H } else { ADVANCED_H };
    let stack = stack(below, advanced.then_some(advanced_h), PLOT_MIN_H);
    Arrangement {
        full,
        plots: stack.plots,
        knob_row: stack.knob_row,
        advanced: stack.advanced,
        advanced_only: stack.advanced_only,
    }
}

fn state_name(phase: GatePhase) -> &'static str {
    match phase {
        GatePhase::Open => "OPEN",
        GatePhase::Hold => "HOLD",
        GatePhase::Closed => "CLOSED",
    }
}

pub struct GateView<'a> {
    pub track_id: TrackId,
    pub effect: &'a UiEffect,
    pub accent: Color,
    pub panel: &'a GatePanelState,
    pub entry: Option<&'a ValueEntry>,
}

impl<'a> GateView<'a> {
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

    fn on(&self, index: usize) -> bool {
        self.effect.params.get(index).copied().unwrap_or(0.0) >= 0.5
    }

    pub fn view(self, preset_bar: Element<'a, Message>, size: Size) -> Element<'a, Message> {
        let layout = arrange(size, self.panel.advanced);
        let top = row![
            preset_bar,
            Space::with_width(Length::Fill),
            disclosure(
                self.panel.advanced,
                Message::Gate(GateMsg::ToggleAdvanced(self.effect.id)),
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
            body = body.push(self.advanced(layout, height));
            return body.width(Length::Fill).height(Length::Fill).into();
        }
        if layout.plots > 0.0 {
            body = body.push(self.plots(layout)).push(rule());
        }
        body = body.push(self.knob_row(layout, size.width));
        if let Some(height) = layout.advanced {
            body = body.push(rule()).push(self.advanced(layout, height));
        }
        body.width(Length::Fill).height(Length::Fill).into()
    }

    /// Without room for the state column, the state reads in the
    /// history's heading.
    fn history(&self, state_inline: bool) -> Element<'a, Message> {
        let mut title = row![heading("Input level (dB)"), Space::with_width(Length::Fill)]
            .align_y(Alignment::Center);
        if state_inline {
            title = title.push(self.state_text(LABEL_SIZE + 1.0));
        }
        column![
            title,
            canvas(GateHistory {
                track_id: self.track_id,
                effect_id: self.effect.id,
                params: &self.effect.params,
                readings: &self.panel.history,
                now: self.panel.last_meter_at.unwrap_or_else(Instant::now),
                accent: self.accent(),
            })
            .width(Length::Fill)
            .height(Length::Fill),
        ]
        .spacing(2)
        .width(Length::Fill)
        .into()
    }

    fn state_text(&self, size: f32) -> Element<'a, Message> {
        let phase = self.panel.phase;
        text(state_name(phase))
            .size(size)
            .font(crate::typography::PERFORM_TECH)
            .color(if phase == GatePhase::Closed {
                th::text_muted()
            } else {
                self.accent()
            })
            .wrapping(text::Wrapping::None)
            .into()
    }

    fn swatch(&self, color: Color, name: &'a str) -> Element<'a, Message> {
        row![
            container(Space::new(12.0, 3.0)).style(move |_| container::Style {
                background: Some(color.into()),
                ..Default::default()
            }),
            text(name).size(LABEL_SIZE).color(th::text_dim()),
        ]
        .spacing(6)
        .align_y(Alignment::Center)
        .into()
    }

    fn state(&self) -> Element<'a, Message> {
        let accent = self.accent();
        column![
            self.swatch(th::text_dim(), "Input"),
            self.swatch(accent, "Gate open"),
            self.swatch(Color { a: 0.4, ..accent }, "Attenuation"),
            Space::with_height(8),
            heading("State"),
            self.state_text(16.0),
        ]
        .spacing(4)
        .width(LEGEND_W)
        .into()
    }

    fn plots(&self, layout: Arrangement) -> Element<'a, Message> {
        let mut content = row![
            level_column("Input", self.panel.input),
            level_column("Output", self.panel.output),
            vertical_rule(),
            self.history(!layout.full),
        ]
        .spacing(8);
        if layout.full {
            content = content.push(vertical_rule()).push(self.state());
        }
        plot_band(content, layout.plots, PAD)
    }

    const MAIN: [usize; 5] = [
        param::THRESHOLD,
        param::ATTACK,
        param::HOLD,
        param::RELEASE,
        param::RANGE,
    ];

    /// Range only applies while its floor is on, so its switch is its name.
    fn range_column(&self) -> Element<'a, Message> {
        let param = self.param(param::RANGE);
        column![
            canvas(param.knob()).width(KNOB).height(KNOB),
            self.param(param::RANGE_ENABLED).switch("Range"),
            param.value_entry(),
        ]
        .spacing(2)
        .align_x(Alignment::Center)
        .into()
    }

    fn knob_row(&self, layout: Arrangement, width: f32) -> Element<'a, Message> {
        if layout.knob_row < KNOB_ROW_H {
            // Threshold comes first, so it stays while there is room for one.
            let fits = ((width - PAD * 2.0 - LINE_STATE_W) / LINE_ITEM_W)
                .floor()
                .max(1.0) as usize;
            let mut line = row![].spacing(10).align_y(Alignment::Center);
            for index in [
                param::THRESHOLD,
                param::RELEASE,
                param::HOLD,
                param::ATTACK,
                param::RANGE,
            ]
            .into_iter()
            .take(fits)
            {
                line = line.push(self.param(index).line_item());
            }
            return container(
                line.push(Space::with_width(Length::Fill))
                    .push(self.state_text(LABEL_SIZE + 1.0)),
            )
            .padding([0.0, PAD])
            .height(layout.knob_row)
            .center_y(layout.knob_row)
            .into();
        }
        let mut knobs = row![].spacing(0).align_y(Alignment::Center);
        for index in Self::MAIN {
            let control = if index == param::RANGE {
                self.range_column()
            } else {
                self.param(index).column(KNOB)
            };
            knobs = knobs.push(
                container(control)
                    .width(Length::Fill)
                    .max_width(KNOB_COL_W * 1.6)
                    .align_x(Alignment::Center),
            );
        }
        container(knobs)
            .padding([4.0, PAD])
            .height(layout.knob_row)
            .center_y(layout.knob_row)
            .into()
    }

    fn expanding(&self) -> bool {
        self.on(param::MODE)
    }

    /// How the gate opens and closes: hysteresis, gating or expanding, and
    /// its blend with the dry signal.
    fn response_group(&self) -> Element<'a, Message> {
        let (track_id, effect_id) = (self.track_id, self.effect.id);
        let mut group = row![
            self.param(param::HYSTERESIS).column(KNOB - 6.0),
            column![
                heading("Mode"),
                segmented(
                    &[(false, "Gate"), (true, "Expand")],
                    self.expanding(),
                    move |expand| Message::set_effect_param(
                        track_id,
                        effect_id,
                        param::MODE,
                        if expand { 1.0 } else { 0.0 },
                    ),
                    self.accent(),
                ),
            ]
            .spacing(4),
        ]
        .spacing(10)
        .align_y(Alignment::Center);
        if self.expanding() {
            group = group.push(self.param(param::EXPANSION_RATIO).column(KNOB - 6.0));
        }
        group.push(self.param(param::MIX).column(KNOB - 6.0)).into()
    }

    fn detector_group(&self) -> Element<'a, Message> {
        let filter = |enabled, frequency, label| -> Element<'a, Message> {
            column![
                self.param(enabled).switch(label),
                self.param(frequency).value_knob()
            ]
            .spacing(4)
            .align_x(Alignment::Center)
            .into()
        };
        row![
            filter(param::HP_ENABLED, param::HP_FREQUENCY, "Low cut"),
            filter(param::LP_ENABLED, param::LP_FREQUENCY, "High cut"),
            column![
                heading("Detector"),
                self.param(param::DETECTOR_LISTEN).switch("Audition")
            ]
            .spacing(4),
        ]
        .spacing(10)
        .align_y(Alignment::Center)
        .into()
    }

    fn advanced(&self, layout: Arrangement, height: f32) -> Element<'a, Message> {
        if layout.full && height >= KNOB_ROW_H {
            return container(
                row![
                    container(self.response_group()).width(GROUP_W),
                    vertical_rule(),
                    self.detector_group(),
                ]
                .spacing(8)
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
                (AdvancedPage::Response, "Response"),
                (AdvancedPage::Detector, "Detector"),
            ],
            self.panel.advanced_page,
            move |page| Message::Gate(GateMsg::ShowAdvancedPage(effect_id, page)),
            self.accent(),
        );
        if height < ADVANCED_H {
            return container(
                row![tabs, self.advanced_line()]
                    .spacing(10)
                    .align_y(Alignment::Center),
            )
            .padding([0.0, PAD])
            .height(height)
            .center_y(height)
            .into();
        }
        let page = match self.panel.advanced_page {
            AdvancedPage::Response => self.response_group(),
            AdvancedPage::Detector => self.detector_group(),
        };
        container(column![tabs, page].spacing(4))
            .padding([4.0, PAD])
            .height(height)
            .into()
    }

    /// The most used controls of the open page, for the shortest cards.
    fn advanced_line(&self) -> Element<'a, Message> {
        match self.panel.advanced_page {
            AdvancedPage::Response => row![
                self.param(param::HYSTERESIS).line_item(),
                self.param(param::MIX).line_item(),
            ],
            AdvancedPage::Detector => row![
                self.param(param::HP_ENABLED).switch("Low cut"),
                self.param(param::HP_FREQUENCY).value_knob(),
                self.param(param::DETECTOR_LISTEN).switch("Audition"),
            ],
        }
        .spacing(8)
        .align_y(Alignment::Center)
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
        for width in [COMPACT_W, FULL_W, MAX_W] {
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
    fn wide_cards_show_both_advanced_groups_in_one_row() {
        let full = arrange(Size::new(FULL_W, 400.0), true);
        assert!(full.full && full.advanced == Some(KNOB_ROW_H) && full.plots > 0.0);
        let compact = arrange(Size::new(COMPACT_W, 400.0), true);
        assert!(!compact.full && compact.advanced == Some(ADVANCED_H));
    }

    #[test]
    fn card_width_grows_with_the_section_up_to_its_maximum() {
        assert_eq!(card_width(300.0), COMPACT_W);
        assert_eq!(card_width(700.0), 700.0);
        assert_eq!(card_width(5000.0), MAX_W);
    }
}
