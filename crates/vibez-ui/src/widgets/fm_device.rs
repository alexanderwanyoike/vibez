//! Native FM Synth body, in the bands every tabbed instrument card shares:
//! the algorithm with the sound's four macros and its scope, the
//! operators, their envelopes, or the LFO and the voices. Tall wide cards
//! show the concept's overview, everything at once.

use iced::widget::{button, canvas, column, container, row, text};
use iced::{Alignment, Color, Element, Length, Size, Theme};
use vibez_core::id::TrackId;
use vibez_instruments::fm::{op, param, OPERATORS};

use crate::domains::device_params::DeviceParams;
use crate::domains::fm::{FmPage, ALGORITHM_NAMES, DEST_NAMES, LFO_SHAPE_NAMES, OPERATOR_NAMES};
use crate::domains::instrument_card::{InstrumentCardMsg, InstrumentPanel};
use crate::domains::param_entry::ValueEntry;
use crate::message::Message;
use crate::theme as th;
use crate::widgets::device_controls::{segmented, Param, LABEL_SIZE, VALUE_SIZE};
use crate::widgets::device_layout::KNOB_ROW_H;
use crate::widgets::fm_display::{AlgorithmPlot, ScopePlot};
pub use crate::widgets::instrument_bands::{
    preset_bar_width, Form, COMPACT_W, FULL_W, MAX_W, MEDIUM_W,
};
use crate::widgets::instrument_bands::{
    BandCard, Group, PlotKind, Tier, BAND_PAD, HEADER_H, STRIP_H,
};
use crate::widgets::synth_display::{EnvelopePlot, LfoPlot};

/// The overview needs two rows of knobs beside the operator's envelope.
const OVERVIEW_TOP_H: f32 = HEADER_H + 4.0 + KNOB_ROW_H * 2.0;

pub fn arrange(size: Size, page: FmPage) -> Form {
    let overview = Tier::for_width(size.width) == Tier::Full && page == FmPage::Overview;
    crate::widgets::instrument_bands::arrange(size, overview.then_some(OVERVIEW_TOP_H))
}

#[derive(Clone, Copy)]
pub enum Plot {
    Algorithm,
    Operators,
    Envelope(usize),
    Scope,
    Lfo,
}

impl PlotKind for Plot {
    fn centrepiece(self) -> bool {
        false
    }

    /// The algorithm and the operator table share the concept's top row
    /// evenly with the envelope.
    fn weight(self) -> f32 {
        match self {
            Self::Algorithm | Self::Operators => 6.0,
            _ => 4.0,
        }
    }
}

const TABLE_ROW_MAX_H: f32 = 26.0;
const OPERATOR_KNOBS: [usize; 5] = [op::RATIO, op::FINE, op::LEVEL, op::FEEDBACK, op::KEY_TRACK];
const ENVELOPE_KNOBS: [usize; 4] = [op::ATTACK, op::DECAY, op::SUSTAIN, op::RELEASE];
const MACROS: [usize; 4] = [
    param::BRIGHTNESS,
    param::DECAY,
    param::MOTION,
    param::VOLUME_DB,
];

pub struct FmView<'a> {
    pub track_id: TrackId,
    pub device: DeviceParams<'a>,
    pub accent: Color,
    pub panel: InstrumentPanel,
    pub entry: Option<&'a ValueEntry>,
}

impl<'a> FmView<'a> {
    fn page(&self) -> FmPage {
        FmPage::at(self.panel.page)
    }

    fn value(&self, index: usize) -> f32 {
        self.device.value(index)
    }

    fn whole(&self, index: usize) -> usize {
        self.value(index).round().max(0.0) as usize
    }

    /// Every control's value, defaults standing in for any a track has not
    /// stored.
    fn patch(&self) -> Vec<f32> {
        (0..param::COUNT).map(|index| self.value(index)).collect()
    }

    /// The operator the card shows.
    fn shown(&self) -> usize {
        self.panel.oscillator.min(OPERATORS - 1)
    }

    fn switch(&self) -> Element<'a, Message> {
        let track_id = self.track_id;
        let options: Vec<(usize, &str)> = OPERATOR_NAMES.iter().copied().enumerate().collect();
        segmented(
            &options,
            self.shown(),
            move |operator| {
                Message::InstrumentCard(InstrumentCardMsg::ShowOscillator(track_id, operator))
            },
            self.accent,
        )
    }

    fn algorithm_group(&self) -> Group<'a, Plot> {
        Group {
            title: "Algorithm",
            note: None,
            switch: None,
            choices: vec![(param::ALGORITHM, &ALGORITHM_NAMES)],
            plot: Some(Plot::Algorithm),
            knobs: Vec::new(),
        }
    }

    fn table_group(&self) -> Group<'a, Plot> {
        Group {
            title: "Operators",
            note: None,
            switch: None,
            choices: Vec::new(),
            plot: Some(Plot::Operators),
            knobs: Vec::new(),
        }
    }

    fn operator_group(&self, operator: usize, switch: bool) -> Group<'a, Plot> {
        Group {
            title: ["Operator A", "Operator B", "Operator C", "Operator D"][operator],
            note: None,
            switch: switch.then(|| self.switch()),
            choices: Vec::new(),
            plot: None,
            knobs: OPERATOR_KNOBS
                .map(|control| param::of(operator, control))
                .to_vec(),
        }
    }

    fn envelope_group(&self, operator: usize, switch: bool) -> Group<'a, Plot> {
        Group {
            title: [
                "Op A Envelope",
                "Op B Envelope",
                "Op C Envelope",
                "Op D Envelope",
            ][operator],
            note: None,
            switch: switch.then(|| self.switch()),
            choices: Vec::new(),
            plot: Some(Plot::Envelope(operator)),
            knobs: ENVELOPE_KNOBS
                .map(|control| param::of(operator, control))
                .to_vec(),
        }
    }

    fn macro_group(&self) -> Group<'a, Plot> {
        Group {
            title: "Sound",
            note: None,
            switch: None,
            choices: Vec::new(),
            plot: Some(Plot::Scope),
            knobs: MACROS.to_vec(),
        }
    }

    fn lfo_group(&self) -> Group<'a, Plot> {
        Group {
            title: "LFO",
            note: None,
            switch: None,
            choices: vec![
                (param::LFO_SHAPE, &LFO_SHAPE_NAMES),
                (param::LFO_DEST, &DEST_NAMES),
            ],
            plot: Some(Plot::Lfo),
            knobs: vec![param::LFO_RATE, param::MOTION],
        }
    }

    fn voice_group(&self) -> Group<'a, Plot> {
        Group {
            title: "Voice",
            note: None,
            switch: None,
            choices: Vec::new(),
            plot: None,
            knobs: vec![param::VELOCITY, param::VOICES],
        }
    }

    /// The groups a tab shows on a card this wide.
    fn groups(&self, page: FmPage, tier: Tier) -> Vec<Group<'a, Plot>> {
        let shown = self.shown();
        match page {
            FmPage::Overview if tier == Tier::Full => vec![
                self.algorithm_group(),
                self.table_group(),
                self.macro_group(),
            ],
            FmPage::Overview => vec![self.algorithm_group(), self.macro_group()],
            FmPage::Operators => vec![self.table_group(), self.operator_group(shown, true)],
            FmPage::Envelopes => vec![self.envelope_group(shown, true)],
            FmPage::Modulation => vec![self.lfo_group(), self.voice_group()],
        }
    }

    pub fn view(self, preset_bar: Element<'a, Message>, size: Size) -> Element<'a, Message> {
        let form = arrange(size, self.page());
        let tier = Tier::for_width(size.width);
        let shown = self.shown();
        let bands = match form {
            Form::Overview(top) => {
                let groups = vec![
                    self.algorithm_group(),
                    self.table_group(),
                    self.envelope_group(shown, true),
                    self.operator_group(shown, false),
                ];
                let strip = vec![self.macro_group(), self.lfo_group(), self.voice_group()];
                vec![(groups, top), (strip, STRIP_H - BAND_PAD * 2.0)]
            }
            Form::Band(height) => vec![(self.groups(self.page(), tier), height)],
            Form::Line => Vec::new(),
        };
        let pages = FmPage::ALL.map(FmPage::name);
        self.frame(preset_bar, size, form, &pages, self.page().index(), bands)
    }

    /// Each operator's ratio, fine tune and level, the shown one marked;
    /// a click on a row shows that operator.
    fn operator_table(&self) -> Element<'a, Message> {
        let cell = |content: String, color: Color| {
            container(text(content).size(VALUE_SIZE).color(color))
                .width(Length::FillPortion(3))
                .center_x(Length::FillPortion(3))
        };
        let heading_cell = |content: &'static str| {
            container(text(content).size(LABEL_SIZE).color(th::text_dim()))
                .width(Length::FillPortion(3))
                .center_x(Length::FillPortion(3))
        };
        let name_w = Length::FillPortion(1);
        let mut table = column![row![
            container(text("")).width(name_w),
            heading_cell("Ratio"),
            heading_cell("Fine"),
            heading_cell("Level"),
        ]
        .height(Length::FillPortion(1))
        .align_y(Alignment::Center)];
        for (operator, name) in OPERATOR_NAMES.iter().enumerate() {
            let shown = operator == self.shown();
            let value = |control: usize| self.param(param::of(operator, control)).value_text();
            let color = if shown { th::text() } else { th::text_dim() };
            let line = row![
                container(text(*name).size(VALUE_SIZE).color(color)).width(name_w),
                cell(value(op::RATIO), color),
                cell(value(op::FINE), color),
                cell(value(op::LEVEL), color),
            ]
            .align_y(Alignment::Center);
            let accent = self.accent;
            let track_id = self.track_id;
            table = table.push(
                button(line)
                    .padding([0.0, 6.0])
                    .width(Length::Fill)
                    .height(Length::FillPortion(1))
                    .on_press(Message::InstrumentCard(InstrumentCardMsg::ShowOscillator(
                        track_id, operator,
                    )))
                    .style(move |_theme: &Theme, status| button::Style {
                        background: Some(
                            if shown {
                                th::with_alpha(accent, 0.16)
                            } else if status == button::Status::Hovered {
                                th::with_alpha(th::border(), 0.35)
                            } else {
                                Color::TRANSPARENT
                            }
                            .into(),
                        ),
                        text_color: th::text(),
                        border: iced::Border {
                            color: if shown {
                                th::with_alpha(accent, 0.6)
                            } else {
                                Color::TRANSPARENT
                            },
                            width: 1.0,
                            radius: 2.0.into(),
                        },
                        ..Default::default()
                    }),
            );
        }
        // Rows grow with the plot only so far, so a tall card keeps the
        // table compact in the middle.
        let table = container(table.spacing(1))
            .width(Length::Fill)
            .max_height(TABLE_ROW_MAX_H * (OPERATORS + 1) as f32);
        container(table)
            .padding(4)
            .width(Length::Fill)
            .height(Length::Fill)
            .center_y(Length::Fill)
            .style(|_theme: &Theme| container::Style {
                background: Some(th::bg_dark().into()),
                border: iced::Border {
                    radius: 3.0.into(),
                    ..Default::default()
                },
                ..Default::default()
            })
            .into()
    }
}

impl<'a> BandCard<'a> for FmView<'a> {
    type Plot = Plot;

    fn track_id(&self) -> TrackId {
        self.track_id
    }

    fn accent(&self) -> Color {
        self.accent
    }

    fn line_part(&self) -> usize {
        self.panel.line_part
    }

    fn param(&self, index: usize) -> Param<'a> {
        let param = Param::new(self.track_id, self.device, index, self.accent, self.entry);
        let label = match index {
            param::ALGORITHM => "Algorithm",
            param::LFO_RATE => "Rate",
            param::LFO_SHAPE => "Shape",
            param::LFO_DEST => "Destination",
            index if index < param::ALGORITHM => match index % op::COUNT {
                op::RATIO => "Ratio",
                op::FINE => "Fine Tune",
                op::LEVEL => "Level",
                op::FEEDBACK => "Feedback",
                op::KEY_TRACK => "Key Track",
                op::ATTACK => "Attack",
                op::DECAY => "Decay",
                op::SUSTAIN => "Sustain",
                _ => "Release",
            },
            _ => return param,
        };
        param.label(label)
    }

    fn plot(&self, plot: Plot) -> Element<'a, Message> {
        let accent = self.accent;
        match plot {
            Plot::Algorithm => canvas(AlgorithmPlot {
                track_id: self.track_id,
                algorithm: self.whole(param::ALGORITHM),
                selected: self.shown(),
                feedback: std::array::from_fn(|operator| {
                    self.value(param::of(operator, op::FEEDBACK)) > 0.0
                }),
                accent,
            })
            .width(Length::Fill)
            .height(Length::Fill)
            .into(),
            Plot::Operators => self.operator_table(),
            Plot::Envelope(operator) => {
                let control = |control: usize| self.value(param::of(operator, control));
                canvas(EnvelopePlot {
                    attack: control(op::ATTACK),
                    decay: control(op::DECAY),
                    sustain: control(op::SUSTAIN),
                    release: control(op::RELEASE),
                    accent,
                })
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
            }
            Plot::Scope => canvas(ScopePlot {
                params: self.patch(),
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
        }
    }

    fn every_group(&self) -> Vec<Group<'a, Plot>> {
        // The algorithm leads, with the macros that move the whole sound.
        let mut groups = vec![Group {
            choices: vec![(param::ALGORITHM, &ALGORITHM_NAMES)],
            ..self.macro_group()
        }];
        for operator in 0..OPERATORS {
            groups.push(self.operator_group(operator, false));
            groups.push(self.envelope_group(operator, false));
        }
        // Motion is the LFO's depth, kept with the other macros.
        groups.push(Group {
            knobs: vec![param::LFO_RATE],
            ..self.lfo_group()
        });
        groups.push(self.voice_group());
        groups
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::instrument_bands::{
        used_height, DEFAULT_BODY_H, LINE_GAP, MINIMUM_BODY_H, PAD, PAGE_MENU_W,
    };

    fn view(track: &crate::state::ProjectTrack) -> FmView<'_> {
        FmView {
            track_id: track.id,
            device: DeviceParams::instrument(track).unwrap(),
            accent: Color::WHITE,
            panel: InstrumentPanel::default(),
            entry: None,
        }
    }

    fn fm_track() -> crate::state::ProjectTrack {
        let mut track = crate::state::ProjectTrack::new(TrackId::new(), "Keys".into(), 0);
        track.has_instrument = true;
        track.instrument_kind = Some(vibez_core::midi::InstrumentKind::FmSynth);
        track
    }

    #[test]
    fn the_default_panel_shows_a_band_with_room_for_knobs() {
        for width in [COMPACT_W, MEDIUM_W, FULL_W, MAX_W] {
            for page in FmPage::ALL {
                let Form::Band(height) = arrange(Size::new(width, DEFAULT_BODY_H), page) else {
                    panic!("{width} {page:?}");
                };
                assert!(
                    height - HEADER_H >= KNOB_ROW_H,
                    "{width} {page:?}: {height}"
                );
            }
        }
    }

    #[test]
    fn the_minimum_panel_sets_a_part_on_a_line() {
        for width in [COMPACT_W, MEDIUM_W, FULL_W] {
            assert_eq!(
                arrange(Size::new(width, MINIMUM_BODY_H), FmPage::Overview),
                Form::Line
            );
        }
    }

    #[test]
    fn tall_wide_cards_show_the_overview_on_its_tab() {
        let tall = Size::new(FULL_W, 420.0);
        assert!(matches!(
            arrange(tall, FmPage::Overview),
            Form::Overview(top) if top >= OVERVIEW_TOP_H
        ));
        assert!(matches!(arrange(tall, FmPage::Envelopes), Form::Band(_)));
        assert!(matches!(
            arrange(Size::new(MEDIUM_W, 420.0), FmPage::Overview),
            Form::Band(_)
        ));
    }

    #[test]
    fn the_shortest_card_reaches_every_control_once() {
        let track = fm_track();
        let view = view(&track);
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
    fn the_scope_plays_a_track_that_stored_no_values() {
        let track = fm_track();
        assert!(track.instrument_params.is_empty());
        let patch = view(&track).patch();
        assert_eq!(patch.len(), param::COUNT);
        assert!(!vibez_instruments::fm::preview(&patch, 100).is_empty());
    }

    #[test]
    fn every_supported_size_fits_without_scrolling() {
        for width in [COMPACT_W, MEDIUM_W, FULL_W, MAX_W] {
            for height in (MINIMUM_BODY_H as usize..=480).step_by(2).map(|h| h as f32) {
                for page in FmPage::ALL {
                    let used = used_height(arrange(Size::new(width, height), page));
                    assert!(used <= height + 0.01, "{width}x{height} {page:?}");
                }
            }
        }
    }
}
