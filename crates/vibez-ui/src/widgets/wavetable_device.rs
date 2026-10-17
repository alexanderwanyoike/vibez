//! Native Wavetable Synth body. Each tab is a band of groups, a plot beside
//! its controls, sharing the card's width: the oscillators with their
//! tables, the filter, the envelopes, or the LFO and the second envelope's
//! routing. A group's controls are knobs when the band is tall enough and
//! lines when not. Tall wide cards show the concept's overview on the
//! Oscillators tab, everything at once; the shortest set the tab menu and
//! the tab's controls on one line.

use iced::widget::{canvas, column, container, row, text, Space};
use iced::{Alignment, Color, Element, Length, Size};
use vibez_core::id::TrackId;
use vibez_instruments::wavetable::{param, resonance_q};
use vibez_instruments::wavetable_params::osc;

use crate::domains::device_params::DeviceParams;
use crate::domains::instrument_card::{InstrumentCardMsg, InstrumentPanel};
use crate::domains::param_entry::ValueEntry;
use crate::domains::wavetable::{
    WavetablePage, DEST_NAMES, FILTER_NAMES, LFO_SHAPE_NAMES, TABLE_NAMES,
};
use crate::message::Message;
use crate::theme as th;
use crate::widgets::device_controls::{
    dropdown, heading, plot_band, rule, segmented, tabs, text_width, vertical_rule, Param,
    LABEL_SIZE,
};
use crate::widgets::device_layout::{pack, KNOB_ROW_H, LINE_H};
use crate::widgets::synth_display::{EnvelopePlot, FilterPlot, LfoPlot};
use crate::widgets::wavetable_display::WavetablePlot;

const PAD: f32 = 10.0;
const PRESET_ROW_H: f32 = 28.0;
const TABS_H: f32 = 24.0;
const HEADER_H: f32 = 22.0;
/// What `plot_band` keeps clear above and below its content.
const BAND_PAD: f32 = 6.0;
const KNOB: f32 = 30.0;
/// A knob with its name and value under it.
const KNOB_W: f32 = 60.0;
/// A menu with its name above it.
const CHOICE_W: f32 = 110.0;
const LINE_GAP: f32 = 8.0;
const GROUP_GAP: f32 = 10.0;
const PLOT_MIN_W: f32 = 110.0;
const PLOT_MAX_W: f32 = 300.0;
/// The wavetable is the card's centrepiece, so it may grow further.
const TABLE_PLOT_MAX_W: f32 = 460.0;
/// Below these a plot no longer reads well enough to keep knobs beside it.
const TABLE_PLOT_GOOD_W: f32 = 220.0;
const PLOT_GOOD_W: f32 = 150.0;
const LINE_SLACK: f32 = 10.0;
/// The shortest a plot gets over its knobs.
const STACKED_PLOT_MIN_H: f32 = 56.0;
/// What a group takes when even lines cannot hold its knobs.
const LINE_ITEM_FALLBACK_W: f32 = 120.0;
/// A band's group header and one line of controls.
const BAND_MIN_H: f32 = HEADER_H + 4.0 + LINE_H;
/// The overview's bottom band: a header over a row of knobs.
const STRIP_H: f32 = HEADER_H + 4.0 + KNOB_ROW_H + BAND_PAD * 2.0;
/// The overview needs two rows of knobs beside each oscillator's plot.
const OVERVIEW_MIN_H: f32 =
    TABS_H + 1.0 + HEADER_H + 4.0 + KNOB_ROW_H * 2.0 + BAND_PAD * 2.0 + 1.0 + STRIP_H;
const PAGE_MENU_W: f32 = 112.0;

pub const COMPACT_W: f32 = 440.0;
/// Both oscillators side by side.
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
    /// The tab menu and the tab's controls on one line.
    Line,
}

pub fn arrange(size: Size, page: WavetablePage) -> Form {
    let below = (size.height - PRESET_ROW_H - 1.0).max(0.0);
    let tabs = TABS_H + 1.0;
    if Tier::for_width(size.width) == Tier::Full
        && page == WavetablePage::Oscillators
        && below >= OVERVIEW_MIN_H
    {
        return Form::Overview(below - tabs - BAND_PAD * 2.0 - 1.0 - STRIP_H);
    }
    if below >= tabs + BAND_MIN_H + BAND_PAD * 2.0 {
        return Form::Band(below - tabs - BAND_PAD * 2.0);
    }
    Form::Line
}

#[derive(Clone, Copy)]
enum Plot {
    Table(usize),
    Filter,
    Envelope(usize),
    Lfo,
}

/// How a group's knobs are set: a grid so many across, or on lines.
#[derive(Clone, Copy)]
enum Knobs {
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
type Choice = (usize, &'static [&'static str]);

/// A plot beside its knobs, under a header that holds the group's menus.
struct Group<'a> {
    title: &'a str,
    note: Option<String>,
    /// A switch between two things the group can show.
    switch: Option<Element<'a, Message>>,
    choices: Vec<Choice>,
    plot: Option<Plot>,
    knobs: Vec<usize>,
}

/// An oscillator's knobs.
fn osc_knobs(osc: usize) -> Vec<usize> {
    [
        osc::POSITION,
        osc::WARP,
        osc::DETUNE,
        osc::UNISON,
        osc::LEVEL,
        osc::OCTAVE,
    ]
    .map(|control| param::of(osc, control))
    .to_vec()
}

fn envelope(first: usize) -> Vec<usize> {
    (first..first + 4).collect()
}

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

pub struct WavetableView<'a> {
    pub track_id: TrackId,
    pub device: DeviceParams<'a>,
    pub accent: Color,
    pub panel: InstrumentPanel,
    pub entry: Option<&'a ValueEntry>,
}

impl<'a> WavetableView<'a> {
    fn param(&self, index: usize) -> Param<'a> {
        let param = Param::new(self.track_id, self.device, index, self.accent, self.entry);
        let label = match index {
            param::ATTACK | param::ENV2_ATTACK => "Attack",
            param::DECAY | param::ENV2_DECAY => "Decay",
            param::SUSTAIN | param::ENV2_SUSTAIN => "Sustain",
            param::RELEASE | param::ENV2_RELEASE => "Release",
            param::LFO_RATE => "Rate",
            param::LFO_AMOUNT | param::ENV2_AMOUNT => "Amount",
            param::LFO_DEST | param::ENV2_DEST => "Destination",
            param::LFO_SHAPE => "Shape",
            param::FILTER_TYPE => "Type",
            index if index < param::FILTER_TYPE => match index % osc::COUNT {
                osc::TABLE => "Wavetable",
                osc::POSITION => "Position",
                osc::WARP => "Warp",
                osc::DETUNE => "Detune",
                osc::UNISON => "Unison",
                osc::LEVEL => "Level",
                _ => "Octave",
            },
            _ => return param,
        };
        param.label(label)
    }

    fn page(&self) -> WavetablePage {
        WavetablePage::at(self.panel.page)
    }

    fn value(&self, index: usize) -> f32 {
        self.device.value(index)
    }

    fn whole(&self, index: usize) -> usize {
        self.value(index).round().max(0.0) as usize
    }

    /// The oscillator a narrow card shows.
    fn shown_osc(&self) -> usize {
        usize::from(self.panel.second_oscillator)
    }

    fn osc_group(&self, osc: usize, switch: bool) -> Group<'a> {
        let note = format!(
            "Frame {:.0}%",
            self.value(param::of(osc, osc::POSITION)) * 100.0
        );
        let switch = switch.then(|| {
            let track_id = self.track_id;
            segmented(
                &[(false, "Osc A"), (true, "Osc B")],
                osc == 1,
                move |second| {
                    Message::InstrumentCard(InstrumentCardMsg::ShowSecondOscillator(
                        track_id, second,
                    ))
                },
                self.accent,
            )
        });
        Group {
            title: if osc == 0 { "Osc A" } else { "Osc B" },
            note: Some(note),
            switch,
            choices: vec![(param::of(osc, osc::TABLE), &TABLE_NAMES)],
            plot: Some(Plot::Table(osc)),
            knobs: osc_knobs(osc),
        }
    }

    /// An oscillator's table and knobs without its plot, beside the shared
    /// one in the overview.
    fn osc_knobs_group(&self, osc: usize) -> Group<'a> {
        Group {
            plot: None,
            note: None,
            ..self.osc_group(osc, false)
        }
    }

    /// The overview's one wavetable, for the oscillator a switch picks.
    fn table_group(&self) -> Group<'a> {
        let osc = self.shown_osc();
        Group {
            title: "Wavetable",
            choices: Vec::new(),
            knobs: Vec::new(),
            ..self.osc_group(osc, true)
        }
    }

    fn filter_group(&self) -> Group<'a> {
        Group {
            title: "Filter",
            note: None,
            switch: None,
            choices: vec![(param::FILTER_TYPE, &FILTER_NAMES)],
            plot: Some(Plot::Filter),
            knobs: vec![param::CUTOFF, param::RESONANCE, param::DRIVE],
        }
    }

    fn envelope_group(&self, second: bool, switch: bool) -> Group<'a> {
        let first = if second {
            param::ENV2_ATTACK
        } else {
            param::ATTACK
        };
        let switch = switch.then(|| {
            let track_id = self.track_id;
            segmented(
                &[(false, "Amp"), (true, "Env 2")],
                second,
                move |second| {
                    Message::InstrumentCard(InstrumentCardMsg::ShowSecondEnvelope(track_id, second))
                },
                self.accent,
            )
        });
        Group {
            title: if second { "Env 2" } else { "Amp Envelope" },
            note: None,
            switch,
            choices: Vec::new(),
            plot: Some(Plot::Envelope(first)),
            knobs: envelope(first),
        }
    }

    fn lfo_group(&self) -> Group<'a> {
        Group {
            title: "LFO 1",
            note: None,
            switch: None,
            choices: vec![
                (param::LFO_SHAPE, &LFO_SHAPE_NAMES),
                (param::LFO_DEST, &DEST_NAMES),
            ],
            plot: Some(Plot::Lfo),
            knobs: vec![param::LFO_RATE, param::LFO_AMOUNT],
        }
    }

    fn env2_routing(&self) -> Group<'a> {
        Group {
            title: "Env 2",
            note: None,
            switch: None,
            choices: vec![(param::ENV2_DEST, &DEST_NAMES)],
            plot: None,
            knobs: vec![param::ENV2_AMOUNT],
        }
    }

    fn voice_group(&self) -> Group<'a> {
        Group {
            title: "Voice",
            note: None,
            switch: None,
            choices: Vec::new(),
            plot: None,
            knobs: vec![param::VOICES, param::OUTPUT_DB],
        }
    }

    /// The groups a tab shows on a card this wide.
    fn groups(&self, page: WavetablePage, tier: Tier) -> Vec<Group<'a>> {
        let wide = tier != Tier::Compact;
        match page {
            WavetablePage::Oscillators if tier == Tier::Full => vec![
                self.osc_group(0, false),
                self.osc_group(1, false),
                self.voice_group(),
            ],
            WavetablePage::Oscillators if wide => {
                vec![self.osc_group(self.shown_osc(), true), self.voice_group()]
            }
            // A compact card keeps the oscillator's whole width for its
            // plot; the voices join the envelopes instead.
            WavetablePage::Oscillators => vec![self.osc_group(self.shown_osc(), true)],
            WavetablePage::Filter => vec![self.filter_group()],
            WavetablePage::Envelopes if wide => vec![
                self.envelope_group(false, false),
                self.envelope_group(true, false),
            ],
            WavetablePage::Envelopes => vec![
                self.envelope_group(self.panel.second_envelope, true),
                self.voice_group(),
            ],
            WavetablePage::Modulation => vec![self.lfo_group(), self.env2_routing()],
        }
    }

    pub fn view(self, preset_bar: Element<'a, Message>, size: Size) -> Element<'a, Message> {
        let form = arrange(size, self.page());
        let tier = Tier::for_width(size.width);
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
        let track_id = self.track_id;
        let pages: Vec<(WavetablePage, &str)> = WavetablePage::ALL
            .iter()
            .map(|page| (*page, page.name()))
            .collect();
        body = body
            .push(
                container(tabs(
                    &pages,
                    self.page(),
                    move |page| {
                        Message::InstrumentCard(InstrumentCardMsg::ShowPage(track_id, page.index()))
                    },
                    self.accent,
                ))
                .padding([0.0, PAD])
                .height(TABS_H)
                .align_y(Alignment::Center),
            )
            .push(rule());
        match form {
            Form::Overview(top) => {
                let groups = vec![
                    self.osc_knobs_group(0),
                    self.table_group(),
                    self.osc_knobs_group(1),
                    self.filter_group(),
                    self.envelope_group(false, false),
                ];
                // Wide enough here to show Env 2's shape beside its routing,
                // as the LFO shows its wave.
                let env2 = Group {
                    plot: Some(Plot::Envelope(param::ENV2_ATTACK)),
                    ..self.env2_routing()
                };
                let strip = vec![self.lfo_group(), env2, self.voice_group()];
                body = body
                    .push(self.band(groups, top, inner))
                    .push(rule())
                    .push(self.band(strip, STRIP_H - BAND_PAD * 2.0, inner));
            }
            Form::Band(height) => {
                body = body.push(self.band(self.groups(self.page(), tier), height, inner));
            }
            Form::Line => {}
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

    fn plot(&self, plot: Plot) -> Element<'a, Message> {
        let accent = self.accent;
        let program: Element<'a, Message> = match plot {
            Plot::Table(osc) => canvas(WavetablePlot {
                table: self.whole(param::of(osc, osc::TABLE)),
                position: self.value(param::of(osc, osc::POSITION)),
                warp: self.value(param::of(osc, osc::WARP)),
                accent,
            })
            .width(Length::Fill)
            .height(Length::Fill)
            .into(),
            Plot::Filter => canvas(FilterPlot {
                filter_type: Some(self.whole(param::FILTER_TYPE)),
                cutoff: self.value(param::CUTOFF),
                resonance: resonance_q(self.value(param::RESONANCE)),
                accent,
            })
            .width(Length::Fill)
            .height(Length::Fill)
            .into(),
            Plot::Envelope(first) => canvas(EnvelopePlot {
                attack: self.value(first),
                decay: self.value(first + 1),
                sustain: self.value(first + 2),
                release: self.value(first + 3),
                accent,
            })
            .width(Length::Fill)
            .height(Length::Fill)
            .into(),
            Plot::Lfo => canvas(LfoPlot {
                shape: self.whole(param::LFO_SHAPE),
                accent,
            })
            .width(Length::Fill)
            .height(Length::Fill)
            .into(),
        };
        container(program)
            .width(Length::Fill)
            .height(Length::Fill)
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
    fn header_width(&self, group: &Group) -> f32 {
        text_width(group.title, LABEL_SIZE, iced::Font::DEFAULT)
            + group.choices.len() as f32 * (CHOICE_W + 8.0)
            + if group.switch.is_some() { 120.0 } else { 0.0 }
            + if group.note.is_some() { 70.0 } else { 0.0 }
    }

    /// The width a group without a plot takes: what its knobs need.
    fn fixed_width(&self, group: &Group, content: f32) -> f32 {
        let widths = self.line_widths(&group.knobs);
        let knobs = least_width(&widths, content, true)
            .or_else(|| least_width(&widths, content, false))
            .unwrap_or(LINE_ITEM_FALLBACK_W);
        knobs.max(self.header_width(group))
    }

    /// Groups across a band whose content is `height` tall and `width`
    /// wide: those without plots take what their knobs need, those with
    /// plots share the rest by how much they hold.
    fn band(&self, groups: Vec<Group<'a>>, height: f32, width: f32) -> Element<'a, Message> {
        let content = height - HEADER_H - 4.0;
        let rules = (groups.len().saturating_sub(1)) as f32 * (1.0 + GROUP_GAP * 2.0);
        let weight = |group: &Group| {
            group.knobs.len() as f32
                + match group.plot {
                    Some(Plot::Table(_)) => 10.0,
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

    fn knob_block(&self, knobs: &[usize], style: Knobs) -> Element<'a, Message> {
        match style {
            Knobs::Grid(across) => {
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
            Knobs::Lines => column![].into(),
        }
    }

    fn lines_block(&self, knobs: &[usize], area: f32) -> Element<'a, Message> {
        let widths: Vec<f32> = knobs
            .iter()
            .map(|index| self.param(*index).line_width())
            .collect();
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
            Knobs::Grid(_) => self.knob_block(knobs, style),
            Knobs::Lines => self.lines_block(knobs, area),
        }
    }

    fn group(&self, group: Group<'a>, width: f32, content: f32) -> Element<'a, Message> {
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
        let Some(plot) = group.plot else {
            let style = arrange_knobs(&widths, width, content).unwrap_or(Knobs::Lines);
            return column![
                header,
                container(self.controls(knobs, style, width))
                    .width(width)
                    .height(content)
                    .center_x(width)
                    .center_y(content)
            ]
            .spacing(4)
            .width(width)
            .into();
        };
        let (comfortable, largest) = match plot {
            Plot::Table(_) => (TABLE_PLOT_GOOD_W, TABLE_PLOT_MAX_W),
            _ => (PLOT_GOOD_W, PLOT_MAX_W),
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
            (_, Some(style), _) if !matches!(plot, Plot::Table(_)) => Layout::Stacked(style),
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
                        container(self.controls(knobs, style, area))
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
                container(self.controls(knobs, style, width))
                    .width(width)
                    .height(KNOB_ROW_H)
                    .center_x(width)
                    .center_y(KNOB_ROW_H),
            ]
            .spacing(4)
            .into(),
            Layout::Alone => {
                let style = arrange_knobs(&widths, width, content).unwrap_or(Knobs::Lines);
                container(self.controls(knobs, style, width))
                    .width(width)
                    .height(content)
                    .center_x(width)
                    .center_y(content)
                    .into()
            }
        };
        column![header, body].spacing(4).width(width).into()
    }

    /// Every group once, named apart, in the order the tabs show them, for
    /// the shortest card to step through.
    fn every_group(&self) -> Vec<Group<'a>> {
        vec![
            self.osc_group(0, false),
            self.osc_group(1, false),
            self.filter_group(),
            self.envelope_group(false, false),
            Group {
                title: "Env 2 Envelope",
                ..self.envelope_group(true, false)
            },
            self.lfo_group(),
            Group {
                title: "Env 2 Routing",
                ..self.env2_routing()
            },
            self.voice_group(),
        ]
    }

    /// The shortest card's parts: each group's menus and knobs split into
    /// what fits one line `width` wide, named for the group and numbered
    /// from its second part.
    fn line_parts(&self, width: f32) -> Vec<(String, Vec<Choice>, Vec<usize>)> {
        let mut parts = Vec::new();
        for group in self.every_group() {
            let mut widths: Vec<f32> = group.choices.iter().map(|_| CHOICE_W).collect();
            widths.extend(
                group
                    .knobs
                    .iter()
                    .map(|index| self.param(*index).line_width()),
            );
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
        let chosen = self.panel.line_part.min(parts.len().saturating_sub(1));
        let names: Vec<String> = parts.iter().map(|(name, ..)| name.clone()).collect();
        let track_id = self.track_id;
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The body an instrument card gets in the default 280 px detail panel
    /// and its 180 px minimum, as measured in the app.
    const DEFAULT_BODY_H: f32 = 168.0;
    const MINIMUM_BODY_H: f32 = 68.0;

    #[test]
    fn the_default_panel_shows_a_band_with_room_for_knobs() {
        for width in [COMPACT_W, MEDIUM_W, FULL_W, MAX_W] {
            for page in WavetablePage::ALL {
                let Form::Band(height) = arrange(Size::new(width, DEFAULT_BODY_H), page) else {
                    panic!("{width} {page:?}");
                };
                assert!(
                    height - HEADER_H - 4.0 + 4.0 >= KNOB_ROW_H,
                    "{width} {page:?}: {height}"
                );
            }
        }
    }

    #[test]
    fn the_minimum_panel_sets_the_tab_on_a_line() {
        for width in [COMPACT_W, MEDIUM_W, FULL_W] {
            assert_eq!(
                arrange(Size::new(width, MINIMUM_BODY_H), WavetablePage::Oscillators),
                Form::Line
            );
        }
    }

    #[test]
    fn tall_wide_cards_show_the_overview_on_the_oscillators_tab() {
        let tall = Size::new(FULL_W, 420.0);
        assert!(matches!(
            arrange(tall, WavetablePage::Oscillators),
            Form::Overview(top) if top >= HEADER_H + 4.0 + KNOB_ROW_H * 2.0
        ));
        assert!(matches!(
            arrange(tall, WavetablePage::Filter),
            Form::Band(_)
        ));
        assert!(matches!(
            arrange(Size::new(MEDIUM_W, 420.0), WavetablePage::Oscillators),
            Form::Band(_)
        ));
    }

    #[test]
    fn the_shortest_card_reaches_every_control_once() {
        let mut track = crate::state::ProjectTrack::new(TrackId::new(), "Pads".into(), 0);
        track.has_instrument = true;
        track.instrument_kind = Some(vibez_core::midi::InstrumentKind::WavetableSynth);
        let device = DeviceParams::instrument(&track).unwrap();
        let view = WavetableView {
            track_id: track.id,
            device,
            accent: Color::WHITE,
            panel: InstrumentPanel::default(),
            entry: None,
        };
        for width in [COMPACT_W, FULL_W] {
            let mut reached: Vec<usize> = view
                .line_parts(width - PAD * 2.0 - PAGE_MENU_W - LINE_GAP)
                .into_iter()
                .flat_map(|(_, menus, knobs)| {
                    menus.into_iter().map(|(index, _)| index).chain(knobs)
                })
                .collect();
            reached.sort_unstable();
            assert_eq!(reached, (0..param::COUNT).collect::<Vec<_>>(), "{width}");
        }
    }

    #[test]
    fn every_supported_size_fits_without_scrolling() {
        for width in [COMPACT_W, MEDIUM_W, FULL_W, MAX_W] {
            for height in (MINIMUM_BODY_H as usize..=480).step_by(2).map(|h| h as f32) {
                for page in WavetablePage::ALL {
                    let used = PRESET_ROW_H
                        + 1.0
                        + match arrange(Size::new(width, height), page) {
                            Form::Band(content) => TABS_H + 1.0 + content + BAND_PAD * 2.0,
                            Form::Overview(top) => {
                                TABS_H + 1.0 + top + BAND_PAD * 2.0 + 1.0 + STRIP_H
                            }
                            Form::Line => LINE_H,
                        };
                    assert!(used <= height + 0.01, "{width}x{height} {page:?}");
                }
            }
        }
    }
}
