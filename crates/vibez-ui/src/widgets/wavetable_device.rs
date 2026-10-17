//! Native Wavetable Synth body, in the bands every tabbed instrument card
//! shares: the oscillators with their tables, the filter, the envelopes,
//! or the LFO and the second envelope's routing. Tall wide cards show the
//! concept's overview on the Oscillators tab, everything at once.

use iced::widget::canvas;
use iced::{Color, Element, Length, Size};
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
use crate::widgets::device_controls::{segmented, Param};
use crate::widgets::device_layout::KNOB_ROW_H;
pub use crate::widgets::instrument_bands::{
    card_width, preset_bar_width, Form, COMPACT_W, FULL_W, MAX_W, MEDIUM_W,
};
use crate::widgets::instrument_bands::{
    BandCard, Group, PlotKind, Tier, BAND_PAD, HEADER_H, STRIP_H,
};
use crate::widgets::synth_display::{EnvelopePlot, FilterPlot, LfoPlot};
use crate::widgets::wavetable_display::WavetablePlot;

/// The overview needs two rows of knobs beside each oscillator's plot.
const OVERVIEW_TOP_H: f32 = HEADER_H + 4.0 + KNOB_ROW_H * 2.0;

pub fn arrange(size: Size, page: WavetablePage) -> Form {
    let overview = Tier::for_width(size.width) == Tier::Full && page == WavetablePage::Oscillators;
    crate::widgets::instrument_bands::arrange(size, overview.then_some(OVERVIEW_TOP_H))
}

#[derive(Clone, Copy)]
pub enum Plot {
    Table(usize),
    Filter,
    Envelope(usize),
    Lfo,
}

impl PlotKind for Plot {
    fn centrepiece(self) -> bool {
        matches!(self, Self::Table(_))
    }
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

pub struct WavetableView<'a> {
    pub track_id: TrackId,
    pub device: DeviceParams<'a>,
    pub accent: Color,
    pub panel: InstrumentPanel,
    pub entry: Option<&'a ValueEntry>,
}

impl<'a> WavetableView<'a> {
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
        self.panel.oscillator.min(1)
    }

    fn osc_group(&self, osc: usize, switch: bool) -> Group<'a, Plot> {
        let note = format!(
            "Frame {:.0}%",
            self.value(param::of(osc, osc::POSITION)) * 100.0
        );
        let switch = switch.then(|| {
            let track_id = self.track_id;
            segmented(
                &[(0, "Osc A"), (1, "Osc B")],
                osc,
                move |osc| {
                    Message::InstrumentCard(InstrumentCardMsg::ShowOscillator(track_id, osc))
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
    fn osc_knobs_group(&self, osc: usize) -> Group<'a, Plot> {
        Group {
            plot: None,
            note: None,
            ..self.osc_group(osc, false)
        }
    }

    /// The overview's one wavetable, for the oscillator a switch picks.
    fn table_group(&self) -> Group<'a, Plot> {
        let osc = self.shown_osc();
        Group {
            title: "Wavetable",
            choices: Vec::new(),
            knobs: Vec::new(),
            ..self.osc_group(osc, true)
        }
    }

    fn filter_group(&self) -> Group<'a, Plot> {
        Group {
            title: "Filter",
            note: None,
            switch: None,
            choices: vec![(param::FILTER_TYPE, &FILTER_NAMES)],
            plot: Some(Plot::Filter),
            knobs: vec![param::CUTOFF, param::RESONANCE, param::DRIVE],
        }
    }

    fn envelope_group(&self, second: bool, switch: bool) -> Group<'a, Plot> {
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

    fn lfo_group(&self) -> Group<'a, Plot> {
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

    fn env2_routing(&self) -> Group<'a, Plot> {
        Group {
            title: "Env 2",
            note: None,
            switch: None,
            choices: vec![(param::ENV2_DEST, &DEST_NAMES)],
            plot: None,
            knobs: vec![param::ENV2_AMOUNT],
        }
    }

    fn voice_group(&self) -> Group<'a, Plot> {
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
    fn groups(&self, page: WavetablePage, tier: Tier) -> Vec<Group<'a, Plot>> {
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
        let bands = match form {
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
                vec![(groups, top), (strip, STRIP_H - BAND_PAD * 2.0)]
            }
            Form::Band(height) => vec![(self.groups(self.page(), tier), height)],
            Form::Line => Vec::new(),
        };
        let pages = WavetablePage::ALL.map(WavetablePage::name);
        self.frame(preset_bar, size, form, &pages, self.page().index(), bands)
    }
}

impl<'a> BandCard<'a> for WavetableView<'a> {
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
        program
    }

    /// Every group once, named apart, in the order the tabs show them, for
    /// the shortest card to step through.
    fn every_group(&self) -> Vec<Group<'a, Plot>> {
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::instrument_bands::{
        used_height, DEFAULT_BODY_H, LINE_GAP, MINIMUM_BODY_H, PAD, PAGE_MENU_W,
    };

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
                    let used = used_height(arrange(Size::new(width, height), page));
                    assert!(used <= height + 0.01, "{width}x{height} {page:?}");
                }
            }
        }
    }
}
