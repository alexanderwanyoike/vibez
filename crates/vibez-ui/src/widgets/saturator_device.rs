//! Native Saturator body. Tall wide cards show the transfer curve and the
//! harmonics side by side, with Advanced in a column beside them, over the
//! main knobs; tall narrow cards stack one plot over the knobs. Anywhere
//! shorter, the plots sit beside a column of controls so neither is lost,
//! and the shortest cards keep the main controls on a line.

use iced::widget::{canvas, column, container, row, text, Space};
use iced::{Alignment, Color, Element, Length, Size};
use vibez_core::id::TrackId;
use vibez_dsp::saturator::{param, shape, SATURATOR_PARAMS};

use crate::domains::device_card::{CardMsg, CardPanelState};
use crate::domains::param_entry::ValueEntry;
use crate::domains::saturator::{
    CURVE_PLOT, FILTERS_PAGE, OUTPUT_PAGE, POST_FILTER_NAMES, PRE_FILTER_NAMES, QUALITY_NAMES,
    SHAPE_NAMES, SHAPE_PAGE, SPECTRUM_PLOT,
};
use crate::message::Message;
use crate::state::UiEffect;
use crate::theme as th;
use crate::widgets::device_controls::{
    disclosure, heading, plot_band, rule, segmented, tabs, vertical_rule, Param, LABEL_SIZE,
};
use crate::widgets::device_layout::{stack, Stack, KNOB_ROW_H, LINE_H};
use crate::widgets::saturator_display::{SpectrumPlot, TransferPlot};

const PAD: f32 = 10.0;
const PRESET_ROW_H: f32 = 28.0;
const KNOB: f32 = 32.0;
const TABS_H: f32 = 22.0;
/// A plot under its tabs, the axis labels dropped below 60 px.
const PLOT_MIN_H: f32 = TABS_H + 4.0 + 40.0;
const PLOT_MIN_W: f32 = 120.0;
/// Each of two plots side by side.
const PLOT_PAIR_W: f32 = 220.0;
/// Plots shorter than this stacked over the knobs read better beside them.
const STACKED_PLOT_MIN_H: f32 = 120.0;
/// One line of dropdowns and switches with room around it.
const LINE_ROW_H: f32 = LINE_H + 8.0;
/// Page tabs over one row of controls.
const PAGED_H: f32 = TABS_H + 4.0 + KNOB_ROW_H;
/// The wide card's Advanced column: three rows of knobs and headed menus.
const SIDE_MIN_H: f32 = 16.0 + KNOB_ROW_H + 2.0 * (14.0 + LINE_H) + 4.0 * 6.0;
const SIDE_W: f32 = 300.0;
const MENU_W: f32 = 96.0;
const LINE_ITEM_W: f32 = 124.0;
/// The four main controls on a line at their widest values, measured.
const MAIN_LINE_W: f32 = 390.0;
const SWITCH_W: f32 = 80.0;
/// A menu with its name beside it.
const LABELLED_MENU_W: f32 = MENU_W + 36.0;
const GAP: f32 = 14.0;
/// A plot and the dividers either side of the controls column.
const BESIDE_GAPS: f32 = 17.0;
/// The Curve and Spectrum tabs with the shape menu beside them.
const HEADER_W: f32 = 240.0;
/// What `plot_band` keeps clear above and below its content.
const BAND_PAD: f32 = 6.0;

pub const COMPACT_W: f32 = 420.0;
// The narrowest card's line still holds every main control.
const _: () = assert!(COMPACT_W - PAD * 2.0 >= MAIN_LINE_W);
/// A plot beside the controls has room to read at any height.
pub const MEDIUM_W: f32 = 640.0;
/// Both plots fit side by side, with Advanced beside them.
pub const FULL_W: f32 = 1_000.0;
pub const MAX_W: f32 = 1_200.0;

pub fn card_width(available: f32) -> f32 {
    available.clamp(COMPACT_W, MAX_W)
}

pub fn preset_bar_width(size: Size) -> f32 {
    size.width - PAD * 2.0 - 90.0
}

/// The controls column beside the plots, narrower on a narrow card.
fn controls_width(width: f32) -> f32 {
    (width * 0.55).clamp(240.0, 320.0)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    Compact,
    Medium,
    Full,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Plots {
    None,
    /// One at a time, under Curve and Spectrum tabs.
    Paged,
    Both,
}

/// How the controls column beside the plots divides: the main knobs, Curve
/// Amount and Asymmetry, and the Auto Gain and Quality line. A zero row is
/// left out, its controls moving to the main line or to Advanced.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Controls {
    pub main: f32,
    pub shaping: f32,
    pub switches: f32,
}

impl Controls {
    const NONE: Self = Self {
        main: 0.0,
        shaping: 0.0,
        switches: 0.0,
    };

    fn fit(height: f32) -> Self {
        let rows = |main, shaping, switches| Self {
            main,
            shaping,
            switches,
        };
        let (tall, line) = (KNOB_ROW_H + 1.0, LINE_H + 1.0);
        if height >= tall * 2.0 + LINE_H {
            rows(KNOB_ROW_H, KNOB_ROW_H, LINE_H)
        } else if height >= tall + line + LINE_H {
            rows(KNOB_ROW_H, LINE_H, LINE_H)
        } else if height >= tall + LINE_H {
            rows(KNOB_ROW_H, 0.0, LINE_H)
        } else if height >= KNOB_ROW_H {
            rows(KNOB_ROW_H, 0.0, 0.0)
        } else if height >= line * 2.0 + LINE_H {
            rows(LINE_H, LINE_H, LINE_H)
        } else if height >= line + LINE_H {
            rows(LINE_H, 0.0, LINE_H)
        } else {
            rows(LINE_H, 0.0, 0.0)
        }
    }

    pub fn height(self) -> f32 {
        [self.main, self.shaping, self.switches]
            .into_iter()
            .filter(|row| *row > 0.0)
            .map(|row| row + 1.0)
            .sum::<f32>()
            - 1.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Arrangement {
    pub tier: Tier,
    /// The plots stand over the knobs rather than beside the controls.
    pub stacked: bool,
    pub plots: Plots,
    /// Stacked, the plot band's height; beside, the band holding the plots
    /// and the controls.
    pub band: f32,
    /// The stacked card's main knobs.
    pub knob_row: f32,
    /// The stacked compact card's Auto Gain and Quality line.
    pub switches: f32,
    /// The controls column beside the plots.
    pub controls: Controls,
    /// Advanced as a row under everything else.
    pub advanced: Option<f32>,
    /// Advanced as a column beside the stacked wide card's plots.
    pub side: bool,
    /// The shape menu sits in the plot's header; when it does not fit
    /// there it joins Auto Gain and Quality.
    pub shape_in_header: bool,
    pub advanced_only: bool,
}

fn stacked(tier: Tier, plots: Plots, stack: Stack, switches: f32, side: bool) -> Arrangement {
    Arrangement {
        tier,
        stacked: true,
        plots,
        band: stack.plots,
        knob_row: stack.knob_row,
        switches,
        controls: Controls::NONE,
        advanced: stack.advanced,
        side,
        shape_in_header: true,
        advanced_only: false,
    }
}

pub fn arrange(size: Size, advanced: bool) -> Arrangement {
    let tier = if size.width >= FULL_W {
        Tier::Full
    } else if size.width >= MEDIUM_W {
        Tier::Medium
    } else {
        Tier::Compact
    };
    let below = (size.height - PRESET_ROW_H - 1.0).max(0.0);
    match tier {
        Tier::Full => {
            let open = stack(below, None, PLOT_MIN_H);
            let needs = if advanced {
                SIDE_MIN_H.max(STACKED_PLOT_MIN_H)
            } else {
                STACKED_PLOT_MIN_H
            };
            if open.plots >= needs {
                return stacked(tier, Plots::Both, open, 0.0, advanced);
            }
        }
        Tier::Compact => {
            let switches = LINE_H;
            let open = stack(
                below - switches - 1.0,
                advanced.then_some(PAGED_H),
                PLOT_MIN_H,
            );
            if !open.advanced_only && open.plots >= STACKED_PLOT_MIN_H {
                return stacked(tier, Plots::Paged, open, switches, false);
            }
        }
        Tier::Medium => {}
    }
    let mut beside = Arrangement {
        tier,
        stacked: false,
        plots: Plots::None,
        band: 0.0,
        knob_row: 0.0,
        switches: 0.0,
        controls: Controls::NONE,
        advanced: advanced.then_some(LINE_ROW_H),
        side: false,
        shape_in_header: false,
        advanced_only: false,
    };
    if advanced && below < LINE_H + 1.0 + LINE_ROW_H {
        beside.advanced = Some(below);
        beside.advanced_only = true;
        return beside;
    }
    let upper = below - if advanced { LINE_ROW_H + 1.0 } else { 0.0 };
    beside.band = upper;
    let plot_w = size.width - PAD * 2.0 - controls_width(size.width) - BESIDE_GAPS;
    beside.plots = if upper < PLOT_MIN_H + BAND_PAD * 2.0 || plot_w < PLOT_MIN_W {
        Plots::None
    } else if plot_w >= PLOT_PAIR_W * 2.0 + BESIDE_GAPS {
        Plots::Both
    } else {
        Plots::Paged
    };
    // Only the plots sit in a padded band; the controls alone fill theirs.
    beside.controls = if beside.plots == Plots::None {
        Controls::fit(upper)
    } else {
        Controls::fit(upper - BAND_PAD * 2.0)
    };
    beside.shape_in_header = match beside.plots {
        Plots::None => false,
        Plots::Paged => plot_w >= HEADER_W,
        Plots::Both => true,
    };
    beside
}

pub struct SaturatorView<'a> {
    pub track_id: TrackId,
    pub effect: &'a UiEffect,
    pub accent: Color,
    pub panel: &'a CardPanelState,
    pub entry: Option<&'a ValueEntry>,
}

impl<'a> SaturatorView<'a> {
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
            .unwrap_or(SATURATOR_PARAMS[index].default)
            .round()
            .max(0.0) as usize
    }

    fn on(&self, index: usize) -> bool {
        self.choice(index) >= 1
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
            body = body.push(self.advanced_row(layout, height, size.width));
            return body.width(Length::Fill).height(Length::Fill).into();
        }
        if layout.stacked {
            body = body.push(self.stacked_plots(layout)).push(rule());
            body = body.push(self.knob_row(layout, size.width));
            if layout.switches > 0.0 {
                body = body.push(rule()).push(
                    container(self.switches(false))
                        .padding([0.0, PAD])
                        .height(layout.switches)
                        .center_y(layout.switches),
                );
            }
        } else {
            body = body.push(self.beside(layout, size.width));
        }
        if let Some(height) = layout.advanced {
            body = body
                .push(rule())
                .push(self.advanced_row(layout, height, size.width));
        }
        body.width(Length::Fill).height(Length::Fill).into()
    }

    fn set(&self, index: usize) -> impl Fn(usize) -> Message {
        let (track_id, effect_id) = (self.track_id, self.effect.id);
        move |choice| Message::set_effect_param(track_id, effect_id, index, choice as f32)
    }

    fn transfer(&self) -> Element<'a, Message> {
        let input = self.panel.input;
        canvas(TransferPlot {
            track_id: self.track_id,
            effect_id: self.effect.id,
            params: &self.effect.params,
            input_peak: input.peak_l.max(input.peak_r),
            accent: self.accent(),
        })
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    fn spectrum(&self) -> Element<'a, Message> {
        canvas(SpectrumPlot {
            params: &self.effect.params,
            accent: self.accent(),
        })
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    /// One plot at a time under its tabs, with the shape beside them.
    fn paged_plot(&self, shape: bool) -> Element<'a, Message> {
        let effect_id = self.effect.id;
        let page = self.panel.plot.min(SPECTRUM_PLOT);
        let mut header = row![
            tabs(
                &[(CURVE_PLOT, "Curve"), (SPECTRUM_PLOT, "Spectrum")],
                page,
                move |plot| Message::Card(CardMsg::ShowPlot(effect_id, plot)),
                self.accent(),
            ),
            Space::with_width(Length::Fill),
        ]
        .align_y(Alignment::Center)
        .height(TABS_H);
        if shape {
            header = header.push(self.shape_menu());
        }
        let plot = if page == SPECTRUM_PLOT {
            self.spectrum()
        } else {
            self.transfer()
        };
        column![header, plot].spacing(4).width(Length::Fill).into()
    }

    /// The transfer curve under the shape tabs, beside the harmonics.
    fn both_plots(&self) -> Element<'a, Message> {
        let shapes: Vec<(usize, &'static str)> = SHAPE_NAMES.iter().copied().enumerate().collect();
        let curve = column![
            row![
                heading("Transfer Curve"),
                Space::with_width(Length::Fill),
                tabs(
                    &shapes,
                    self.choice(param::SHAPE).min(shape::CUSTOM),
                    self.set(param::SHAPE),
                    self.accent(),
                ),
            ]
            .align_y(Alignment::Center)
            .height(TABS_H),
            self.transfer(),
        ]
        .spacing(4)
        .width(Length::Fill);
        let swatch = |color: Color| {
            container(Space::new(10.0, 2.0)).style(move |_theme: &iced::Theme| container::Style {
                background: Some(color.into()),
                ..Default::default()
            })
        };
        let legend = row![
            swatch(th::text()),
            text("Input").size(LABEL_SIZE).color(th::text_dim()),
            swatch(self.accent()),
            text("Processed").size(LABEL_SIZE).color(th::text_dim()),
        ]
        .spacing(4)
        .align_y(Alignment::Center);
        let spectrum = column![
            row![
                heading("Harmonic Spectrum"),
                Space::with_width(Length::Fill),
                legend
            ]
            .align_y(Alignment::Center)
            .height(TABS_H),
            self.spectrum(),
        ]
        .spacing(4)
        .width(Length::Fill);
        row![curve, vertical_rule(), spectrum].spacing(8).into()
    }

    fn shape_menu(&self) -> Element<'a, Message> {
        container(self.param(param::SHAPE).choices(&SHAPE_NAMES, 0))
            .width(MENU_W * 0.75)
            .into()
    }

    fn plots(&self, layout: Arrangement) -> Option<Element<'a, Message>> {
        match layout.plots {
            Plots::None => None,
            Plots::Paged => Some(self.paged_plot(layout.shape_in_header)),
            Plots::Both => Some(self.both_plots()),
        }
    }

    fn stacked_plots(&self, layout: Arrangement) -> Element<'a, Message> {
        let mut content = row![].spacing(8);
        if let Some(plots) = self.plots(layout) {
            content = content.push(plots);
        }
        if layout.side {
            content = content.push(vertical_rule()).push(self.side(layout.band));
        }
        plot_band(content, layout.band, PAD)
    }

    /// The plots beside the controls column, or the controls alone across
    /// the card when no plot fits.
    fn beside(&self, layout: Arrangement, width: f32) -> Element<'a, Message> {
        let shape = !layout.shape_in_header;
        match self.plots(layout) {
            Some(plots) => {
                let controls_w = controls_width(width);
                let content = row![
                    plots,
                    vertical_rule(),
                    container(self.controls(layout.controls, controls_w, shape)).width(controls_w),
                ]
                .spacing(8);
                plot_band(content, layout.band, PAD)
            }
            None => container(self.controls(layout.controls, width - PAD * 2.0, shape))
                .padding([0.0, PAD])
                .height(layout.band)
                .into(),
        }
    }

    /// A menu, its name above it or beside it.
    fn menu(
        &self,
        name: &'a str,
        index: usize,
        names: &'static [&'static str],
        stacked: bool,
        width: Length,
    ) -> Element<'a, Message> {
        let menu = container(self.param(index).choices(names, 0)).width(width);
        if stacked {
            column![heading(name), menu].spacing(4).into()
        } else {
            row![
                text(name)
                    .size(LABEL_SIZE)
                    .color(th::text_dim())
                    .wrapping(text::Wrapping::None),
                menu
            ]
            .spacing(4)
            .align_y(Alignment::Center)
            .into()
        }
    }

    fn quality(&self) -> Element<'a, Message> {
        self.menu(
            "Quality",
            param::QUALITY,
            &QUALITY_NAMES,
            false,
            Length::Fixed(MENU_W * 0.7),
        )
    }

    /// A switch under its name, reading On or Off.
    fn headed_switch(&self, name: &'a str, index: usize) -> Element<'a, Message> {
        let state = if self.on(index) { "On" } else { "Off" };
        column![heading(name), self.param(index).switch(state)]
            .spacing(4)
            .into()
    }

    fn knob(&self, index: usize) -> Element<'a, Message> {
        container(self.param(index).column(KNOB))
            .width(Length::Fill)
            .max_width(110.0)
            .align_x(Alignment::Center)
            .into()
    }

    /// The stacked wide card's Advanced, beside its plots.
    fn side(&self, height: f32) -> Element<'a, Message> {
        let pair = |a: Element<'a, Message>, b: Element<'a, Message>| {
            row![
                container(a).width(Length::FillPortion(1)),
                container(b).width(Length::FillPortion(1))
            ]
            .spacing(12)
            .align_y(Alignment::End)
        };
        let menu = |name, index, names| self.menu(name, index, names, true, Length::Fill);
        // The rows share the band's height rather than bunching at its top.
        let gap = || Space::with_height(Length::Fill);
        column![
            heading("Advanced"),
            gap(),
            pair(self.knob(param::CURVE), self.knob(param::ASYMMETRY)),
            gap(),
            pair(
                menu("Pre Filter", param::PRE_FILTER, &PRE_FILTER_NAMES),
                menu("Post Filter", param::POST_FILTER, &POST_FILTER_NAMES),
            ),
            gap(),
            pair(
                self.headed_switch("DC Removal", param::DC_REMOVAL),
                menu("Quality", param::QUALITY, &QUALITY_NAMES),
            ),
            gap(),
        ]
        .width(SIDE_W)
        .height(height)
        .into()
    }

    const MAIN: [usize; 4] = [param::DRIVE_DB, param::BIAS, param::OUTPUT_DB, param::MIX];

    /// The main knobs across `width`, or on a line when short.
    fn main_knobs(&self, height: f32, width: f32) -> Element<'a, Message> {
        if height < KNOB_ROW_H {
            let fits = if width >= MAIN_LINE_W {
                4
            } else {
                (width / (MAIN_LINE_W / 4.0)).floor().max(1.0) as usize
            };
            let mut line = row![].spacing(10).align_y(Alignment::Center);
            for index in [param::DRIVE_DB, param::OUTPUT_DB, param::MIX, param::BIAS]
                .into_iter()
                .take(fits)
            {
                line = line.push(self.param(index).line_item());
            }
            return line.width(Length::Fill).into();
        }
        let mut knobs = row![].spacing(6).align_y(Alignment::Center);
        for index in Self::MAIN {
            knobs = knobs.push(self.knob(index));
        }
        knobs.width(Length::Fill).into()
    }

    fn knob_row(&self, layout: Arrangement, width: f32) -> Element<'a, Message> {
        let height = layout.knob_row;
        let inner = width - PAD * 2.0;
        let content: Element<'a, Message> = if layout.tier == Tier::Full {
            row![
                self.main_knobs(height, inner - SWITCH_W - 12.0),
                vertical_rule(),
                self.headed_switch("Auto Gain", param::AUTO_GAIN),
            ]
            .spacing(12)
            .align_y(Alignment::Center)
            .into()
        } else {
            self.main_knobs(height, inner)
        };
        let padding = if height < KNOB_ROW_H { 0.0 } else { 4.0 };
        container(content)
            .padding([padding, PAD])
            .height(height)
            .center_y(height)
            .into()
    }

    /// Auto Gain and Quality, with the shape menu when the plot's header
    /// has no room for it.
    fn switches(&self, shape: bool) -> Element<'a, Message> {
        let mut line = row![
            self.param(param::AUTO_GAIN).switch("Auto Gain"),
            Space::with_width(Length::Fill),
        ]
        .spacing(6)
        .align_y(Alignment::Center);
        if shape {
            line = line.push(self.shape_menu());
        }
        line.push(self.quality()).into()
    }

    /// The controls column: the rows that fit, sharing whatever height is
    /// left between them.
    fn controls(&self, rows: Controls, width: f32, shape: bool) -> Element<'a, Message> {
        let gap = || Space::with_height(Length::Fill);
        let mut controls = column![
            gap(),
            container(self.main_knobs(rows.main, width)).height(rows.main),
        ]
        .width(Length::Fill)
        .height(Length::Fill);
        if rows.shaping > 0.0 {
            let shaping: Element<'a, Message> = if rows.shaping >= KNOB_ROW_H {
                row![self.knob(param::CURVE), self.knob(param::ASYMMETRY)]
                    .spacing(6)
                    .into()
            } else {
                row![
                    self.param(param::CURVE).line_item(),
                    self.param(param::ASYMMETRY).line_item()
                ]
                .spacing(10)
                .align_y(Alignment::Center)
                .into()
            };
            controls = controls.push(gap()).push(rule()).push(gap()).push(
                container(shaping)
                    .height(rows.shaping)
                    .center_y(rows.shaping),
            );
        }
        if rows.switches > 0.0 {
            controls = controls.push(gap()).push(rule()).push(gap()).push(
                container(self.switches(shape))
                    .height(rows.switches)
                    .center_y(rows.switches),
            );
        }
        controls.push(gap()).into()
    }

    /// What Advanced holds on this card that its other rows do not, on one
    /// line when it fits and a page at a time when it does not.
    fn advanced_row(&self, layout: Arrangement, height: f32, width: f32) -> Element<'a, Message> {
        let held = Held::of(layout);
        if layout.stacked || held.line_width() > width - PAD * 2.0 {
            return self.pages(held, height);
        }
        let menu = |name, index, names| self.menu(name, index, names, false, Length::Fixed(MENU_W));
        let mut line = row![].spacing(GAP).align_y(Alignment::Center);
        if held.shape {
            line = line.push(self.shape_menu());
        }
        if held.shaping {
            line = line
                .push(self.param(param::CURVE).label("Curve").line_item())
                .push(self.param(param::ASYMMETRY).line_item());
        }
        line = line
            .push(menu("Pre", param::PRE_FILTER, &PRE_FILTER_NAMES))
            .push(menu("Post", param::POST_FILTER, &POST_FILTER_NAMES))
            .push(self.param(param::DC_REMOVAL).switch("DC Removal"));
        if held.gain {
            line = line
                .push(self.param(param::AUTO_GAIN).switch("Auto Gain"))
                .push(self.quality());
        }
        container(line)
            .padding([0.0, PAD])
            .height(height)
            .center_y(height)
            .into()
    }

    /// Advanced a page at a time: the curve, the filters, and the output
    /// switches when nothing else holds them.
    fn pages(&self, held: Held, height: f32) -> Element<'a, Message> {
        let effect_id = self.effect.id;
        let mut pages = vec![(SHAPE_PAGE, "Shape"), (FILTERS_PAGE, "Filters")];
        if held.gain {
            pages.push((OUTPUT_PAGE, "Output"));
        }
        let page = pages
            .iter()
            .map(|(page, _)| *page)
            .find(|page| *page == self.panel.advanced_page)
            .unwrap_or(SHAPE_PAGE);
        let tabs = segmented(
            &pages,
            page,
            move |page| Message::Card(CardMsg::ShowAdvancedPage(effect_id, page)),
            self.accent(),
        );
        let tall = height >= PAGED_H;
        let menu = |name, index, names, width| self.menu(name, index, names, tall, width);
        let narrow = Length::Fixed(MENU_W * 0.85);
        let mut controls = row![].spacing(if tall { 12 } else { 8 }).align_y(if tall {
            Alignment::End
        } else {
            Alignment::Center
        });
        match page {
            SHAPE_PAGE => {
                if held.shape {
                    controls = controls.push(if tall {
                        column![heading("Shape"), self.shape_menu()]
                            .spacing(4)
                            .into()
                    } else {
                        self.shape_menu()
                    });
                }
                controls = if tall {
                    controls
                        .push(self.knob(param::CURVE))
                        .push(self.knob(param::ASYMMETRY))
                } else {
                    controls
                        .push(self.param(param::CURVE).label("Curve").line_item())
                        .push(self.param(param::ASYMMETRY).line_item())
                };
            }
            FILTERS_PAGE if tall => {
                controls = controls
                    .push(menu(
                        "Pre Filter",
                        param::PRE_FILTER,
                        &PRE_FILTER_NAMES,
                        Length::Fill,
                    ))
                    .push(menu(
                        "Post Filter",
                        param::POST_FILTER,
                        &POST_FILTER_NAMES,
                        Length::Fill,
                    ))
                    .push(self.headed_switch("DC Removal", param::DC_REMOVAL));
            }
            FILTERS_PAGE => {
                controls = controls
                    .push(menu("Pre", param::PRE_FILTER, &PRE_FILTER_NAMES, narrow))
                    .push(menu("Post", param::POST_FILTER, &POST_FILTER_NAMES, narrow))
                    .push(self.param(param::DC_REMOVAL).switch("DC"));
            }
            _ => {
                controls = controls
                    .push(self.param(param::AUTO_GAIN).switch("Auto Gain"))
                    .push(self.quality());
            }
        }
        if !tall {
            return container(row![tabs, controls].spacing(10).align_y(Alignment::Center))
                .padding([0.0, PAD])
                .height(height)
                .center_y(height)
                .into();
        }
        container(column![tabs, controls].spacing(6))
            .padding([4.0, PAD])
            .height(height)
            .into()
    }
}

/// The controls Advanced must hold because no other row on the card does.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Held {
    shape: bool,
    shaping: bool,
    gain: bool,
}

impl Held {
    fn of(layout: Arrangement) -> Self {
        let switches = layout.stacked || layout.controls.switches > 0.0;
        Self {
            shape: !layout.shape_in_header && !switches,
            shaping: layout.stacked || layout.controls.shaping == 0.0,
            gain: !switches,
        }
    }

    /// Roughly how wide these sit on one line.
    fn line_width(self) -> f32 {
        let mut widths = vec![LABELLED_MENU_W, LABELLED_MENU_W, SWITCH_W];
        if self.shape {
            widths.push(MENU_W * 0.75);
        }
        if self.shaping {
            widths.extend([LINE_ITEM_W, LINE_ITEM_W]);
        }
        if self.gain {
            widths.extend([SWITCH_W, LABELLED_MENU_W]);
        }
        widths.iter().sum::<f32>() + GAP * (widths.len() - 1) as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Below the preset row in the default 280 px detail panel and in its
    /// 180 px minimum, as measured in the app.
    const DEFAULT_BELOW: f32 = 135.0;
    const MINIMUM_BELOW: f32 = 35.0;

    fn size(width: f32, below: f32) -> Size {
        Size::new(width, below + PRESET_ROW_H + 1.0)
    }

    fn used(layout: Arrangement) -> f32 {
        let mut used = PRESET_ROW_H + 1.0;
        if layout.advanced_only {
            return used + layout.advanced.unwrap();
        }
        if layout.stacked {
            used += layout.band + 1.0 + layout.knob_row;
            if layout.switches > 0.0 {
                used += layout.switches + 1.0;
            }
        } else {
            let padding = if layout.plots == Plots::None {
                0.0
            } else {
                BAND_PAD * 2.0
            };
            assert!(
                layout.controls.height() <= layout.band - padding + 0.01,
                "{layout:?}"
            );
            used += layout.band;
        }
        used + layout.advanced.map_or(0.0, |height| height + 1.0)
    }

    #[test]
    fn every_supported_size_fits_without_scrolling() {
        for width in [COMPACT_W, MEDIUM_W, FULL_W, MAX_W] {
            let lowest = (MINIMUM_BELOW + PRESET_ROW_H + 1.0) as usize;
            for height in (lowest..=460).step_by(2).map(|height| height as f32) {
                for advanced in [false, true] {
                    let layout = arrange(Size::new(width, height), advanced);
                    let context = format!("{width}x{height} {advanced}: {layout:?}");
                    assert!(used(layout) <= height + 0.01, "{context}");
                    if layout.plots != Plots::None {
                        assert!(layout.band >= PLOT_MIN_H, "{context}");
                    }
                    if layout.stacked {
                        assert!(layout.band >= STACKED_PLOT_MIN_H, "{context}");
                    }
                    if layout.side {
                        assert!(layout.band >= SIDE_MIN_H, "{context}");
                    }
                }
            }
        }
    }

    #[test]
    fn the_default_panel_keeps_a_plot_beside_full_knobs() {
        for width in [COMPACT_W, 460.0, MEDIUM_W, FULL_W, MAX_W] {
            for advanced in [false, true] {
                let layout = arrange(size(width, DEFAULT_BELOW), advanced);
                let context = format!("{width} {advanced}: {layout:?}");
                assert_ne!(layout.plots, Plots::None, "{context}");
                assert!(!layout.stacked, "{context}");
                assert_eq!(layout.controls.main, KNOB_ROW_H, "{context}");
                // Open, Advanced takes Auto Gain and Quality instead.
                assert_eq!(layout.controls.switches > 0.0, !advanced, "{context}");
            }
        }
        let wide = arrange(size(FULL_W, DEFAULT_BELOW), false);
        assert_eq!(wide.plots, Plots::Both);
    }

    #[test]
    fn the_minimum_panel_keeps_the_main_controls_on_a_line() {
        for width in [COMPACT_W, MEDIUM_W, FULL_W] {
            let layout = arrange(size(width, MINIMUM_BELOW), false);
            assert!(
                !layout.advanced_only && !layout.stacked,
                "{width}: {layout:?}"
            );
            assert!(layout.controls.main >= LINE_H, "{width}: {layout:?}");
            let open = arrange(size(width, MINIMUM_BELOW), true);
            assert!(open.advanced_only, "{width}: {open:?}");
        }
    }

    #[test]
    fn every_control_has_a_place_at_every_size() {
        for width in [COMPACT_W, 460.0, MEDIUM_W, FULL_W, MAX_W] {
            let lowest = (MINIMUM_BELOW + PRESET_ROW_H + 1.0) as usize;
            for height in (lowest..=460).step_by(2).map(|height| height as f32) {
                let closed = arrange(Size::new(width, height), false);
                let open = arrange(Size::new(width, height), true);
                let held = Held::of(open);
                let context = format!("{width}x{height}: {open:?}");
                // Shape is in the header, beside Auto Gain, or in Advanced.
                let switches = open.stacked || open.controls.switches > 0.0;
                assert!(open.shape_in_header || switches || held.shape, "{context}");
                assert!(switches || held.gain, "{context}");
                // Closing Advanced never drops the plot it had room for.
                if open.plots != Plots::None {
                    assert_ne!(closed.plots, Plots::None, "{width}x{height}");
                }
            }
        }
    }

    #[test]
    fn tall_cards_stack_as_the_concept_does() {
        let wide = arrange(Size::new(FULL_W + 150.0, 400.0), true);
        assert!(
            wide.stacked && wide.side && wide.plots == Plots::Both,
            "{wide:?}"
        );
        let compact = arrange(Size::new(COMPACT_W, 330.0), false);
        assert!(
            compact.stacked && compact.plots == Plots::Paged,
            "{compact:?}"
        );
        let medium = arrange(Size::new(MEDIUM_W, 330.0), false);
        assert!(
            !medium.stacked && medium.plots == Plots::Paged,
            "{medium:?}"
        );
    }
}
