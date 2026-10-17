//! The layout the tabbed native instrument cards share. Each tab is a band
//! of groups, a plot beside its controls, sharing the card's width; a
//! group's controls are knobs when the band is tall enough and lines when
//! not. Tall wide cards may show an overview of two bands instead, and the
//! shortest set a menu of the card's parts and the chosen part's controls
//! on one line.

use iced::widget::{column, container, row, text, Space};
use iced::{Alignment, Color, Element, Length, Size};
use vibez_core::id::TrackId;

use crate::domains::instrument_card::InstrumentCardMsg;
use crate::message::Message;
use crate::theme as th;
use crate::widgets::device_controls::{
    dropdown, heading, plot_band, rule, tabs, text_width, vertical_rule, Param, LABEL_SIZE,
};
use crate::widgets::device_layout::{pack, KNOB_ROW_H, LINE_H};

pub const PAD: f32 = 10.0;
pub const PRESET_ROW_H: f32 = 28.0;
pub const TABS_H: f32 = 24.0;
pub const HEADER_H: f32 = 22.0;
/// What `plot_band` keeps clear above and below its content.
pub const BAND_PAD: f32 = 6.0;
const KNOB: f32 = 30.0;
/// A knob with its name and value under it.
const KNOB_W: f32 = 60.0;
/// A menu with its name above it.
const CHOICE_W: f32 = 110.0;
pub const LINE_GAP: f32 = 8.0;
const GROUP_GAP: f32 = 10.0;
const PLOT_MIN_W: f32 = 110.0;
const PLOT_MAX_W: f32 = 300.0;
/// A card's centrepiece may grow further.
const CENTREPIECE_MAX_W: f32 = 460.0;
/// Below these a plot no longer reads well enough to keep knobs beside it.
const CENTREPIECE_GOOD_W: f32 = 220.0;
const PLOT_GOOD_W: f32 = 150.0;
const LINE_SLACK: f32 = 10.0;
/// The shortest a plot gets over its knobs.
const STACKED_PLOT_MIN_H: f32 = 56.0;
/// What a group takes when even lines cannot hold its knobs.
const LINE_ITEM_FALLBACK_W: f32 = 120.0;
/// A band's group header and one line of controls.
const BAND_MIN_H: f32 = HEADER_H + 4.0 + LINE_H;
/// The overview's bottom band: a header over a row of knobs.
pub const STRIP_H: f32 = HEADER_H + 4.0 + KNOB_ROW_H + BAND_PAD * 2.0;
pub const PAGE_MENU_W: f32 = 112.0;

pub const COMPACT_W: f32 = 440.0;
pub const MEDIUM_W: f32 = 700.0;
pub const FULL_W: f32 = 1_100.0;
pub const MAX_W: f32 = 1_300.0;

pub fn card_width(available: f32) -> f32 {
    available.clamp(COMPACT_W, MAX_W)
}

pub fn preset_bar_width(size: Size) -> f32 {
    size.width - PAD * 2.0
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    Compact,
    Medium,
    Full,
}

impl Tier {
    pub fn for_width(width: f32) -> Self {
        if width >= FULL_W {
            Self::Full
        } else if width >= MEDIUM_W {
            Self::Medium
        } else {
            Self::Compact
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Form {
    /// Tabs over one band of the tab's groups, its content this tall.
    Band(f32),
    /// Tabs over the overview's two bands, the top one's content this tall.
    Overview(f32),
    /// The part menu and the part's controls on one line.
    Line,
}

/// The form a card `size` takes; `overview` is how tall the overview's top
/// band's content must be, when the tab shown may have one.
pub fn arrange(size: Size, overview: Option<f32>) -> Form {
    let below = (size.height - PRESET_ROW_H - 1.0).max(0.0);
    let tabs = TABS_H + 1.0;
    let strip = BAND_PAD * 2.0 + 1.0 + STRIP_H;
    if overview.is_some_and(|top| below >= tabs + top + strip) {
        return Form::Overview(below - tabs - strip);
    }
    if below >= tabs + BAND_MIN_H + BAND_PAD * 2.0 {
        return Form::Band(below - tabs - BAND_PAD * 2.0);
    }
    Form::Line
}

/// How tall a card is in `form`, to check it never scrolls.
pub fn used_height(form: Form) -> f32 {
    PRESET_ROW_H
        + 1.0
        + match form {
            Form::Band(content) => TABS_H + 1.0 + content + BAND_PAD * 2.0,
            Form::Overview(top) => TABS_H + 1.0 + top + BAND_PAD * 2.0 + 1.0 + STRIP_H,
            Form::Line => LINE_H,
        }
}

/// What the layout needs to know of a card's plots.
pub trait PlotKind: Copy {
    /// The card's centrepiece takes more of the band and may grow wider.
    fn centrepiece(self) -> bool;
}

/// How a group's knobs are set: a grid so many across, or on lines.
#[derive(Clone, Copy)]
pub enum Knobs {
    Grid(usize),
    Lines,
}

/// Where a group's plot goes: beside its knobs (the plot's width, the
/// knobs' width), over them, or nowhere.
#[derive(Clone, Copy)]
enum Layout {
    Beside(f32, f32, Knobs),
    Stacked(Knobs),
    Alone,
}

/// A menu in a group's header: the parameter and its choices' names.
pub type Choice = (usize, &'static [&'static str]);

/// A plot beside its knobs, under a header that holds the group's menus.
pub struct Group<'a, P> {
    pub title: &'a str,
    pub note: Option<String>,
    /// A switch between things the group can show.
    pub switch: Option<Element<'a, Message>>,
    pub choices: Vec<Choice>,
    pub plot: Option<P>,
    pub knobs: Vec<usize>,
}

/// A part of the shortest card: its name, menus and knobs.
pub type LinePart = (String, Vec<Choice>, Vec<usize>);

/// How knobs `widths` wide on a line sit in an area `area` wide and
/// `content` tall: as a grid so many across, or on lines, or not at all.
fn arrange_knobs(widths: &[f32], area: f32, content: f32) -> Option<Knobs> {
    let count = widths.len();
    if count == 0 {
        return Some(Knobs::Grid(1));
    }
    let rows = ((content + 4.0) / KNOB_ROW_H).floor() as usize;
    let across = ((area + 6.0) / (KNOB_W + 6.0)).floor() as usize;
    if rows > 0 && across > 0 && count.div_ceil(across) <= rows {
        let rows = count.div_ceil(across);
        return Some(Knobs::Grid(count.div_ceil(rows)));
    }
    let lines = ((content + 1.0) / (LINE_H + 1.0)).floor() as usize;
    // `pack` gives a control wider than the line a line of its own, so each
    // must also fit.
    let fits = widths.iter().all(|width| *width <= area);
    (lines > 0 && fits && pack(widths, area, LINE_GAP).len() <= lines).then_some(Knobs::Lines)
}

/// The narrowest area that holds knobs `widths` wide on a line as a grid
/// (when `grid`) or on lines in `content`.
fn least_width(widths: &[f32], content: f32, grid: bool) -> Option<f32> {
    if widths.is_empty() {
        return Some(0.0);
    }
    // A little over the measure, so rounding never cuts a value off.
    (0..300)
        .map(|step| 40.0 + step as f32 * 4.0)
        .find(|area| {
            matches!(
                (arrange_knobs(widths, *area, content), grid),
                (Some(Knobs::Grid(_)), true) | (Some(Knobs::Lines), false)
            )
        })
        .map(|area| area + LINE_SLACK)
}

/// A tabbed native instrument card laid out in bands.
pub trait BandCard<'a> {
    type Plot: PlotKind;

    fn track_id(&self) -> TrackId;
    fn accent(&self) -> Color;
    fn param(&self, index: usize) -> Param<'a>;
    /// The plot, filling what it is given.
    fn plot(&self, plot: Self::Plot) -> Element<'a, Message>;
    /// Every group once, named apart, in the order the tabs show them, for
    /// the shortest card to step through.
    fn every_group(&self) -> Vec<Group<'a, Self::Plot>>;
    /// The part the shortest card shows.
    fn line_part(&self) -> usize;

    /// The card: the preset bar over the tabs `pages` with `page` chosen,
    /// over `bands` (each its groups and content height) in `form`.
    fn frame(
        &self,
        preset_bar: Element<'a, Message>,
        size: Size,
        form: Form,
        pages: &[&'a str],
        page: usize,
        bands: Vec<(Vec<Group<'a, Self::Plot>>, f32)>,
    ) -> Element<'a, Message> {
        let inner = size.width - PAD * 2.0;
        let mut body = column![
            container(preset_bar)
                .padding([0.0, PAD])
                .height(PRESET_ROW_H)
                .align_y(Alignment::Center),
            rule(),
        ];
        if form == Form::Line {
            body = body.push(self.line(inner, size.height - PRESET_ROW_H - 1.0));
            return body.width(Length::Fill).height(Length::Fill).into();
        }
        let track_id = self.track_id();
        let pages: Vec<(usize, &str)> = pages.iter().copied().enumerate().collect();
        body = body
            .push(
                container(tabs(
                    &pages,
                    page,
                    move |page| {
                        Message::InstrumentCard(InstrumentCardMsg::ShowPage(track_id, page))
                    },
                    self.accent(),
                ))
                .padding([0.0, PAD])
                .height(TABS_H)
                .align_y(Alignment::Center),
            )
            .push(rule());
        let count = bands.len();
        for (index, (groups, height)) in bands.into_iter().enumerate() {
            body = body.push(self.band(groups, height, inner));
            if index + 1 < count {
                body = body.push(rule());
            }
        }
        body.width(Length::Fill).height(Length::Fill).into()
    }

    fn knob(&self, index: usize, knobs: bool) -> Element<'a, Message> {
        if knobs {
            container(self.param(index).column(KNOB))
                .width(KNOB_W)
                .align_x(Alignment::Center)
                .into()
        } else {
            self.param(index).line_item()
        }
    }

    fn menu(&self, (index, names): Choice) -> Element<'a, Message> {
        container(self.param(index).choices(names, 0))
            .width(CHOICE_W)
            .into()
    }

    /// How wide each of `knobs` sets on a line, measured once for a group.
    fn line_widths(&self, knobs: &[usize]) -> Vec<f32> {
        knobs
            .iter()
            .map(|index| self.param(*index).line_width())
            .collect()
    }

    /// How wide the group's header needs to be.
    fn header_width(&self, group: &Group<'a, Self::Plot>) -> f32 {
        text_width(group.title, LABEL_SIZE, iced::Font::DEFAULT)
            + group.choices.len() as f32 * (CHOICE_W + 8.0)
            + if group.switch.is_some() { 120.0 } else { 0.0 }
            + if group.note.is_some() { 70.0 } else { 0.0 }
    }

    /// The width a group without a plot takes: what its knobs need.
    fn fixed_width(&self, group: &Group<'a, Self::Plot>, content: f32) -> f32 {
        let widths = self.line_widths(&group.knobs);
        let knobs = least_width(&widths, content, true)
            .or_else(|| least_width(&widths, content, false))
            .unwrap_or(LINE_ITEM_FALLBACK_W);
        knobs.max(self.header_width(group))
    }

    /// Groups across a band whose content is `height` tall and `width`
    /// wide: those without plots take what their knobs need, those with
    /// plots share the rest by how much they hold.
    fn band(
        &self,
        groups: Vec<Group<'a, Self::Plot>>,
        height: f32,
        width: f32,
    ) -> Element<'a, Message> {
        let content = height - HEADER_H - 4.0;
        let rules = (groups.len().saturating_sub(1)) as f32 * (1.0 + GROUP_GAP * 2.0);
        let weight = |group: &Group<'a, Self::Plot>| {
            group.knobs.len() as f32
                + match group.plot {
                    Some(plot) if plot.centrepiece() => 10.0,
                    Some(_) => 4.0,
                    None => 0.0,
                }
        };
        let fixed_widths: Vec<f32> = groups
            .iter()
            .map(|group| {
                if group.plot.is_none() {
                    self.fixed_width(group, content)
                } else {
                    0.0
                }
            })
            .collect();
        let fixed: f32 = fixed_widths.iter().sum();
        let shared: f32 = groups
            .iter()
            .filter(|group| group.plot.is_some())
            .map(weight)
            .sum();
        let room = (width - rules - fixed).max(0.0);
        let mut band = row![].spacing(GROUP_GAP);
        let count = groups.len();
        for (index, group) in groups.into_iter().enumerate() {
            let group_w = if group.plot.is_some() {
                room * weight(&group) / shared.max(1.0)
            } else {
                fixed_widths[index]
            };
            band = band.push(self.group(group, group_w, content));
            if index + 1 < count {
                band = band.push(vertical_rule());
            }
        }
        plot_band(band, height + BAND_PAD * 2.0, PAD)
    }

    fn knob_block(&self, knobs: &[usize], across: usize) -> Element<'a, Message> {
        let mut rows = column![].spacing(4);
        for chunk in knobs.chunks(across.max(1)) {
            let mut items = row![].spacing(6).align_y(Alignment::Center);
            for index in chunk {
                items = items.push(self.knob(*index, true));
            }
            rows = rows.push(
                container(items)
                    .height(KNOB_ROW_H - 4.0)
                    .center_y(KNOB_ROW_H - 4.0),
            );
        }
        rows.into()
    }

    fn lines_block(&self, knobs: &[usize], area: f32) -> Element<'a, Message> {
        let widths = self.line_widths(knobs);
        let mut stacked = column![].spacing(1);
        for line in pack(&widths, area, LINE_GAP) {
            let mut items = row![].spacing(LINE_GAP).align_y(Alignment::Center);
            for index in &knobs[line] {
                items = items.push(self.knob(*index, false));
            }
            stacked = stacked.push(container(items).height(LINE_H).center_y(LINE_H));
        }
        stacked.into()
    }

    fn controls(&self, knobs: &[usize], style: Knobs, area: f32) -> Element<'a, Message> {
        match style {
            Knobs::Grid(across) => self.knob_block(knobs, across),
            Knobs::Lines => self.lines_block(knobs, area),
        }
    }

    fn group(
        &self,
        group: Group<'a, Self::Plot>,
        width: f32,
        content: f32,
    ) -> Element<'a, Message> {
        // A note is the first thing a crowded header lets go.
        let note = group
            .note
            .clone()
            .filter(|_| self.header_width(&group) <= width);
        let mut header = row![heading(group.title)]
            .spacing(8)
            .align_y(Alignment::Center);
        if let Some(switch) = group.switch {
            header = header.push(switch);
        }
        for choice in &group.choices {
            header = header.push(self.menu(*choice));
        }
        header = header.push(Space::with_width(Length::Fill));
        if let Some(note) = note {
            header = header.push(text(note).size(LABEL_SIZE).color(th::text_dim()));
        }
        let knobs = &group.knobs;
        let widths = self.line_widths(knobs);
        let header = container(header).height(HEADER_H).center_y(HEADER_H);
        let controls = |style: Knobs, area: f32| self.controls(knobs, style, area);
        let Some(plot) = group.plot else {
            let style = arrange_knobs(&widths, width, content).unwrap_or(Knobs::Lines);
            return column![
                header,
                container(controls(style, width))
                    .width(width)
                    .height(content)
                    .center_x(width)
                    .center_y(content)
            ]
            .spacing(4)
            .width(width)
            .into();
        };
        let (comfortable, largest) = if plot.centrepiece() {
            (CENTREPIECE_GOOD_W, CENTREPIECE_MAX_W)
        } else {
            (PLOT_GOOD_W, PLOT_MAX_W)
        };
        let beside = |area: Option<f32>| {
            area.map(|area| {
                let gap = if knobs.is_empty() { 0.0 } else { GROUP_GAP };
                ((width - area - gap).min(largest), area)
            })
        };
        let with_grid = beside(least_width(&widths, content, true));
        let with_lines = beside(least_width(&widths, content, false));
        // Tall enough, the plot can sit over a row of knobs across the
        // group's width.
        let stacked = (!knobs.is_empty())
            .then(|| arrange_knobs(&widths, width, KNOB_ROW_H))
            .flatten()
            .filter(|_| content - KNOB_ROW_H - 4.0 >= STACKED_PLOT_MIN_H);
        let layout = match (with_grid, stacked, with_lines) {
            (Some((plot_w, area)), ..) if plot_w >= comfortable || knobs.is_empty() => {
                Layout::Beside(plot_w, area, Knobs::Grid(0))
            }
            (_, Some(style), _) if !plot.centrepiece() => Layout::Stacked(style),
            (_, _, Some((plot_w, area))) if plot_w >= PLOT_MIN_W => {
                Layout::Beside(plot_w, area, Knobs::Lines)
            }
            (_, Some(style), _) => Layout::Stacked(style),
            (Some((plot_w, area)), ..) if plot_w >= PLOT_MIN_W => {
                Layout::Beside(plot_w, area, Knobs::Grid(0))
            }
            _ => Layout::Alone,
        };
        let body: Element<'a, Message> = match layout {
            Layout::Beside(plot_w, area, style) => {
                let style = match style {
                    Knobs::Grid(_) => arrange_knobs(&widths, area, content).unwrap_or(Knobs::Lines),
                    Knobs::Lines => Knobs::Lines,
                };
                let mut body = row![container(self.plot(plot)).width(plot_w).height(content)]
                    .spacing(GROUP_GAP)
                    .height(content);
                if !knobs.is_empty() {
                    let area = width - plot_w - GROUP_GAP;
                    body = body.push(
                        container(controls(style, area))
                            .width(area)
                            .height(content)
                            .center_x(area)
                            .center_y(content),
                    );
                }
                body.into()
            }
            Layout::Stacked(style) => column![
                container(self.plot(plot))
                    .width(width)
                    .height(content - KNOB_ROW_H - 4.0),
                container(controls(style, width))
                    .width(width)
                    .height(KNOB_ROW_H)
                    .center_x(width)
                    .center_y(KNOB_ROW_H),
            ]
            .spacing(4)
            .into(),
            Layout::Alone => {
                let style = arrange_knobs(&widths, width, content).unwrap_or(Knobs::Lines);
                container(controls(style, width))
                    .width(width)
                    .height(content)
                    .center_x(width)
                    .center_y(content)
                    .into()
            }
        };
        column![header, body].spacing(4).width(width).into()
    }

    /// The shortest card's parts: each group's menus and knobs split into
    /// what fits one line `width` wide, named for the group and numbered
    /// from its second part.
    fn line_parts(&self, width: f32) -> Vec<LinePart> {
        let mut parts = Vec::new();
        for group in self.every_group() {
            let mut widths: Vec<f32> = group.choices.iter().map(|_| CHOICE_W).collect();
            widths.extend(self.line_widths(&group.knobs));
            let choices = group.choices.len();
            for (number, line) in pack(&widths, width, LINE_GAP).into_iter().enumerate() {
                let name = if number == 0 {
                    group.title.to_string()
                } else {
                    format!("{} ({})", group.title, number + 1)
                };
                let menus = group.choices[line.start.min(choices)..line.end.min(choices)].to_vec();
                let knobs = group.knobs
                    [line.start.saturating_sub(choices)..line.end.saturating_sub(choices)]
                    .to_vec();
                parts.push((name, menus, knobs));
            }
        }
        parts
    }

    /// The shortest card: a menu of its parts and the chosen part's
    /// controls on one line.
    fn line(&self, width: f32, height: f32) -> Element<'a, Message> {
        let parts = self.line_parts(width - PAGE_MENU_W - LINE_GAP);
        let chosen = self.line_part().min(parts.len().saturating_sub(1));
        let names: Vec<String> = parts.iter().map(|(name, ..)| name.clone()).collect();
        let track_id = self.track_id();
        let lookup = names.clone();
        let chosen_name = names.get(chosen).cloned().unwrap_or_default();
        let menu = container(dropdown(names, chosen_name, move |name| {
            let part = lookup
                .iter()
                .position(|candidate| *candidate == name)
                .unwrap_or(0);
            Message::InstrumentCard(InstrumentCardMsg::ShowLinePart(track_id, part))
        }))
        .width(PAGE_MENU_W);
        let mut line = row![menu].spacing(LINE_GAP).align_y(Alignment::Center);
        if let Some((_, menus, knobs)) = parts.into_iter().nth(chosen) {
            for choice in menus {
                line = line.push(self.menu(choice));
            }
            for index in knobs {
                line = line.push(self.knob(index, false));
            }
        }
        container(line)
            .padding([0.0, PAD])
            .height(height.max(0.0))
            .center_y(height.max(0.0))
            .into()
    }
}

/// The body an instrument card gets in the default 280 px detail panel and
/// its 180 px minimum, as measured in the app.
#[cfg(test)]
pub const DEFAULT_BODY_H: f32 = 168.0;
#[cfg(test)]
pub const MINIMUM_BODY_H: f32 = 68.0;
