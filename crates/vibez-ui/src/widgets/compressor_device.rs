//! Native Compressor body: input level, the transfer curve, reduction over
//! time and the reduction meter above a row of knobs. Narrow cards switch
//! between the curve and the history; the shortest put the knobs on a line.

use std::time::Instant;

use iced::widget::{canvas, column, container, row, text, Space};
use iced::{Alignment, Color, Element, Length, Size};
use vibez_core::id::TrackId;
use vibez_dsp::compressor::param;

use crate::domains::compressor::{AdvancedPage, CompressorMsg, CompressorPanelState, Page};
use crate::domains::param_entry::ValueEntry;
use crate::message::Message;
use crate::state::UiEffect;
use crate::theme as th;
use crate::widgets::compressor_display::{ReductionHistory, ReductionMeter, TransferPlot};
use crate::widgets::device_controls::{
    disclosure, heading, level_column, plot_band, rule, segmented, vertical_rule, Param,
    LABEL_SIZE, METER_W,
};
use crate::widgets::device_layout::{stack, KNOB_ROW_H};

const PAD: f32 = 10.0;
const PRESET_ROW_H: f32 = 28.0;
const KNOB: f32 = 32.0;
const KNOB_COL_W: f32 = 68.0;
const PLOT_MIN_H: f32 = 72.0;
const TABS_H: f32 = 22.0;
/// Page tabs over one row of controls.
const ADVANCED_H: f32 = TABS_H + 4.0 + KNOB_ROW_H;
const GR_W: f32 = 64.0;
const HISTORY_W: f32 = 190.0;
const TRANSFER_MIN_W: f32 = 220.0;
/// Width of one advanced group's row.
const GROUP_W: f32 = 380.0;
/// One labelled value and knob on a single-line knob row.
const LINE_ITEM_W: f32 = 124.0;
const LINE_READOUT_W: f32 = 80.0;

pub const COMPACT_W: f32 = 420.0;
/// Seven knobs, and the curve and history side by side.
pub const MEDIUM_W: f32 =
    METER_W + TRANSFER_MIN_W + HISTORY_W + GR_W + 3.0 * 8.0 + PAD * 2.0 + 30.0;
/// Every knob, Auto included, and both advanced groups side by side.
pub const FULL_W: f32 = GROUP_W * 2.0 + 1.0 + PAD * 2.0 + 8.0;
pub const MAX_W: f32 = 1000.0;

pub const WIDTHS: crate::widgets::device_chrome::CardWidths =
    crate::widgets::device_chrome::CardWidths {
        compact: COMPACT_W,
        medium: MEDIUM_W,
        expanded: FULL_W,
        max: MAX_W,
    };

/// Which knobs the main row carries; the rest live in Advanced.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Knobs {
    /// Threshold to Mix, with Auto release.
    All,
    /// Threshold to Mix.
    Seven,
    /// Threshold, Ratio, Attack and Release.
    Four,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Arrangement {
    pub knobs: Knobs,
    /// Height of the plots, zero when there is no room for them.
    pub plots: f32,
    /// The knob row's height: full knobs, or a single line.
    pub knob_row: f32,
    pub advanced: Option<f32>,
    pub advanced_only: bool,
    /// Both advanced groups fit side by side.
    pub groups_beside: bool,
}

pub fn arrange(size: Size, advanced: bool) -> Arrangement {
    let below = (size.height - PRESET_ROW_H - 1.0).max(0.0);
    let knobs = if size.width >= FULL_W {
        Knobs::All
    } else if size.width >= MEDIUM_W {
        Knobs::Seven
    } else {
        Knobs::Four
    };
    let groups_beside = size.width >= FULL_W;
    let advanced_h = if groups_beside {
        KNOB_ROW_H
    } else {
        ADVANCED_H
    };
    let stack = stack(below, advanced.then_some(advanced_h), PLOT_MIN_H);
    Arrangement {
        knobs,
        plots: stack.plots,
        knob_row: stack.knob_row,
        advanced: stack.advanced,
        advanced_only: stack.advanced_only,
        groups_beside,
    }
}

pub fn preset_bar_width(size: Size) -> f32 {
    size.width - PAD * 2.0 - 90.0
}

pub struct CompressorView<'a> {
    pub track_id: TrackId,
    pub effect: &'a UiEffect,
    pub accent: Color,
    pub panel: &'a CompressorPanelState,
    pub entry: Option<&'a ValueEntry>,
}

impl<'a> CompressorView<'a> {
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

    pub fn view(self, preset_bar: Element<'a, Message>, size: Size) -> Element<'a, Message> {
        let layout = arrange(size, self.panel.advanced);
        let top = row![
            preset_bar,
            Space::with_width(Length::Fill),
            self.disclosure()
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

    fn transfer(&self) -> Element<'a, Message> {
        let range = self
            .effect
            .descriptors
            .get(param::THRESHOLD)
            .map_or((-60.0, 0.0), |d| (d.min, d.max));
        column![
            heading("Output (dB)"),
            canvas(TransferPlot {
                track_id: self.track_id,
                effect_id: self.effect.id,
                params: &self.effect.params,
                accent: self.accent(),
                threshold_range: range,
            })
            .width(Length::Fill)
            .height(Length::Fill),
        ]
        .spacing(2)
        .width(Length::Fill)
        .into()
    }

    fn history(&self) -> Element<'a, Message> {
        column![
            heading("Gain reduction (dB)"),
            canvas(ReductionHistory {
                points: &self.panel.history,
                now: self.panel.last_meter_at.unwrap_or_else(Instant::now),
                accent: self.accent(),
            })
            .width(Length::Fill)
            .height(Length::Fill),
        ]
        .spacing(2)
        .into()
    }

    fn reduction(&self) -> Element<'a, Message> {
        column![
            row![
                heading("GR"),
                text(format!("{:.1} dB", self.panel.reduction_db))
                    .size(11)
                    .font(crate::typography::PERFORM_TECH)
                    .wrapping(text::Wrapping::None),
            ]
            .spacing(6),
            canvas(ReductionMeter {
                db: self.panel.reduction_db,
                accent: self.accent(),
            })
            .width(Length::Fill)
            .height(Length::Fill),
        ]
        .spacing(2)
        .width(GR_W)
        .into()
    }

    fn plots(&self, layout: Arrangement) -> Element<'a, Message> {
        let content: Element<'a, Message> = if layout.knobs == Knobs::Four {
            let effect_id = self.effect.id;
            let shown = match self.panel.page {
                Page::Transfer => self.transfer(),
                Page::History => self.history(),
            };
            column![
                segmented(
                    &[(Page::Transfer, "Transfer"), (Page::History, "History")],
                    self.panel.page,
                    move |page| Message::Compressor(CompressorMsg::ShowPage(effect_id, page)),
                    self.accent(),
                ),
                row![
                    container(shown).width(Length::Fill),
                    vertical_rule(),
                    self.reduction()
                ]
                .spacing(8),
            ]
            .spacing(4)
            .into()
        } else {
            row![
                level_column("Input", self.panel.input),
                vertical_rule(),
                self.transfer(),
                vertical_rule(),
                container(self.history()).width(HISTORY_W),
                vertical_rule(),
                self.reduction(),
            ]
            .spacing(8)
            .into()
        };
        plot_band(content, layout.plots, PAD)
    }

    fn main_indices(knobs: Knobs) -> &'static [usize] {
        match knobs {
            Knobs::All | Knobs::Seven => &[
                param::THRESHOLD,
                param::RATIO,
                param::ATTACK,
                param::RELEASE,
                param::KNEE,
                param::MAKEUP,
                param::MIX,
            ],
            Knobs::Four => &[
                param::THRESHOLD,
                param::RATIO,
                param::ATTACK,
                param::RELEASE,
            ],
        }
    }

    fn knob_row(&self, layout: Arrangement, width: f32) -> Element<'a, Message> {
        let indices = Self::main_indices(layout.knobs);
        if layout.knob_row < KNOB_ROW_H {
            // The line keeps the leading controls that fit; Threshold and
            // Ratio come first.
            let fits = ((width - PAD * 2.0 - LINE_READOUT_W) / LINE_ITEM_W)
                .floor()
                .max(1.0) as usize;
            let mut line = row![].spacing(10).align_y(Alignment::Center);
            for &index in indices.iter().take(fits) {
                line = line.push(
                    row![
                        text(self.param(index).name())
                            .size(LABEL_SIZE)
                            .color(th::text_dim()),
                        self.param(index).value_knob(),
                    ]
                    .spacing(4)
                    .align_y(Alignment::Center),
                );
            }
            return container(
                line.push(Space::with_width(Length::Fill)).push(
                    text(format!("GR {:.1} dB", self.panel.reduction_db))
                        .size(LABEL_SIZE)
                        .font(crate::typography::PERFORM_TECH),
                ),
            )
            .padding([0.0, PAD])
            .height(layout.knob_row)
            .center_y(layout.knob_row)
            .into();
        }
        let mut knobs = row![].spacing(0).align_y(Alignment::Center);
        for &index in indices {
            if index == param::KNEE && layout.knobs == Knobs::All {
                knobs = knobs.push(
                    column![
                        heading("Auto"),
                        self.param(param::AUTO_RELEASE).switch("Auto")
                    ]
                    .spacing(4)
                    .width(Length::Fill)
                    .align_x(Alignment::Center),
                );
            }
            knobs = knobs.push(
                container(self.param(index).column(KNOB))
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

    /// Parameters the main row does not carry, plus the auto release switch.
    fn dynamics_group(&self, layout: Arrangement) -> Element<'a, Message> {
        let mut group = row![].spacing(10).align_y(Alignment::Center);
        if layout.knobs == Knobs::Four {
            for index in [param::KNEE, param::MAKEUP, param::MIX] {
                group = group.push(self.param(index).column(KNOB - 6.0));
            }
        }
        if layout.knobs != Knobs::All {
            group = group.push(
                column![
                    heading("Release"),
                    self.param(param::AUTO_RELEASE).switch("Auto")
                ]
                .spacing(4)
                .align_x(Alignment::Center),
            );
        }
        group
            .push(
                column![
                    self.param(param::RANGE_ENABLED).switch("Range"),
                    self.param(param::RANGE).value_knob(),
                ]
                .spacing(4)
                .align_x(Alignment::Center),
            )
            .into()
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
        let mode = self
            .effect
            .params
            .get(param::DETECTOR_MODE)
            .copied()
            .unwrap_or(0.0)
            >= 0.5;
        let (track_id, effect_id) = (self.track_id, self.effect.id);
        row![
            filter(param::HP_ENABLED, param::HP_FREQUENCY, "Low cut"),
            filter(param::LP_ENABLED, param::LP_FREQUENCY, "High cut"),
            column![
                heading("Detector"),
                segmented(
                    &[(false, "Peak"), (true, "RMS")],
                    mode,
                    move |rms| Message::set_effect_param(
                        track_id,
                        effect_id,
                        param::DETECTOR_MODE,
                        if rms { 1.0 } else { 0.0 },
                    ),
                    self.accent(),
                ),
            ]
            .spacing(4),
            column![
                self.param(param::STEREO_LINK).switch("Link"),
                self.param(param::DETECTOR_LISTEN).switch("Listen"),
            ]
            .spacing(4),
        ]
        .spacing(10)
        .align_y(Alignment::Center)
        .into()
    }

    fn advanced(&self, layout: Arrangement, height: f32) -> Element<'a, Message> {
        if layout.groups_beside && height >= KNOB_ROW_H {
            return container(
                row![
                    container(self.dynamics_group(layout)).width(GROUP_W),
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
                (AdvancedPage::Dynamics, "Dynamics"),
                (AdvancedPage::Detector, "Detector"),
            ],
            self.panel.advanced_page,
            move |page| Message::Compressor(CompressorMsg::ShowAdvancedPage(effect_id, page)),
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
            AdvancedPage::Dynamics => self.dynamics_group(layout),
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
            AdvancedPage::Dynamics => row![
                self.param(param::KNEE).value_knob(),
                self.param(param::MAKEUP).value_knob(),
                self.param(param::MIX).value_knob(),
                self.param(param::AUTO_RELEASE).switch("Auto"),
            ],
            AdvancedPage::Detector => row![
                self.param(param::HP_ENABLED).switch("Low cut"),
                self.param(param::HP_FREQUENCY).value_knob(),
                self.param(param::STEREO_LINK).switch("Link"),
            ],
        }
        .spacing(8)
        .align_y(Alignment::Center)
        .into()
    }

    fn disclosure(&self) -> Element<'a, Message> {
        disclosure(
            self.panel.advanced,
            Message::Compressor(CompressorMsg::ToggleAdvanced(self.effect.id)),
            self.accent(),
        )
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
        for width in [COMPACT_W, MEDIUM_W, FULL_W, MAX_W] {
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
    fn wider_cards_carry_more_knobs_and_both_advanced_groups() {
        let full = arrange(Size::new(FULL_W, 300.0), false);
        assert_eq!(full.knobs, Knobs::All);
        assert!(full.plots > 0.0 && full.knob_row == KNOB_ROW_H);
        assert_eq!(
            arrange(Size::new(MEDIUM_W, 300.0), false).knobs,
            Knobs::Seven
        );
        assert_eq!(
            arrange(Size::new(COMPACT_W, 300.0), false).knobs,
            Knobs::Four
        );
        let open = arrange(Size::new(FULL_W, 400.0), true);
        assert!(open.groups_beside && open.advanced == Some(KNOB_ROW_H) && !open.advanced_only);
    }

    #[test]
    fn short_cards_drop_the_plots_before_the_knobs() {
        let short = arrange(Size::new(FULL_W, 120.0), false);
        assert_eq!(short.plots, 0.0);
        assert_eq!(short.knob_row, KNOB_ROW_H);
        let shortest = arrange(Size::new(FULL_W, 70.0), false);
        assert_eq!(shortest.knob_row, LINE_H);
        assert!(arrange(Size::new(COMPACT_W, 150.0), true).advanced_only);
    }

    #[test]
    fn card_width_is_compact_until_the_medium_layout_fits() {
        assert_eq!(WIDTHS.card_width(300.0), COMPACT_W);
        assert_eq!(WIDTHS.card_width(MEDIUM_W), MEDIUM_W);
        assert_eq!(WIDTHS.card_width(5000.0), MAX_W);
    }
}
