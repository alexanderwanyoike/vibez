//! Native EQ body: the response over the EQ's own spectrum, the selected
//! band beside it, and every band below when the card is tall and wide
//! enough. Narrow cards pick a band with tabs instead.

use iced::widget::{canvas, column, container, mouse_area, row, text, Space};
use iced::{Alignment, Color, Element, Length, Size};
use vibez_core::id::TrackId;
use vibez_dsp::eq::param;

use crate::domains::eq::{auditioned_band, has_q, EqMsg, EqPanelState, SpectrumSide, BAND_NAMES};
use crate::domains::param_entry::ValueEntry;
use crate::icons;
use crate::message::Message;
use crate::spectrum::SpectrumState;
use crate::state::UiEffect;
use crate::theme as th;
use crate::widgets::device_controls::{
    disclosure, heading, rule, segmented, toggle, vertical_rule, Param, LABEL_SIZE,
};
use crate::widgets::eq_display::EqDisplayWidget;
use crate::widgets::level_meter::{self, LevelMeter};

const PAD: f32 = 10.0;
const PRESET_ROW_H: f32 = 28.0;
const SIDE_W: f32 = 200.0;
/// Band name and three parameter lines.
const SIDE_H: f32 = 112.0;
const GRAPH_MIN_W: f32 = 300.0;
const GRAPH_SHORT_MIN_W: f32 = 180.0;
const GRAPH_MIN_H: f32 = 56.0;
/// Band tabs over the selected band's three controls.
const TABS_H: f32 = 56.0;
const TABS_W: f32 = 320.0;
const STRIP_H: f32 = 86.0;
const ADVANCED_H: f32 = 56.0;
const BAND_GROUP_W: f32 = 172.0;
const OUTPUT_W: f32 = 340.0;
const STRIP_KNOB: f32 = 28.0;
const ROW_KNOB: f32 = 24.0;
/// A single line of controls for the shortest cards.
const LINE_H: f32 = 24.0;
const LINE_KNOB: f32 = 18.0;
/// A one-line Advanced has room for the output trim after both cuts.
const LINE_OUTPUT_W: f32 = 480.0;

pub const COMPACT_W: f32 = TABS_W + PAD * 2.0;
/// The graph with the selected band beside it.
pub const MEDIUM_W: f32 = GRAPH_MIN_W + 1.0 + SIDE_W + PAD * 2.0;
/// A short card keeps its graph beside the band tabs.
const TABS_BESIDE_W: f32 = GRAPH_SHORT_MIN_W + 9.0 + TABS_W + PAD * 2.0;
/// Advanced has room for the analyser controls after the cuts and output.
const ADVANCED_SPECTRUM_W: f32 = 640.0;
/// Room for all four bands in a row.
pub const STRIP_W: f32 = BAND_GROUP_W * 4.0 + 3.0 + PAD * 2.0;
/// The band row plus output and analyser controls at its end.
pub const FULL_W: f32 = STRIP_W + 1.0 + OUTPUT_W;
pub const MAX_W: f32 = 1100.0;

/// Card width for a share of the device section.
pub fn card_width(available: f32) -> f32 {
    if available >= MEDIUM_W {
        available.min(MAX_W)
    } else {
        COMPACT_W
    }
}

/// What fills the space between the preset row and anything below it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Upper {
    /// The graph with the selected band's panel beside it.
    Side,
    /// A short card: the graph beside the band tabs.
    TabsBeside,
    /// The graph over the band tabs.
    TabsBelow,
    /// A short, narrow card keeps every band control and drops the graph.
    TabsOnly,
    /// The shortest cards: band tabs and the selected band's values on one line.
    Line,
}

/// How the body divides its measured size. It never scrolls: what does not
/// fit moves into Advanced, and Advanced replaces the body when it cannot
/// sit under it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Arrangement {
    pub kind: Upper,
    pub upper: f32,
    pub strip: bool,
    /// Output trim and analyser controls sit at the end of the band row
    /// rather than in Advanced.
    pub output_in_strip: bool,
    pub advanced: Option<f32>,
    pub advanced_only: bool,
    /// Advanced is wide enough to carry the analyser controls too.
    pub advanced_spectrum: bool,
}

fn upper_for(width: f32, height: f32) -> Upper {
    if width >= MEDIUM_W && height >= SIDE_H {
        Upper::Side
    } else if width >= TABS_BESIDE_W && height >= TABS_H {
        Upper::TabsBeside
    } else if height >= GRAPH_MIN_H + TABS_H {
        Upper::TabsBelow
    } else if height >= TABS_H {
        Upper::TabsOnly
    } else {
        Upper::Line
    }
}

pub fn arrange(size: Size, advanced: bool) -> Arrangement {
    let below = (size.height - PRESET_ROW_H - 1.0).max(0.0);
    let stacked = if advanced { 1.0 + ADVANCED_H } else { 0.0 };
    let kind = upper_for(size.width, below - stacked);
    let advanced_only = advanced && (kind != Upper::Side || below - stacked < SIDE_H);
    let strip = !advanced_only
        && kind == Upper::Side
        && size.width >= STRIP_W
        && below - stacked >= SIDE_H + 1.0 + STRIP_H;
    let upper = if advanced_only {
        0.0
    } else {
        below - stacked - if strip { 1.0 + STRIP_H } else { 0.0 }
    };
    Arrangement {
        kind,
        upper,
        strip,
        output_in_strip: strip && size.width >= FULL_W,
        advanced: match (advanced, advanced_only) {
            (false, _) => None,
            (true, true) => Some(below),
            (true, false) => Some(ADVANCED_H),
        },
        advanced_only,
        advanced_spectrum: size.width >= ADVANCED_SPECTRUM_W,
    }
}

/// Width left for the preset bar beside the Advanced toggle.
pub fn preset_bar_width(size: Size) -> f32 {
    size.width - PAD * 2.0 - 90.0
}

pub struct EqView<'a> {
    pub track_id: TrackId,
    pub effect: &'a UiEffect,
    pub accent: Color,
    pub panel: &'a EqPanelState,
    pub entry: Option<&'a ValueEntry>,
    pub spectrum: Option<&'a SpectrumState>,
    pub sample_rate: f32,
}

impl<'a> EqView<'a> {
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
        self.effect.params.get(index).copied().unwrap_or_else(|| {
            self.effect
                .descriptors
                .get(index)
                .map_or(0.0, |descriptor| descriptor.default)
        })
    }

    fn set(&self, index: usize, value: f32) -> Message {
        Message::set_effect_param(self.track_id, self.effect.id, index, value)
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
            body = body.push(self.advanced(
                layout.advanced.unwrap_or(0.0),
                layout.advanced_spectrum,
                size.width,
            ));
            return body.width(Length::Fill).height(Length::Fill).into();
        }
        body = body.push(self.upper(layout));
        if layout.strip {
            body = body.push(rule()).push(self.strip(layout.output_in_strip));
        }
        if let Some(height) = layout.advanced {
            body =
                body.push(rule())
                    .push(self.advanced(height, layout.advanced_spectrum, size.width));
        }
        body.width(Length::Fill).height(Length::Fill).into()
    }

    fn graph(&self) -> Element<'a, Message> {
        let spectrum = self.spectrum.filter(|spectrum| spectrum.active);
        canvas(EqDisplayWidget {
            track_id: self.track_id,
            effect_id: self.effect.id,
            params: self.effect.params.clone(),
            descriptors: self.effect.descriptors,
            bypass: self.effect.bypass,
            sample_rate: self.sample_rate,
            spectrum: spectrum
                .map(|spectrum| spectrum.bins.clone())
                .unwrap_or_default(),
            spectrum_active: spectrum.is_some(),
            accent: self.accent(),
            selected: self.panel.band,
        })
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }

    fn upper(&self, layout: Arrangement) -> Element<'a, Message> {
        let graph = || container(self.graph()).padding([6.0, 0.0]);
        let content: Element<'a, Message> = match layout.kind {
            Upper::Side => row![
                graph().width(Length::Fill),
                vertical_rule(),
                container(self.band_panel())
                    .width(SIDE_W)
                    .padding([6.0, 8.0]),
            ]
            .spacing(8)
            .into(),
            Upper::TabsBeside => row![
                graph().width(Length::Fill),
                vertical_rule(),
                container(self.band_tabs())
                    .width(TABS_W)
                    .center_y(Length::Fill),
            ]
            .spacing(8)
            .into(),
            Upper::TabsBelow => column![graph().height(Length::Fill), self.band_tabs()]
                .spacing(4)
                .into(),
            Upper::TabsOnly => container(self.band_tabs()).center_y(Length::Fill).into(),
            Upper::Line => container(self.band_line()).center_y(Length::Fill).into(),
        };
        container(content)
            .padding([0.0, PAD])
            .height(layout.upper)
            .width(Length::Fill)
            .into()
    }

    fn band_header(&self, band: usize, with_name: bool) -> Element<'a, Message> {
        let accent = self.accent();
        let enabled = self.value(param::ENABLE[band]) >= 0.5;
        let auditioned = auditioned_band(&self.effect.params) == Some(band);
        let mut header = row![].spacing(4).align_y(Alignment::Center);
        if with_name {
            header = header.push(text(BAND_NAMES[band]).size(11).color(if enabled {
                accent
            } else {
                th::text_muted()
            }));
        }
        header
            .push(Space::with_width(Length::Fill))
            .push(toggle(
                icons::icon(icons::HEADPHONES).size(10).into(),
                auditioned,
                self.set(
                    param::AUDITION,
                    if auditioned { 0.0 } else { band as f32 + 1.0 },
                ),
                accent,
            ))
            .push(toggle(
                icons::icon(icons::POWER).size(10).into(),
                enabled,
                self.set(param::ENABLE[band], if enabled { 0.0 } else { 1.0 }),
                accent,
            ))
            .into()
    }

    /// Shelf or bell for the outer bands, which have no Q.
    fn shape(&self, band: usize) -> Element<'a, Message> {
        let index = param::SHAPE[band];
        let bell = self.value(index) >= 0.5;
        let track_id = self.track_id;
        let effect_id = self.effect.id;
        segmented(
            &[(false, "Shelf"), (true, "Bell")],
            bell,
            move |bell| {
                Message::set_effect_param(track_id, effect_id, index, if bell { 1.0 } else { 0.0 })
            },
            self.accent(),
        )
    }

    /// The band row has room for one switch, lit while the shelf is a bell.
    fn bell_toggle(&self, band: usize) -> Element<'a, Message> {
        let index = param::SHAPE[band];
        let bell = self.value(index) >= 0.5;
        toggle(
            text("Bell").size(LABEL_SIZE).into(),
            bell,
            self.set(index, if bell { 0.0 } else { 1.0 }),
            self.accent(),
        )
    }

    fn band_panel(&self) -> Element<'a, Message> {
        let band = self.panel.band;
        let mut lines = column![
            self.band_header(band, true),
            self.param(param::FREQUENCY[band])
                .label("Frequency")
                .row(ROW_KNOB),
            self.param(param::GAIN[band]).label("Gain").row(ROW_KNOB),
        ]
        .spacing(4);
        lines = if has_q(band) {
            lines.push(self.param(param::SHAPE[band]).label("Q").row(ROW_KNOB))
        } else {
            lines.push(
                row![
                    text("Shape")
                        .size(LABEL_SIZE)
                        .color(th::text_dim())
                        .width(Length::Fill),
                    self.shape(band)
                ]
                .align_y(Alignment::Center),
            )
        };
        lines.into()
    }

    fn band_tabs(&self) -> Element<'a, Message> {
        let band = self.panel.band;
        let effect_id = self.effect.id;
        let tabs = segmented(
            &[(0, "LF"), (1, "LMF"), (2, "HMF"), (3, "HF")],
            band,
            move |band| Message::Eq(EqMsg::SelectBand(effect_id, band)),
            self.accent(),
        );
        let third: Element<'a, Message> = if has_q(band) {
            self.param(param::SHAPE[band]).label("Q").row(ROW_KNOB)
        } else {
            self.shape(band)
        };
        column![
            row![tabs, self.band_header(band, false)]
                .spacing(6)
                .align_y(Alignment::Center),
            row![
                self.param(param::FREQUENCY[band])
                    .label("Freq")
                    .row(ROW_KNOB),
                self.param(param::GAIN[band]).label("Gain").row(ROW_KNOB),
                third,
            ]
            .spacing(12)
            .align_y(Alignment::Center),
        ]
        .spacing(4)
        .height(TABS_H)
        .into()
    }

    /// A value and its knob with no name, for single-line layouts where the
    /// band tab and the unit say what the value is.
    fn value_knob(&self, index: usize, label: &'a str) -> Element<'a, Message> {
        let param = self.param(index).label(label);
        row![
            param.value_entry(),
            canvas(param.knob()).width(LINE_KNOB).height(LINE_KNOB),
        ]
        .spacing(2)
        .align_y(Alignment::Center)
        .into()
    }

    fn band_line(&self) -> Element<'a, Message> {
        let band = self.panel.band;
        let effect_id = self.effect.id;
        let third = if has_q(band) {
            self.value_knob(param::SHAPE[band], "Q")
        } else {
            self.bell_toggle(band)
        };
        row![
            segmented(
                &[(0, "LF"), (1, "LMF"), (2, "HMF"), (3, "HF")],
                band,
                move |band| Message::Eq(EqMsg::SelectBand(effect_id, band)),
                self.accent(),
            ),
            self.value_knob(param::FREQUENCY[band], "Freq"),
            self.value_knob(param::GAIN[band], "Gain"),
            third,
        ]
        .spacing(6)
        .height(LINE_H)
        .align_y(Alignment::Center)
        .into()
    }

    fn strip(&self, with_output: bool) -> Element<'a, Message> {
        let mut bands = row![].spacing(0).height(STRIP_H);
        for band in 0..BAND_NAMES.len() {
            if band > 0 {
                bands = bands.push(vertical_rule());
            }
            let third: Element<'a, Message> = if has_q(band) {
                self.param(param::SHAPE[band]).label("Q").column(STRIP_KNOB)
            } else {
                column![heading("Shape"), self.bell_toggle(band)]
                    .spacing(4)
                    .align_x(Alignment::Center)
                    .into()
            };
            let selected = band == self.panel.band;
            let effect_id = self.effect.id;
            let group = container(
                column![
                    self.band_header(band, true),
                    row![
                        self.param(param::FREQUENCY[band])
                            .label("Freq")
                            .column(STRIP_KNOB),
                        self.param(param::GAIN[band])
                            .label("Gain")
                            .column(STRIP_KNOB),
                        third,
                    ]
                    .spacing(8)
                    .align_y(Alignment::Center),
                ]
                .spacing(2),
            )
            .padding([4.0, 6.0])
            .width(BAND_GROUP_W)
            .style(move |_| container::Style {
                background: selected.then(|| th::bg_hover().into()),
                ..Default::default()
            });
            bands = bands
                .push(mouse_area(group).on_press(Message::Eq(EqMsg::SelectBand(effect_id, band))));
        }
        let mut strip = row![bands].height(STRIP_H);
        if with_output {
            strip = strip.push(vertical_rule()).push(
                container(self.output_controls())
                    .width(OUTPUT_W)
                    .padding([4.0, 8.0]),
            );
        }
        container(strip)
            .padding([0.0, PAD])
            .width(Length::Fill)
            .into()
    }

    fn output_controls(&self) -> Element<'a, Message> {
        let meter = |label: &'static str, levels| {
            column![
                heading(label),
                canvas(LevelMeter { levels })
                    .width(level_meter::WIDTH)
                    .height(Length::Fill),
            ]
            .spacing(2)
            .height(STRIP_H - 8.0)
        };
        row![
            meter("Input", self.panel.input),
            meter("Output", self.panel.output),
            self.param(param::OUTPUT).label("Trim").column(STRIP_KNOB),
            self.spectrum_controls(),
        ]
        .spacing(10)
        .align_y(Alignment::Center)
        .into()
    }

    fn spectrum_controls(&self) -> Element<'a, Message> {
        let effect_id = self.effect.id;
        column![
            heading("Spectrum"),
            row![
                segmented(
                    &[(SpectrumSide::Pre, "Pre"), (SpectrumSide::Post, "Post")],
                    self.panel.side,
                    move |side| Message::Eq(EqMsg::SetSpectrumSide(effect_id, side)),
                    self.accent(),
                ),
                toggle(
                    row![
                        icons::icon(icons::SNOWFLAKE).size(10),
                        text("Freeze")
                            .size(LABEL_SIZE)
                            .wrapping(text::Wrapping::None)
                    ]
                    .spacing(4)
                    .align_y(Alignment::Center)
                    .into(),
                    self.panel.frozen,
                    Message::Eq(EqMsg::ToggleFreeze(effect_id)),
                    self.accent(),
                ),
            ]
            .spacing(6)
            .align_y(Alignment::Center),
        ]
        .spacing(4)
        .into()
    }

    fn cut(&self, enabled: usize, frequency: usize, label: &'static str) -> Element<'a, Message> {
        let on = self.value(enabled) >= 0.5;
        column![
            row![
                heading(label),
                toggle(
                    icons::icon(icons::POWER).size(10).into(),
                    on,
                    self.set(enabled, if on { 0.0 } else { 1.0 }),
                    self.accent(),
                )
            ]
            .spacing(6)
            .align_y(Alignment::Center),
            row![
                self.param(frequency).value_entry(),
                canvas(self.param(frequency).knob())
                    .width(ROW_KNOB)
                    .height(ROW_KNOB),
            ]
            .spacing(6)
            .align_y(Alignment::Center),
        ]
        .spacing(4)
        .into()
    }

    fn advanced(&self, height: f32, with_spectrum: bool, width: f32) -> Element<'a, Message> {
        if height < ADVANCED_H {
            return container(self.advanced_line(width))
                .padding([0.0, PAD])
                .height(height)
                .center_y(height)
                .into();
        }
        let mut content = row![
            self.cut(param::HP_ENABLED, param::HP_FREQUENCY, "Low cut"),
            self.cut(param::LP_ENABLED, param::LP_FREQUENCY, "High cut"),
            vertical_rule(),
            container(self.param(param::OUTPUT).label("Output").row(ROW_KNOB)).width(150.0),
        ]
        .spacing(12)
        .align_y(Alignment::Center);
        if with_spectrum {
            content = content.push(vertical_rule()).push(self.spectrum_controls());
        }
        container(content)
            .padding([4.0, PAD])
            .height(height)
            .center_y(height)
            .into()
    }

    fn advanced_line(&self, width: f32) -> Element<'a, Message> {
        let cut = |enabled: usize, frequency: usize, label: &'static str| {
            let on = self.value(enabled) >= 0.5;
            row![
                toggle(
                    text(label).size(LABEL_SIZE).into(),
                    on,
                    self.set(enabled, if on { 0.0 } else { 1.0 }),
                    self.accent(),
                ),
                self.value_knob(frequency, label),
            ]
            .spacing(4)
            .align_y(Alignment::Center)
        };
        let mut line = row![
            cut(param::HP_ENABLED, param::HP_FREQUENCY, "Low cut"),
            cut(param::LP_ENABLED, param::LP_FREQUENCY, "High cut"),
        ]
        .spacing(10)
        .height(LINE_H)
        .align_y(Alignment::Center);
        if width >= LINE_OUTPUT_W {
            line = line.push(
                row![heading("Output"), self.value_knob(param::OUTPUT, "Output")]
                    .spacing(4)
                    .align_y(Alignment::Center),
            );
        }
        line.into()
    }

    fn disclosure(&self) -> Element<'a, Message> {
        disclosure(
            self.panel.advanced,
            Message::Eq(EqMsg::ToggleAdvanced(self.effect.id)),
            self.accent(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn used(layout: Arrangement) -> f32 {
        let mut used = PRESET_ROW_H + 1.0;
        if layout.advanced_only {
            return used + layout.advanced.unwrap();
        }
        used += layout.upper;
        if layout.strip {
            used += 1.0 + STRIP_H;
        }
        if let Some(height) = layout.advanced {
            used += 1.0 + height;
        }
        used
    }

    /// At its minimum the device section leaves this much for a card body;
    /// at 1.5x interface scale the real app measured about 73.
    const MIN_BODY_H: u32 = 56;

    #[test]
    fn every_supported_size_fits_without_scrolling() {
        for width in [COMPACT_W, MEDIUM_W, TABS_BESIDE_W, STRIP_W, FULL_W, MAX_W] {
            for height in (MIN_BODY_H..=420).step_by(2).map(|height| height as f32) {
                for advanced in [false, true] {
                    let layout = arrange(Size::new(width, height), advanced);
                    let context = format!("{width}x{height} {advanced}: {layout:?}");
                    assert!(used(layout) <= height + 0.01, "{context}");
                    if layout.advanced_only {
                        assert!(layout.advanced.unwrap() >= LINE_H - 0.01, "{context}");
                        continue;
                    }
                    let minimum = match layout.kind {
                        Upper::Side => SIDE_H,
                        Upper::TabsBelow => GRAPH_MIN_H + TABS_H,
                        Upper::TabsBeside | Upper::TabsOnly => TABS_H,
                        Upper::Line => LINE_H,
                    };
                    assert!(layout.upper >= minimum - 0.01, "{context}");
                }
            }
        }
    }

    #[test]
    fn wide_tall_cards_show_every_band_and_the_output_controls() {
        let layout = arrange(Size::new(FULL_W, 360.0), false);
        assert!(layout.kind == Upper::Side && layout.strip && layout.output_in_strip);
        let narrower = arrange(Size::new(STRIP_W, 360.0), false);
        assert!(narrower.strip && !narrower.output_in_strip);
    }

    #[test]
    fn short_cards_keep_the_graph_beside_the_tabs_when_wide_enough() {
        assert_eq!(
            arrange(Size::new(FULL_W, 100.0), false).kind,
            Upper::TabsBeside
        );
        assert_eq!(
            arrange(Size::new(COMPACT_W, 100.0), false).kind,
            Upper::TabsOnly
        );
        assert_eq!(
            arrange(Size::new(COMPACT_W, 360.0), false).kind,
            Upper::TabsBelow
        );
        assert_eq!(arrange(Size::new(MEDIUM_W, 200.0), false).kind, Upper::Side);
        assert_eq!(arrange(Size::new(FULL_W, 70.0), false).kind, Upper::Line);
    }

    #[test]
    fn advanced_stacks_under_the_side_panel_and_otherwise_takes_the_body() {
        let tall = arrange(Size::new(FULL_W, 400.0), true);
        assert!(!tall.advanced_only && tall.advanced == Some(ADVANCED_H));
        assert!(arrange(Size::new(FULL_W, 150.0), true).advanced_only);
        assert!(arrange(Size::new(COMPACT_W, 400.0), true).advanced_only);
    }

    #[test]
    fn card_width_is_compact_until_the_side_panel_fits() {
        assert_eq!(card_width(200.0), COMPACT_W);
        assert_eq!(card_width(MEDIUM_W), MEDIUM_W);
        assert_eq!(card_width(5000.0), MAX_W);
    }
}
