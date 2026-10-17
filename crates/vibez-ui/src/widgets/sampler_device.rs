//! Native Sampler body: the sample with its trim and loop markers above its
//! sections behind tabs. Wide and medium cards keep the mode, reverse and
//! crossfade beside the waveform; compact ones put them on a line beneath.
//! Short cards drop the section plot, then the sections, then the waveform.

use std::sync::Arc;

use iced::widget::{button, column, container, row, text, Space};
use iced::{Alignment, Color, Element, Length, Size, Theme};
use vibez_core::audio_buffer::DecodedAudio;
use vibez_core::id::TrackId;
use vibez_instruments::sampler::{filter_type, param};

use crate::domains::device_params::DeviceParams;
use crate::domains::device_params::DeviceSlot;
use crate::domains::instrument_card::{InstrumentCardMsg, InstrumentPanel};
use crate::domains::param_entry::ValueEntry;
use crate::domains::sampler::{
    header_controls, pages, pitch_controls, Mode, SamplerPage, FILTER_NAMES,
};
use crate::domains::synth::{LFO_SHAPE_NAMES, LFO_TARGET_NAMES};
use crate::message::Message;
use crate::theme as th;
use crate::widgets::device_controls::{
    heading, plot_band, rule, segmented, tabs, vertical_rule, Param, LABEL_SIZE,
};
use crate::widgets::device_layout::{KNOB_ROW_H, LINE_H};
use crate::widgets::sampler_display::{Markers, SampleView};
use crate::widgets::synth_display::{EnvelopePlot, FilterPlot, LfoPlot};

const PAD: f32 = 10.0;
const PRESET_ROW_H: f32 = 28.0;
const TABS_H: f32 = 24.0;
/// The sample's name, length and pitch values above its waveform.
const SAMPLE_HEADER_H: f32 = 24.0;
/// The compact card's mode, reverse and crossfade line.
const MODE_LINE_H: f32 = 28.0;
const WAVE_MIN_H: f32 = 44.0;
/// What `plot_band` keeps clear above and below the sample.
const BAND_PAD: f32 = 6.0;
const WAVE_ALONE_MIN_H: f32 = 16.0;
/// A section's name and its choice, above its plot.
const HEADER_H: f32 = 26.0;
const KNOB: f32 = 30.0;
const PLOT_MIN_H: f32 = 40.0;
const SIDE_W: f32 = 176.0;
/// A section's plot and knobs share their row in this proportion.
const PLOT_SHARE: u16 = 4;
const KNOBS_SHARE: u16 = 7;
/// The mode's name and choice, beside the waveform.
const SIDE_MODE_H: f32 = 40.0;
const SIDE_GAP: f32 = 6.0;
const CHOICE_W: f32 = 92.0;

pub const COMPACT_W: f32 = 440.0;
/// The mode, reverse and crossfade fit beside the waveform.
pub const MEDIUM_W: f32 = 680.0;
pub const MAX_W: f32 = 1_300.0;

pub fn card_width(available: f32) -> f32 {
    available.clamp(COMPACT_W, MAX_W)
}

pub fn preset_bar_width(size: Size) -> f32 {
    size.width - PAD * 2.0
}

/// How a body's height divides: the sample band, the tabs and the section
/// beneath (its header, plot and knobs).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Layout {
    pub compact: bool,
    /// The section's plot sits beside its knobs, as in the concept,
    /// rather than above them.
    pub beside: bool,
    pub sample: f32,
    /// A compact card's playback line sits under the sample; the shortest
    /// leave it out rather than cut it.
    pub mode_line: bool,
    /// Zero when there is no room for the waveform.
    pub wave: f32,
    /// Zero when the sections do not fit.
    pub section: f32,
    pub plot: f32,
    pub knobs: f32,
}

pub fn arrange(size: Size) -> Layout {
    let compact = size.width < MEDIUM_W;
    let below = (size.height - PRESET_ROW_H - 1.0).max(0.0);
    let header = BAND_PAD * 2.0 + SAMPLE_HEADER_H + if compact { MODE_LINE_H } else { 0.0 };
    let sample_min = header + WAVE_MIN_H;
    let tabs = TABS_H + 1.0;
    let knobs = if below >= sample_min + 1.0 + tabs + HEADER_H + KNOB_ROW_H {
        KNOB_ROW_H
    } else {
        LINE_H
    };
    let rest = below - 1.0 - tabs - HEADER_H - knobs;
    if rest < sample_min {
        // No room for a section, so the sample takes the body.
        let sample = below;
        let mode_line = compact && sample >= header;
        let header = if compact && !mode_line {
            header - MODE_LINE_H
        } else {
            header
        };
        return Layout {
            compact,
            beside: false,
            sample,
            mode_line,
            // Alone, the waveform may be thinner than beside a section:
            // still the sample, still its markers to drag.
            wave: if sample - header >= WAVE_ALONE_MIN_H {
                sample - header
            } else {
                0.0
            },
            section: 0.0,
            plot: 0.0,
            knobs: 0.0,
        };
    }
    if !compact {
        // The plot shares the knobs' row, growing with any spare height
        // while the waveform keeps the larger share.
        let row = knobs.max(((rest + knobs) * 0.35).min(rest + knobs - sample_min));
        let sample = below - 1.0 - tabs - HEADER_H - row;
        return Layout {
            compact,
            beside: true,
            sample,
            mode_line: false,
            wave: sample - header,
            section: HEADER_H + row,
            plot: if row >= PLOT_MIN_H { row } else { 0.0 },
            knobs,
        };
    }
    let plot = if rest >= sample_min + PLOT_MIN_H {
        // The waveform keeps the larger share, as in the concept.
        (rest * 0.3).max(PLOT_MIN_H).min(rest - sample_min)
    } else {
        0.0
    };
    let sample = rest - plot;
    Layout {
        compact,
        beside: false,
        sample,
        mode_line: true,
        wave: sample - header,
        section: HEADER_H + plot + knobs,
        plot,
        knobs,
    }
}

/// A control beside a wide card's sample.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Side {
    Mode,
    Reverse,
    Crossfade,
}

/// The controls beside the sample that fit `room`, each joining while the
/// band has room for it.
fn side_rows(mut room: f32) -> Vec<Side> {
    let mut rows = Vec::new();
    for (row, height) in [
        (Side::Mode, SIDE_MODE_H),
        (Side::Reverse, LINE_H),
        (Side::Crossfade, LINE_H),
    ] {
        if height > room {
            break;
        }
        room -= height + SIDE_GAP;
        rows.push(row);
    }
    rows
}

pub struct SamplerView<'a> {
    pub track_id: TrackId,
    pub device: DeviceParams<'a>,
    /// What the Load button asks for.
    pub load: Message,
    pub audio: Option<&'a Arc<DecodedAudio>>,
    pub sample_name: Option<String>,
    pub accent: Color,
    pub panel: InstrumentPanel,
    pub entry: Option<&'a ValueEntry>,
}

impl<'a> SamplerView<'a> {
    fn param(&self, index: usize) -> Param<'a> {
        let param = Param::new(self.track_id, self.device, index, self.accent, self.entry);
        match index {
            param::ROOT_NOTE => param.label("Root"),
            param::VOLUME => param.label("Gain"),
            param::CROSSFADE => param.label("Xfade"),
            param::CUTOFF => param.label("Cutoff"),
            param::RESONANCE => param.label("Resonance"),
            param::FILTER_ENV => param.label("Env"),
            param::LFO_RATE => param.label("Rate"),
            param::LFO_AMOUNT => param.label("Amount"),
            param::VELOCITY_LOW => param.label("Vel Low"),
            param::VELOCITY_HIGH => param.label("Vel High"),
            param::ATTACK | param::FILTER_ATTACK => param.label("Attack"),
            param::DECAY | param::FILTER_DECAY => param.label("Decay"),
            param::SUSTAIN | param::FILTER_SUSTAIN => param.label("Sustain"),
            param::RELEASE | param::FILTER_RELEASE => param.label("Release"),
            _ => param,
        }
    }

    fn on_pad(&self) -> bool {
        matches!(self.device.slot, DeviceSlot::DrumPad(_))
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
            self.sample(layout),
        ];
        if layout.section > 0.0 {
            let track_id = self.track_id;
            let pages: Vec<(SamplerPage, &str)> = pages(self.on_pad())
                .iter()
                .map(|page| (*page, page.name()))
                .collect();
            body = body
                .push(rule())
                .push(
                    container(tabs(
                        &pages,
                        SamplerPage::at(self.panel.page),
                        move |page| {
                            Message::InstrumentCard(InstrumentCardMsg::ShowPage(
                                track_id,
                                page.index(),
                            ))
                        },
                        self.accent,
                    ))
                    .padding([0.0, PAD])
                    .height(TABS_H)
                    .align_y(Alignment::Center),
                )
                .push(rule())
                .push(self.page(layout));
        }
        body.width(Length::Fill).height(Length::Fill).into()
    }

    fn markers(&self) -> Markers {
        let value = |index: usize| self.device.value(index);
        Markers {
            start: value(param::START),
            end: value(param::END),
            looped: (Mode::of(self.device.params) == Mode::Loop)
                .then(|| (value(param::LOOP_START), value(param::LOOP_END))),
        }
    }

    fn sample(&self, layout: Layout) -> Element<'a, Message> {
        let length = self
            .audio
            .map(|audio| audio.num_frames() as f32 / audio.sample_rate.max(1) as f32);
        let name = text(
            self.sample_name
                .clone()
                .unwrap_or_else(|| "No sample".into()),
        )
        .size(LABEL_SIZE + 1.0)
        .wrapping(text::Wrapping::None)
        .color(if self.sample_name.is_some() {
            th::text()
        } else {
            th::text_muted()
        });
        let mut header = row![container(name).width(Length::Fill).clip(true)]
            .spacing(10)
            .align_y(Alignment::Center);
        if let Some(seconds) = length {
            header = header.push(heading(format!("{seconds:.3} s")));
        }
        if !layout.compact {
            for &index in header_controls(self.on_pad()) {
                let param = self.param(index);
                header = header.push(
                    row![heading(param.name()), param.value_entry()]
                        .spacing(4)
                        .align_y(Alignment::Center),
                );
            }
        }
        header = header.push(
            button(text("Load").size(LABEL_SIZE))
                .on_press(self.load.clone())
                .padding([2, 10])
                .style(|_theme: &Theme, status| button::Style {
                    background: Some(
                        if matches!(status, button::Status::Hovered | button::Status::Pressed) {
                            th::bg_hover()
                        } else {
                            th::bg_surface()
                        }
                        .into(),
                    ),
                    text_color: th::text(),
                    border: iced::Border {
                        color: th::border(),
                        width: 1.0,
                        radius: 3.0.into(),
                    },
                    ..Default::default()
                }),
        );
        let mut left = column![container(header)
            .height(SAMPLE_HEADER_H)
            .align_y(Alignment::Center)];
        if layout.wave > 0.0 {
            left = left.push(
                iced::widget::canvas(SampleView {
                    track_id: self.track_id,
                    slot: self.device.slot,
                    audio: self.audio,
                    markers: self.markers(),
                    accent: self.accent,
                })
                .width(Length::Fill)
                .height(layout.wave),
            );
        }
        if layout.compact {
            if layout.mode_line {
                left = left.push(
                    container(self.playback_line())
                        .height(MODE_LINE_H)
                        .align_y(Alignment::Center),
                );
            }
            return plot_band(left.width(Length::Fill), layout.sample, PAD);
        }
        let mut side = column![].spacing(SIDE_GAP).width(SIDE_W);
        for row in side_rows(layout.sample - PAD) {
            side = side.push(match row {
                Side::Mode => column![heading("Mode"), self.mode()].spacing(4).into(),
                Side::Reverse => self.param(param::REVERSE).switch("Reverse"),
                Side::Crossfade => self.param(param::CROSSFADE).line_item(),
            });
        }
        plot_band(
            row![left.width(Length::Fill), vertical_rule(), side].spacing(8),
            layout.sample,
            PAD,
        )
    }

    fn mode(&self) -> Element<'a, Message> {
        let (track_id, slot) = (self.track_id, self.device.slot);
        segmented(
            &Mode::ALL,
            Mode::of(self.device.params),
            move |mode| Message::Devices(slot.set_all(track_id, mode.updates())),
            self.accent,
        )
    }

    fn playback_line(&self) -> Element<'a, Message> {
        row![
            self.mode(),
            self.param(param::REVERSE).switch("Reverse"),
            self.param(param::CROSSFADE).line_item(),
        ]
        .spacing(10)
        .align_y(Alignment::Center)
        .into()
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
        let mut row_of_knobs = row![].spacing(6).align_y(Alignment::Center);
        for &index in knobs {
            row_of_knobs = row_of_knobs.push(if layout.knobs < KNOB_ROW_H {
                self.param(index).line_item()
            } else {
                container(self.param(index).column(KNOB))
                    .width(Length::Fill)
                    .max_width(110.0)
                    .align_x(Alignment::Center)
                    .into()
            });
        }
        let header = container(header)
            .height(HEADER_H)
            .align_y(Alignment::Center);
        let plot = plot.filter(|_| layout.plot > 0.0);
        if layout.beside {
            let body = layout.section - HEADER_H;
            let mut content = row![].spacing(14).align_y(Alignment::Center);
            if let Some(plot) = plot {
                content = content.push(
                    container(plot)
                        .height(body)
                        .width(Length::FillPortion(PLOT_SHARE)),
                );
            }
            content = content.push(
                container(row_of_knobs)
                    .width(Length::FillPortion(KNOBS_SHARE))
                    .height(body)
                    .center_y(body),
            );
            return column![header, content].width(Length::Fill).into();
        }
        let mut section = column![header];
        if let Some(plot) = plot {
            section = section.push(container(plot).height(layout.plot).width(Length::Fill));
        }
        section
            .push(
                container(row_of_knobs)
                    .height(layout.knobs)
                    .center_y(layout.knobs),
            )
            .width(Length::Fill)
            .into()
    }

    fn envelope(&self, layout: Layout, filter: bool) -> Element<'a, Message> {
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
        let mut knobs = indices.to_vec();
        if !filter && layout.compact {
            knobs.push(param::VOLUME);
        }
        let name = if filter {
            "Filter Envelope"
        } else {
            "Amp Envelope"
        };
        let mut header = row![heading(name), Space::with_width(Length::Fill)]
            .spacing(6)
            .align_y(Alignment::Center);
        if filter && layout.compact {
            header = header.push(self.filter_switch());
        }
        self.section(layout, header.into(), Some(plot.into()), &knobs)
    }

    /// Switches a compact card's Filter tab between the filter and its
    /// envelope.
    fn filter_switch(&self) -> Element<'a, Message> {
        let track_id = self.track_id;
        segmented(
            &[(false, "Filter"), (true, "Envelope")],
            self.panel.filter_envelope,
            move |envelope| {
                Message::InstrumentCard(InstrumentCardMsg::ShowFilterEnvelope(track_id, envelope))
            },
            self.accent,
        )
    }

    fn filter(&self, layout: Layout) -> Element<'a, Message> {
        let kind = self.whole(param::FILTER_TYPE);
        let mut header = row![
            heading("Filter"),
            Space::with_width(Length::Fill),
            container(self.param(param::FILTER_TYPE).choices(&FILTER_NAMES, 0)).width(CHOICE_W)
        ]
        .spacing(6)
        .align_y(Alignment::Center);
        if layout.compact {
            header = header.push(self.filter_switch());
        }
        let plot = iced::widget::canvas(FilterPlot {
            // The sampler's types follow its Off, in the synth's order.
            filter_type: (kind != filter_type::OFF).then(|| kind - 1),
            cutoff: self.device.value(param::CUTOFF),
            resonance: self.device.value(param::RESONANCE),
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

    fn page(&self, layout: Layout) -> Element<'a, Message> {
        let content: Element<'a, Message> = match SamplerPage::at(self.panel.page) {
            SamplerPage::Sample => self.envelope(layout, false),
            SamplerPage::Zones => self.section(
                layout,
                heading("Key and Velocity Zone").into(),
                None,
                &[
                    param::KEY_LOW,
                    param::KEY_HIGH,
                    param::VELOCITY_LOW,
                    param::VELOCITY_HIGH,
                    param::VELOCITY_AMOUNT,
                ],
            ),
            SamplerPage::Pitch => self.section(
                layout,
                heading("Pitch").into(),
                None,
                pitch_controls(self.on_pad()),
            ),
            SamplerPage::Filter if layout.compact && self.panel.filter_envelope => {
                self.envelope(layout, true)
            }
            SamplerPage::Filter if layout.compact => self.filter(layout),
            SamplerPage::Filter => row![
                self.filter(layout),
                vertical_rule(),
                self.envelope(layout, true)
            ]
            .spacing(10)
            .into(),
            SamplerPage::Modulation => {
                let header = row![
                    heading("LFO"),
                    Space::with_width(Length::Fill),
                    container(self.param(param::LFO_SHAPE).choices(&LFO_SHAPE_NAMES, 0))
                        .width(CHOICE_W),
                    heading("to"),
                    container(self.param(param::LFO_TARGET).choices(&LFO_TARGET_NAMES, 0))
                        .width(CHOICE_W),
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
        };
        container(content)
            .padding([0.0, PAD])
            .height(layout.section)
            .into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_supported_size_fits_without_scrolling() {
        for width in [COMPACT_W, MEDIUM_W, MAX_W] {
            for height in (56..=520).step_by(2).map(|height| height as f32) {
                let layout = arrange(Size::new(width, height));
                let context = format!("{width}x{height}: {layout:?}");
                let mut used = PRESET_ROW_H + 1.0 + layout.sample;
                if layout.section > 0.0 {
                    used += 1.0 + TABS_H + 1.0 + layout.section;
                    assert!(layout.plot == 0.0 || layout.plot >= PLOT_MIN_H, "{context}");
                    assert!(layout.wave >= WAVE_MIN_H, "{context}");
                    assert!(
                        HEADER_H + layout.knobs <= layout.section + 0.01,
                        "{context}"
                    );
                }
                assert!(used <= height + 0.01, "{context}");
            }
        }
    }

    /// The playback line under a compact card's sample.
    fn mode_line(layout: Layout) -> f32 {
        if layout.mode_line {
            MODE_LINE_H
        } else {
            0.0
        }
    }

    /// The body an instrument card gets in the 180 px minimum detail
    /// panel, measured in the app.
    const MINIMUM_BODY_H: f32 = 68.0;

    #[test]
    fn the_sample_band_holds_everything_it_shows() {
        for width in [COMPACT_W, MEDIUM_W, MAX_W] {
            for height in (MINIMUM_BODY_H as usize..=520)
                .step_by(2)
                .map(|height| height as f32)
            {
                let layout = arrange(Size::new(width, height));
                // `plot_band` keeps 6 px clear above and below.
                let shown = 12.0 + SAMPLE_HEADER_H + layout.wave + mode_line(layout);
                assert!(
                    shown <= layout.sample + 0.01,
                    "{width}x{height}: {layout:?}"
                );
            }
        }
    }

    #[test]
    fn the_side_column_is_never_left_empty_while_a_line_fits() {
        // The wide card's sample band at the minimum panel.
        let room = arrange(Size::new(MEDIUM_W, MINIMUM_BODY_H)).sample - PAD;
        assert!(room >= LINE_H, "{room}");
        assert!(!side_rows(room).is_empty());
        assert!(side_rows(LINE_H - 1.0).is_empty());
    }

    #[test]
    fn the_shortest_card_still_shows_the_waveform() {
        let shortest = arrange(Size::new(MAX_W, 82.0));
        assert_eq!(shortest.section, 0.0);
        assert!(shortest.wave >= WAVE_ALONE_MIN_H, "{shortest:?}");
    }

    #[test]
    fn a_detail_panel_shows_the_waveform_and_a_section() {
        // A 360 px detail panel leaves a card about 250 px of body.
        let panel = arrange(Size::new(MEDIUM_W, 250.0));
        assert!(panel.wave > 0.0 && panel.section > 0.0, "{panel:?}");
        assert!(
            panel.plot > 0.0 && panel.knobs == KNOB_ROW_H,
            "the envelope beside its knobs"
        );
    }
}
