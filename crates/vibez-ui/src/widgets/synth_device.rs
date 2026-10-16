//! Native Subtractive Synth body. Wide cards show the two oscillators, the
//! filter and an envelope side by side above the voice, mix, LFO and volume
//! controls; narrower ones put the sections behind tabs, showing one beside
//! the level envelope, or one alone. Short cards drop the plots, then put
//! the knobs on a line.

use iced::widget::{column, container, row, Space};
use iced::{Alignment, Color, Element, Length, Size};
use vibez_core::id::TrackId;
use vibez_instruments::synth::{param, Waveform};

use crate::domains::device_params::DeviceParams;
use crate::domains::param_entry::ValueEntry;
use crate::domains::synth::{
    SynthMsg, SynthPage, SynthPanel, FILTER_NAMES, LFO_SHAPE_NAMES, LFO_TARGET_NAMES,
    VOICE_MODE_NAMES, WAVE_NAMES,
};
use crate::message::Message;
use crate::widgets::device_controls::{
    heading, plot_band, rule, segmented, tabs, vertical_rule, Param,
};
use crate::widgets::device_layout::{KNOB_ROW_H, LINE_H};
use crate::widgets::synth_display::{EnvelopePlot, FilterPlot, LfoPlot, WavePlot};

const PAD: f32 = 10.0;
const PRESET_ROW_H: f32 = 28.0;
const TABS_H: f32 = 24.0;
/// A section's name and its choice, above its plot.
const HEADER_H: f32 = 26.0;
const KNOB: f32 = 30.0;
const PLOT_MIN_H: f32 = 40.0;
const CHOICE_W: f32 = 92.0;
/// One labelled value and knob on a single-line row.
const LINE_ITEM_W: f32 = 120.0;
const LINE_GAP: f32 = 12.0;

pub const COMPACT_W: f32 = 440.0;
/// A section beside the level envelope.
pub const MEDIUM_W: f32 = 700.0;
/// Every section at once.
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
    Full,
    Medium,
    Compact,
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

/// How a body's height divides: the tabs, the sections (their header, plot
/// and knobs) and the shared row beneath them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Layout {
    pub tier: Tier,
    pub tabs: bool,
    /// Zero when the sections do not fit at all.
    pub sections: f32,
    /// Zero when the sections have no room for their plots.
    pub plot: f32,
    /// Knobs with their names, or a line when shorter.
    pub knobs: f32,
    pub shared: f32,
}

pub fn arrange(size: Size) -> Layout {
    let tier = Tier::for_width(size.width);
    let below = (size.height - PRESET_ROW_H - 1.0).max(0.0);
    let tabs_h = if tier == Tier::Full {
        0.0
    } else {
        TABS_H + 1.0
    };
    let with_shared = |shared: f32| {
        let room = below - tabs_h - shared - 1.0;
        let knobs = if room >= HEADER_H + KNOB_ROW_H {
            KNOB_ROW_H
        } else {
            LINE_H
        };
        let plot = room - HEADER_H - knobs;
        Layout {
            tier,
            tabs: tier != Tier::Full,
            sections: room,
            plot: if plot >= PLOT_MIN_H { plot } else { 0.0 },
            knobs,
            shared,
        }
    };
    // The plots matter more than the shared row's knobs, so that row folds
    // onto a line first when it lets them in.
    let mut layout = with_shared(KNOB_ROW_H);
    if layout.plot == 0.0 {
        layout = with_shared(LINE_H);
    }
    if layout.sections < HEADER_H + LINE_H {
        // Too short for any section, so their tabs would choose nothing.
        return Layout {
            tier,
            tabs: false,
            sections: 0.0,
            plot: 0.0,
            knobs: 0.0,
            shared: LINE_H.min(below),
        };
    }
    layout
}

pub struct SynthView<'a> {
    pub track_id: TrackId,
    pub device: DeviceParams<'a>,
    pub accent: Color,
    pub panel: SynthPanel,
    pub entry: Option<&'a ValueEntry>,
}

impl<'a> SynthView<'a> {
    fn param(&self, index: usize) -> Param<'a> {
        let param = Param::new(self.track_id, self.device, index, self.accent, self.entry);
        match index {
            param::CUTOFF => param.label("Cutoff"),
            param::RESONANCE => param.label("Resonance"),
            param::OCTAVE | param::OCTAVE_2 => param.label("Octave"),
            param::DETUNE | param::DETUNE_2 => param.label("Detune"),
            param::LEVEL_2 => param.label("Level"),
            param::FILTER_ENV => param.label("Env"),
            param::LFO_RATE => param.label("Rate"),
            param::LFO_AMOUNT => param.label("Amount"),
            param::ATTACK | param::FILTER_ATTACK => param.label("Attack"),
            param::DECAY | param::FILTER_DECAY => param.label("Decay"),
            param::SUSTAIN | param::FILTER_SUSTAIN => param.label("Sustain"),
            param::RELEASE | param::FILTER_RELEASE => param.label("Release"),
            _ => param,
        }
    }

    fn choice(&self, index: usize, names: &'static [&'static str]) -> Element<'a, Message> {
        container(self.param(index).choices(names, 0))
            .width(CHOICE_W)
            .into()
    }

    fn whole(&self, index: usize) -> usize {
        self.device.value(index).round().max(0.0) as usize
    }

    pub fn view(self, preset_bar: Element<'a, Message>, size: Size) -> Element<'a, Message> {
        let layout = arrange(size);
        let mut body = column![
            container(preset_bar)
                .padding([0.0, PAD])
                .height(PRESET_ROW_H)
                .align_y(Alignment::Center),
            rule(),
        ];
        if layout.tabs {
            let track_id = self.track_id;
            let pages: Vec<(SynthPage, &str)> = SynthPage::ALL
                .iter()
                .map(|page| (*page, page.name()))
                .collect();
            body = body
                .push(
                    container(tabs(
                        &pages,
                        self.panel.page,
                        move |page| Message::Synth(SynthMsg::ShowPage(track_id, page)),
                        self.accent,
                    ))
                    .padding([0.0, PAD])
                    .height(TABS_H)
                    .align_y(Alignment::Center),
                )
                .push(rule());
        }
        if layout.sections > 0.0 {
            body = body.push(self.sections(layout)).push(rule());
        }
        body = body.push(self.shared(layout, size.width));
        body.width(Length::Fill).height(Length::Fill).into()
    }

    fn sections(&self, layout: Layout) -> Element<'a, Message> {
        let content: Element<'a, Message> = match (layout.tier, self.panel.page) {
            (Tier::Full, _) => row![
                self.oscillator(layout, false),
                vertical_rule(),
                self.oscillator(layout, true),
                vertical_rule(),
                self.filter(layout),
                vertical_rule(),
                self.envelope(layout, self.panel.filter_envelope, true),
            ]
            .spacing(10)
            .into(),
            (Tier::Medium, SynthPage::Envelope) => row![
                self.envelope(layout, false, false),
                vertical_rule(),
                self.envelope(layout, true, false),
            ]
            .spacing(10)
            .into(),
            (Tier::Medium, page) => row![
                self.page(layout, page),
                vertical_rule(),
                self.envelope(layout, false, false),
            ]
            .spacing(10)
            .into(),
            (Tier::Compact, SynthPage::Envelope) => {
                self.envelope(layout, self.panel.filter_envelope, true)
            }
            (Tier::Compact, page) => self.page(layout, page),
        };
        plot_band(content, layout.sections, PAD)
    }

    fn page(&self, layout: Layout, page: SynthPage) -> Element<'a, Message> {
        match page {
            SynthPage::Oscillator => {
                let mut oscillators = row![
                    self.oscillator(layout, false),
                    vertical_rule(),
                    self.oscillator(layout, true),
                ]
                .spacing(10);
                if layout.tier == Tier::Compact {
                    oscillators = oscillators
                        .push(vertical_rule())
                        .push(self.knobs(layout, &[param::SUB, param::NOISE]));
                }
                oscillators.width(Length::Fill).into()
            }
            SynthPage::Filter => self.filter(layout),
            SynthPage::Envelope => self.envelope(layout, false, false),
            SynthPage::Modulation => self.modulation(layout),
        }
    }

    /// A section: its name and choice, its plot when there is room, and
    /// its knobs.
    fn section(
        &self,
        layout: Layout,
        header: Element<'a, Message>,
        plot: Option<Element<'a, Message>>,
        knobs: &[usize],
    ) -> Element<'a, Message> {
        let mut section = column![container(header)
            .height(HEADER_H)
            .align_y(Alignment::Center)];
        if let Some(plot) = plot.filter(|_| layout.plot > 0.0) {
            section = section.push(container(plot).height(layout.plot).width(Length::Fill));
        }
        section
            .push(self.knobs(layout, knobs))
            .width(Length::Fill)
            .into()
    }

    fn knobs(&self, layout: Layout, indices: &[usize]) -> Element<'a, Message> {
        let mut knobs = row![].spacing(6).align_y(Alignment::Center);
        for &index in indices {
            knobs = knobs.push(if layout.knobs < KNOB_ROW_H {
                self.param(index).line_item()
            } else {
                container(self.param(index).column(KNOB))
                    .width(Length::Fill)
                    .align_x(Alignment::Center)
                    .into()
            });
        }
        container(knobs)
            .height(layout.knobs)
            .center_y(layout.knobs)
            .into()
    }

    fn oscillator(&self, layout: Layout, second: bool) -> Element<'a, Message> {
        let (name, wave, knobs): (&str, usize, &[usize]) = if second {
            (
                "Osc 2",
                param::WAVEFORM_2,
                &[param::OCTAVE_2, param::DETUNE_2, param::LEVEL_2],
            )
        } else {
            ("Osc 1", param::WAVEFORM, &[param::OCTAVE, param::DETUNE])
        };
        let header = row![
            heading(name),
            Space::with_width(Length::Fill),
            self.choice(wave, &WAVE_NAMES)
        ]
        .spacing(6)
        .align_y(Alignment::Center);
        let plot = iced::widget::canvas(WavePlot {
            waveform: Waveform::from_index(self.whole(wave)),
            accent: self.accent,
        })
        .width(Length::Fill)
        .height(Length::Fill);
        self.section(layout, header.into(), Some(plot.into()), knobs)
    }

    fn filter(&self, layout: Layout) -> Element<'a, Message> {
        let header = row![
            heading("Filter"),
            Space::with_width(Length::Fill),
            self.choice(param::FILTER_TYPE, &FILTER_NAMES)
        ]
        .spacing(6)
        .align_y(Alignment::Center);
        let plot = iced::widget::canvas(FilterPlot {
            params: self.device.params,
            accent: self.accent,
        })
        .width(Length::Fill)
        .height(Length::Fill);
        self.section(
            layout,
            header.into(),
            Some(plot.into()),
            &[param::CUTOFF, param::RESONANCE, param::FILTER_ENV],
        )
    }

    /// The level's envelope or the filter's, with a switch between them
    /// when `switchable`.
    fn envelope(&self, layout: Layout, filter: bool, switchable: bool) -> Element<'a, Message> {
        let indices = if filter {
            [
                param::FILTER_ATTACK,
                param::FILTER_DECAY,
                param::FILTER_SUSTAIN,
                param::FILTER_RELEASE,
            ]
        } else {
            [param::ATTACK, param::DECAY, param::SUSTAIN, param::RELEASE]
        };
        let header: Element<'a, Message> = if switchable {
            let track_id = self.track_id;
            row![
                heading("Envelope"),
                Space::with_width(Length::Fill),
                segmented(
                    &[(false, "Amp"), (true, "Filter")],
                    filter,
                    move |filter| Message::Synth(SynthMsg::ShowFilterEnvelope(track_id, filter)),
                    self.accent,
                )
            ]
            .spacing(6)
            .align_y(Alignment::Center)
            .into()
        } else {
            heading(if filter {
                "Envelope (Filter)"
            } else {
                "Envelope (Amp)"
            })
            .into()
        };
        let value = |index: usize| self.device.value(index);
        let plot = iced::widget::canvas(EnvelopePlot {
            attack: value(indices[0]),
            decay: value(indices[1]),
            sustain: value(indices[2]),
            release: value(indices[3]),
            accent: self.accent,
        })
        .width(Length::Fill)
        .height(Length::Fill);
        self.section(layout, header, Some(plot.into()), &indices)
    }

    fn modulation(&self, layout: Layout) -> Element<'a, Message> {
        let header = row![
            heading("LFO"),
            Space::with_width(Length::Fill),
            self.choice(param::LFO_SHAPE, &LFO_SHAPE_NAMES),
            heading("to"),
            self.choice(param::LFO_TARGET, &LFO_TARGET_NAMES),
        ]
        .spacing(6)
        .align_y(Alignment::Center);
        let plot = iced::widget::canvas(LfoPlot {
            shape: self.whole(param::LFO_SHAPE),
            accent: self.accent,
        })
        .width(Length::Fill)
        .height(Length::Fill);
        self.section(
            layout,
            header.into(),
            Some(plot.into()),
            &[param::LFO_RATE, param::LFO_AMOUNT],
        )
    }

    /// The voice, mix, LFO and volume controls every tier keeps in reach,
    /// beneath the sections.
    fn shared(&self, layout: Layout, width: f32) -> Element<'a, Message> {
        let mut knobs = match layout.tier {
            Tier::Full => vec![
                param::VOICES,
                param::GLIDE,
                param::SUB,
                param::NOISE,
                param::LFO_RATE,
                param::LFO_AMOUNT,
            ],
            Tier::Medium => vec![param::VOICES, param::GLIDE, param::SUB, param::NOISE],
            Tier::Compact => vec![param::VOICES, param::GLIDE],
        };
        knobs.push(param::VOLUME);
        if layout.shared < KNOB_ROW_H {
            let mut room = width - PAD * 2.0;
            let mut line = row![].spacing(LINE_GAP).align_y(Alignment::Center);
            let mode = self.choice(param::VOICE_MODE, &VOICE_MODE_NAMES);
            let items = std::iter::once((mode, CHOICE_W)).chain(
                [param::VOLUME]
                    .into_iter()
                    .chain(knobs.into_iter().filter(|index| *index != param::VOLUME))
                    .map(|index| (self.param(index).line_item(), LINE_ITEM_W)),
            );
            for (control, width) in items {
                if width > room {
                    break;
                }
                room -= width + LINE_GAP;
                line = line.push(control);
            }
            return container(line)
                .padding([0.0, PAD])
                .height(layout.shared)
                .center_y(layout.shared)
                .into();
        }
        let column = |control: Element<'a, Message>| -> Element<'a, Message> {
            container(control)
                .width(Length::Fill)
                .max_width(110.0)
                .align_x(Alignment::Center)
                .into()
        };
        let mut shared = row![column(
            column![
                heading("Voice Mode"),
                self.choice(param::VOICE_MODE, &VOICE_MODE_NAMES)
            ]
            .spacing(4)
            .into()
        )]
        .spacing(0)
        .align_y(Alignment::Center);
        for index in knobs {
            if index == param::LFO_RATE {
                shared = shared.push(vertical_rule()).push(column(
                    column![
                        heading("LFO"),
                        self.choice(param::LFO_SHAPE, &LFO_SHAPE_NAMES),
                        self.choice(param::LFO_TARGET, &LFO_TARGET_NAMES),
                    ]
                    .spacing(3)
                    .into(),
                ));
            }
            if index == param::VOLUME {
                shared = shared.push(vertical_rule());
            }
            shared = shared.push(column(self.param(index).column(KNOB)));
        }
        container(shared)
            .padding([4.0, PAD])
            .height(layout.shared)
            .center_y(layout.shared)
            .into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_supported_size_fits_without_scrolling() {
        for width in [COMPACT_W, MEDIUM_W, FULL_W, MAX_W] {
            for height in (56..=480).step_by(2).map(|height| height as f32) {
                let layout = arrange(Size::new(width, height));
                let context = format!("{width}x{height}: {layout:?}");
                let mut used = PRESET_ROW_H + 1.0 + layout.shared;
                if layout.tabs {
                    used += TABS_H + 1.0;
                }
                if layout.sections > 0.0 {
                    used += layout.sections + 1.0;
                    assert!(
                        HEADER_H + layout.plot + layout.knobs <= layout.sections + 0.01,
                        "{context}"
                    );
                    assert!(layout.plot == 0.0 || layout.plot >= PLOT_MIN_H, "{context}");
                }
                assert!(used <= height + 0.01, "{context}");
            }
        }
    }

    #[test]
    fn wide_cards_show_every_section_and_narrow_ones_use_tabs() {
        assert!(!arrange(Size::new(FULL_W, 360.0)).tabs);
        assert!(arrange(Size::new(MEDIUM_W, 360.0)).tabs);
        let tall = arrange(Size::new(COMPACT_W, 360.0));
        assert!(tall.plot > 0.0 && tall.knobs == KNOB_ROW_H && tall.shared == KNOB_ROW_H);
        // A 360 px detail panel leaves a card about 250 px of body.
        let panel = arrange(Size::new(MEDIUM_W, 250.0));
        assert!(panel.plot > 0.0 && panel.knobs == KNOB_ROW_H, "{panel:?}");
    }
}
