//! Native Filter body: the filter type, its response with a cutoff to drag
//! and the output level above the main knobs and the cutoff modulation.
//! Narrower cards page between the filter and its modulation; the shortest
//! put everything on one line.

use iced::widget::{canvas, column, container, row, text, Space};
use iced::{Alignment, Color, Element, Length, Size};
use vibez_core::id::TrackId;
use vibez_dsp::filter::param;

use crate::domains::filter::{
    FilterMsg, FilterPanelState, Page, MODE_NAMES, SOURCE_NAMES, SYNC_NAMES,
};
use crate::domains::param_entry::ValueEntry;
use crate::message::Message;
use crate::state::UiEffect;
use crate::theme as th;
use crate::widgets::device_controls::{
    disclosure, dropdown, heading, rule, segmented, toggle, vertical_rule, Param, LABEL_SIZE,
};
use crate::widgets::filter_display::{ModulationPreview, ResponsePlot};
use crate::widgets::level_meter::{self, LevelMeter};

const PAD: f32 = 10.0;
const PRESET_ROW_H: f32 = 28.0;
const KNOB: f32 = 32.0;
const SMALL_KNOB: f32 = 18.0;
const KNOB_COL_W: f32 = 68.0;
const KNOB_ROW_H: f32 = 74.0;
const LINE_H: f32 = 24.0;
const PLOT_MIN_H: f32 = 72.0;
const PLOT_MIN_W: f32 = 220.0;
const TABS_ROW_H: f32 = 26.0;
const TYPE_LINE_H: f32 = 30.0;
/// The full card's Type buttons stacked above Slope.
const TYPE_COLUMN_H: f32 = 140.0;
const TYPE_COLUMN_W: f32 = 100.0;
/// The medium card's Type and Slope rows beside the plot.
const TYPE_PANEL_H: f32 = 100.0;
const TYPE_PANEL_W: f32 = 170.0;
const OUTPUT_W: f32 = 58.0;
const MODULATION_W: f32 = 520.0;
const PREVIEW_MIN_H: f32 = 36.0;
/// One labelled value and knob on a single-line row.
const LINE_ITEM_W: f32 = 124.0;
const LINE_TABS_W: f32 = 140.0;
const LINE_TYPE_W: f32 = 160.0;

pub const COMPACT_W: f32 = 420.0;
/// The Type controls fit beside the plot.
pub const MEDIUM_W: f32 = TYPE_PANEL_W + PLOT_MIN_W + OUTPUT_W + 2.0 * 8.0 + PAD * 2.0;
/// The main knobs and the modulation share one row, so nothing pages.
pub const FULL_W: f32 = 4.0 * KNOB_COL_W + 1.0 + MODULATION_W + 2.0 * 8.0 + PAD * 2.0;
pub const MAX_W: f32 = 1000.0;

pub fn card_width(available: f32) -> f32 {
    if available >= COMPACT_W.max(MEDIUM_W) {
        available.min(MAX_W)
    } else {
        COMPACT_W
    }
}

pub fn preset_bar_width(size: Size) -> f32 {
    size.width - PAD * 2.0 - 90.0
}

/// Where the Type and Slope controls sit.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TypePlace {
    Beside,
    /// Their own line under the plot.
    Line,
    /// At the start of the single-line knob row.
    InKnobs,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Arrangement {
    /// Everything shows at once; narrower cards page Filter and Modulation.
    pub full: bool,
    /// Height of the page tabs' own row; zero when they are not paged or
    /// share the knob line.
    pub tabs_row: f32,
    /// Height of the plots, zero when there is no room for them.
    pub plots: f32,
    pub type_place: TypePlace,
    pub knob_row: f32,
    /// Height between the tabs and Advanced, which the Filter page may
    /// not fill and the Modulation page takes whole.
    pub room: f32,
    pub advanced: Option<f32>,
    pub advanced_only: bool,
}

impl Arrangement {
    fn tabs_inline(&self) -> bool {
        !self.full && !self.advanced_only && self.tabs_row == 0.0
    }
}

pub fn arrange(size: Size, advanced: bool) -> Arrangement {
    let full = size.width >= FULL_W;
    let below = (size.height - PRESET_ROW_H - 1.0).max(0.0);
    let tabs = if full { 0.0 } else { TABS_ROW_H };
    let stacked = 1.0 + KNOB_ROW_H;
    let advanced_only = advanced && below < tabs + TYPE_LINE_H + 1.0 + KNOB_ROW_H + stacked;
    if advanced_only {
        return Arrangement {
            full,
            tabs_row: 0.0,
            plots: 0.0,
            type_place: TypePlace::InKnobs,
            knob_row: 0.0,
            room: 0.0,
            advanced: Some(below),
            advanced_only,
        };
    }
    let tabs_row = if !full && below >= TABS_ROW_H + LINE_H {
        TABS_ROW_H
    } else {
        0.0
    };
    let room = below - tabs_row - if advanced { stacked } else { 0.0 };
    let mut rest = room;
    let knob_row = if rest >= KNOB_ROW_H {
        KNOB_ROW_H
    } else {
        LINE_H
    };
    rest -= knob_row;
    let beside_h = if full { TYPE_COLUMN_H } else { TYPE_PANEL_H };
    let (plots, type_place) = if size.width >= MEDIUM_W && rest >= beside_h + 1.0 {
        (rest - 1.0, TypePlace::Beside)
    } else if rest >= PLOT_MIN_H + 1.0 + TYPE_LINE_H + 1.0 {
        (rest - TYPE_LINE_H - 2.0, TypePlace::Line)
    } else if rest >= TYPE_LINE_H + 1.0 {
        (0.0, TypePlace::Line)
    } else {
        (0.0, TypePlace::InKnobs)
    };
    Arrangement {
        full,
        tabs_row,
        plots,
        type_place,
        knob_row,
        room,
        advanced: advanced.then_some(KNOB_ROW_H),
        advanced_only,
    }
}

/// A Sync division, named for the dropdown.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Division(usize);

impl std::fmt::Display for Division {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(SYNC_NAMES[self.0])
    }
}

pub struct FilterView<'a> {
    pub track_id: TrackId,
    pub effect: &'a UiEffect,
    pub accent: Color,
    pub panel: &'a FilterPanelState,
    pub entry: Option<&'a ValueEntry>,
    pub sample_rate: f32,
}

impl<'a> FilterView<'a> {
    fn accent(&self) -> Color {
        if self.effect.bypass {
            th::text_muted()
        } else {
            self.accent
        }
    }

    fn param(&self, index: usize) -> Param<'a> {
        let param = Param::new(self.track_id, self.effect, index, self.accent(), self.entry);
        if index == param::DEPTH {
            param.label("Amount")
        } else {
            param
        }
    }

    fn choice(&self, index: usize) -> usize {
        self.effect
            .params
            .get(index)
            .copied()
            .unwrap_or(vibez_dsp::filter::FILTER_PARAMS[index].default)
            .round()
            .max(0.0) as usize
    }

    fn set(&self, index: usize) -> impl Fn(usize) -> Message {
        let (track_id, effect_id) = (self.track_id, self.effect.id);
        move |choice| Message::set_effect_param(track_id, effect_id, index, choice as f32)
    }

    pub fn view(self, preset_bar: Element<'a, Message>, size: Size) -> Element<'a, Message> {
        let layout = arrange(size, self.panel.advanced);
        let top = row![
            preset_bar,
            Space::with_width(Length::Fill),
            disclosure(
                self.panel.advanced,
                Message::Filter(FilterMsg::ToggleAdvanced(self.effect.id)),
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
        if layout.tabs_row > 0.0 {
            body = body.push(
                container(self.tabs())
                    .padding([0.0, PAD])
                    .height(layout.tabs_row)
                    .center_y(layout.tabs_row),
            );
        }
        let page = if layout.full {
            Page::Filter
        } else {
            self.panel.page
        };
        match page {
            Page::Filter => {
                if layout.plots > 0.0 {
                    body = body.push(self.plots(layout)).push(rule());
                }
                if layout.type_place == TypePlace::Line {
                    body = body.push(self.type_line()).push(rule());
                }
                body = body.push(self.knob_row(layout, size.width));
            }
            Page::Modulation => body = body.push(self.modulation_page(layout)),
        }
        if let Some(height) = layout.advanced {
            body = body.push(rule()).push(self.advanced(height));
        }
        body.width(Length::Fill).height(Length::Fill).into()
    }

    fn tabs(&self) -> Element<'a, Message> {
        let effect_id = self.effect.id;
        segmented(
            &[(Page::Filter, "Filter"), (Page::Modulation, "Modulation")],
            self.panel.page,
            move |page| Message::Filter(FilterMsg::ShowPage(effect_id, page)),
            self.accent(),
        )
    }

    fn modes(&self) -> Element<'a, Message> {
        let options: Vec<(usize, &'static str)> = MODE_NAMES.iter().copied().enumerate().collect();
        segmented(
            &options,
            self.choice(param::MODE),
            self.set(param::MODE),
            self.accent(),
        )
    }

    fn slopes(&self) -> Element<'a, Message> {
        segmented(
            &[(0, "12 dB"), (1, "24 dB")],
            self.choice(param::SLOPE),
            self.set(param::SLOPE),
            self.accent(),
        )
    }

    /// The full card's Type buttons, one above the other.
    fn type_column(&self) -> Element<'a, Message> {
        let selected = self.choice(param::MODE);
        let mut modes = column![heading("Type")].spacing(3);
        for (mode, name) in MODE_NAMES.iter().enumerate() {
            modes = modes.push(
                container(toggle(
                    container(text(*name).size(LABEL_SIZE))
                        .center_x(Length::Fill)
                        .into(),
                    mode == selected,
                    self.set(param::MODE)(mode),
                    self.accent(),
                ))
                .width(Length::Fill),
            );
        }
        // The slope buttons name themselves, so they go without a heading.
        column![modes, self.slopes()]
            .spacing(8)
            .width(TYPE_COLUMN_W)
            .into()
    }

    fn type_panel(&self) -> Element<'a, Message> {
        column![
            column![heading("Type"), self.modes()].spacing(4),
            column![heading("Slope"), self.slopes()].spacing(4),
        ]
        .spacing(8)
        .width(TYPE_PANEL_W)
        .into()
    }

    fn type_line(&self) -> Element<'a, Message> {
        container(
            row![
                heading("Type"),
                self.modes(),
                Space::with_width(Length::Fill),
                heading("Slope"),
                self.slopes(),
            ]
            .spacing(6)
            .align_y(Alignment::Center),
        )
        .padding([0.0, PAD])
        .height(TYPE_LINE_H)
        .center_y(TYPE_LINE_H)
        .into()
    }

    fn response(&self) -> Element<'a, Message> {
        column![
            heading("dB"),
            canvas(ResponsePlot {
                track_id: self.track_id,
                effect_id: self.effect.id,
                params: &self.effect.params,
                sample_rate: self.sample_rate,
                accent: self.accent(),
            })
            .width(Length::Fill)
            .height(Length::Fill),
        ]
        .spacing(2)
        .width(Length::Fill)
        .into()
    }

    fn output(&self) -> Element<'a, Message> {
        column![
            heading("Output"),
            canvas(LevelMeter {
                levels: self.panel.output
            })
            .width(level_meter::WIDTH)
            .height(Length::Fill),
        ]
        .spacing(2)
        .width(OUTPUT_W)
        .into()
    }

    fn plots(&self, layout: Arrangement) -> Element<'a, Message> {
        let content = match (layout.type_place, layout.full) {
            (TypePlace::Beside, true) => {
                row![self.type_column(), self.response(), self.output()]
            }
            (TypePlace::Beside, false) => {
                row![self.response(), self.output(), self.type_panel()]
            }
            _ => row![self.response(), self.output()],
        };
        container(content.spacing(8))
            .padding([6.0, PAD])
            .height(layout.plots)
            .width(Length::Fill)
            .into()
    }

    const MAIN: [usize; 4] = [param::CUTOFF, param::RESONANCE, param::DRIVE, param::MIX];

    fn value_knob(&self, index: usize) -> Element<'a, Message> {
        let param = self.param(index);
        row![
            param.value_entry(),
            canvas(param.knob()).width(SMALL_KNOB).height(SMALL_KNOB),
        ]
        .spacing(2)
        .align_y(Alignment::Center)
        .into()
    }

    fn line_item(&self, index: usize) -> Element<'a, Message> {
        row![
            text(self.param(index).name())
                .size(LABEL_SIZE)
                .color(th::text_dim())
                .wrapping(text::Wrapping::None),
            self.value_knob(index),
        ]
        .spacing(4)
        .align_y(Alignment::Center)
        .into()
    }

    fn line(
        &self,
        layout: Arrangement,
        height: f32,
        content: Element<'a, Message>,
    ) -> Element<'a, Message> {
        let mut line = row![].spacing(10).align_y(Alignment::Center);
        if layout.tabs_inline() {
            line = line.push(self.tabs());
        }
        container(line.push(content))
            .padding([0.0, PAD])
            .height(height)
            .center_y(height)
            .into()
    }

    fn knob_row(&self, layout: Arrangement, width: f32) -> Element<'a, Message> {
        if layout.knob_row < KNOB_ROW_H {
            let mut room = width - PAD * 2.0;
            let mut content = row![].spacing(10).align_y(Alignment::Center);
            if layout.tabs_inline() {
                room -= LINE_TABS_W;
            }
            if layout.type_place == TypePlace::InKnobs {
                content = content.push(self.modes());
                room -= LINE_TYPE_W;
            }
            // Cutoff comes first, so it stays while there is room for one.
            let fits = (room / LINE_ITEM_W).floor().max(0.0) as usize;
            for &index in Self::MAIN.iter().take(fits) {
                content = content.push(self.line_item(index));
            }
            return self.line(layout, layout.knob_row, content.into());
        }
        let mut knobs = row![].spacing(0).align_y(Alignment::Center);
        for index in Self::MAIN {
            knobs = knobs.push(
                container(self.param(index).column(KNOB))
                    .width(Length::Fill)
                    .max_width(KNOB_COL_W * 1.6)
                    .align_x(Alignment::Center),
            );
        }
        let content: Element<'a, Message> = if layout.full {
            row![
                container(knobs).width(Length::FillPortion(4)),
                vertical_rule(),
                container(self.modulation(true)).width(Length::FillPortion(7)),
            ]
            .spacing(8)
            .align_y(Alignment::Center)
            .into()
        } else {
            knobs.into()
        };
        container(content)
            .padding([4.0, PAD])
            .height(layout.knob_row)
            .center_y(layout.knob_row)
            .into()
    }

    fn preview(&self) -> Element<'a, Message> {
        canvas(ModulationPreview {
            params: &self.effect.params,
            accent: self.accent(),
        })
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    /// Source, the LFO's rate and the amount, with the preview beside them
    /// when `preview` is set.
    fn modulation(&self, preview: bool) -> Element<'a, Message> {
        let source = self.choice(param::MOD_SOURCE);
        let sources: Vec<(usize, &'static str)> =
            SOURCE_NAMES.iter().copied().enumerate().collect();
        let mut controls = row![column![
            heading("Modulation"),
            segmented(&sources, source, self.set(param::MOD_SOURCE), self.accent()),
        ]
        .spacing(4)]
        .spacing(8)
        .align_y(Alignment::Center);
        if source == 1 {
            let set_sync = self.set(param::SYNC);
            controls = controls.push(
                column![
                    heading("Sync"),
                    container(dropdown(
                        (0..SYNC_NAMES.len()).map(Division).collect(),
                        Division(self.choice(param::SYNC).min(SYNC_NAMES.len() - 1)),
                        move |division| set_sync(division.0),
                    ))
                    .width(72),
                ]
                .spacing(4),
            );
            if self.choice(param::SYNC) == 0 {
                controls = controls.push(self.param(param::RATE).column(KNOB));
            }
        }
        controls = controls.push(self.param(param::DEPTH).column(KNOB));
        if preview {
            controls = controls.push(container(self.preview()).height(48));
        }
        controls.into()
    }

    fn modulation_page(&self, layout: Arrangement) -> Element<'a, Message> {
        let height = layout.room;
        if height < KNOB_ROW_H {
            let source = self.choice(param::MOD_SOURCE);
            let sources: Vec<(usize, &'static str)> =
                SOURCE_NAMES.iter().copied().enumerate().collect();
            let content = row![
                segmented(&sources, source, self.set(param::MOD_SOURCE), self.accent()),
                self.line_item(param::DEPTH),
            ]
            .spacing(10)
            .align_y(Alignment::Center);
            return self.line(layout, height, content.into());
        }
        let mut page = column![container(self.modulation(false)).height(KNOB_ROW_H - 8.0)]
            .spacing(4)
            .height(Length::Fill);
        if height - KNOB_ROW_H >= PREVIEW_MIN_H {
            page = page.push(self.preview());
        }
        container(page)
            .padding([4.0, PAD])
            .height(height)
            .width(Length::Fill)
            .into()
    }

    const ADVANCED: [usize; 3] = [param::STEREO_PHASE, param::ENV_ATTACK, param::ENV_RELEASE];

    fn advanced(&self, height: f32) -> Element<'a, Message> {
        if height < KNOB_ROW_H {
            let mut line = row![].spacing(10).align_y(Alignment::Center);
            for index in Self::ADVANCED {
                line = line.push(self.line_item(index));
            }
            return container(line)
                .padding([0.0, PAD])
                .height(height)
                .center_y(height)
                .into();
        }
        let mut knobs = row![].spacing(16).align_y(Alignment::Center);
        for index in Self::ADVANCED {
            knobs = knobs.push(self.param(index).column(KNOB));
        }
        container(knobs)
            .padding([4.0, PAD])
            .height(KNOB_ROW_H)
            .center_y(KNOB_ROW_H)
            .into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What the Filter page fills of the room under the tabs.
    fn page(layout: Arrangement) -> f32 {
        let plots = if layout.plots > 0.0 {
            layout.plots + 1.0
        } else {
            0.0
        };
        let type_line = if layout.type_place == TypePlace::Line {
            TYPE_LINE_H + 1.0
        } else {
            0.0
        };
        plots + type_line + layout.knob_row
    }

    fn used(layout: Arrangement) -> f32 {
        let mut used = PRESET_ROW_H + 1.0;
        if layout.advanced_only {
            return used + layout.advanced.unwrap();
        }
        used += layout.tabs_row + layout.room;
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
                        continue;
                    }
                    assert!(layout.knob_row >= LINE_H, "{context}");
                    assert!(page(layout) <= layout.room + 0.01, "{context}");
                    if layout.type_place == TypePlace::Beside {
                        let needs = if layout.full {
                            TYPE_COLUMN_H
                        } else {
                            TYPE_PANEL_H
                        };
                        assert!(layout.plots >= needs, "{context}");
                    }
                }
            }
        }
    }

    #[test]
    fn only_narrower_cards_page_the_modulation() {
        let full = arrange(Size::new(FULL_W, 320.0), false);
        assert!(full.full && full.tabs_row == 0.0 && full.type_place == TypePlace::Beside);
        let medium = arrange(Size::new(MEDIUM_W, 320.0), false);
        assert!(!medium.full && medium.tabs_row == TABS_ROW_H);
        assert_eq!(medium.type_place, TypePlace::Beside);
        let compact = arrange(Size::new(COMPACT_W, 320.0), false);
        assert_eq!(compact.type_place, TypePlace::Line);
        assert!(compact.plots >= PLOT_MIN_H);
    }

    #[test]
    fn short_cards_keep_the_type_and_tabs_reachable() {
        let short = arrange(Size::new(COMPACT_W, 56.0), false);
        assert_eq!(short.knob_row, LINE_H);
        assert_eq!(short.type_place, TypePlace::InKnobs);
        assert!(short.tabs_inline());
        let line = arrange(Size::new(COMPACT_W, 110.0), false);
        assert!(line.tabs_row > 0.0 && line.type_place == TypePlace::Line);
        assert!(arrange(Size::new(COMPACT_W, 150.0), true).advanced_only);
    }

    #[test]
    fn card_width_is_compact_until_the_type_fits_beside_the_plot() {
        assert_eq!(card_width(300.0), COMPACT_W);
        assert_eq!(card_width(MEDIUM_W), MEDIUM_W);
        assert_eq!(card_width(5000.0), MAX_W);
    }
}
