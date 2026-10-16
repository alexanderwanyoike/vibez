//! Native Drum Rack body: a bank of sixteen pads beside the selected pad,
//! which is a Sampler and shows as the Sampler's own body. Narrow cards show
//! the pads or the pad's Sampler behind tabs.

use std::cell::Cell;
use std::sync::Arc;
use std::time::Instant;

use iced::mouse;
use iced::widget::{button, canvas, column, container, mouse_area, row, text, text_input};
use iced::{Alignment, Color, Element, Length, Point, Rectangle, Renderer, Size, Theme};
use vibez_core::audio_buffer::DecodedAudio;
use vibez_core::id::TrackId;
use vibez_core::track::{drum_rack_pad_pitch, DRUM_RACK_BANK_COUNT, DRUM_RACK_BANK_SIZE};
use vibez_instruments::sampler::param;

use crate::domains::browser::BrowserMsg;
use crate::domains::device_params::DeviceParams;
use crate::domains::devices::DevicesMsg;
use crate::domains::instrument_card::{InstrumentCardMsg, InstrumentPanel};
use crate::domains::param_entry::ValueEntry;
use crate::message::Message;
use crate::state::UiDrumPad;
use crate::theme as th;
use crate::widgets::device_controls::{heading, rule, segmented, tabs, vertical_rule, LABEL_SIZE};
use crate::widgets::mini_waveform::column_peaks;
use crate::widgets::on_blur::on_blur;
use crate::widgets::sampler_device::SamplerView;

const PAD: f32 = 10.0;
const GAP: f32 = 4.0;
const TABS_H: f32 = 24.0;
const BANK_ROW_H: f32 = 24.0;
/// The selected pad's name, note, choke group and Clear, above its Sampler.
const STRIP_H: f32 = 30.0;
/// The share of a side-by-side card the pads take.
const GRID_SHARE: f32 = 0.3;
const GRID_MIN_W: f32 = 200.0;
const GRID_MAX_W: f32 = 340.0;
/// Below this a pad has no room for its waveform under its name.
const WAVE_PAD_H: f32 = 34.0;
/// Below this a pad's name takes its whole width, its note shown only in
/// the selected pad's strip.
const NOTE_PAD_W: f32 = 72.0;
/// Below this the bank row keeps only its banks.
const LABELLED_BANKS_W: f32 = 280.0;
/// How long the cursor rests on a long name before it starts to scroll,
/// and how fast it then moves.
const MARQUEE_DELAY_S: f32 = 0.4;
const MARQUEE_PX_PER_S: f32 = 28.0;
const MARQUEE_GAP: f32 = 24.0;
/// The average advance of a label character, as a share of its size.
const CHAR_W: f32 = 0.56;
const CHOKES: [(Option<u8>, &str); 5] = [
    (None, "Off"),
    (Some(1), "1"),
    (Some(2), "2"),
    (Some(3), "3"),
    (Some(4), "4"),
];

pub const COMPACT_W: f32 = crate::widgets::sampler_device::COMPACT_W;
/// The pads fit beside a compact Sampler.
pub const SIDE_BY_SIDE_W: f32 =
    GRID_MIN_W + PAD * 2.0 + 1.0 + crate::widgets::sampler_device::COMPACT_W;
pub const MAX_W: f32 = 1_600.0;

pub fn card_width(available: f32) -> f32 {
    available.clamp(COMPACT_W, MAX_W)
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Layout {
    /// The pads sit beside the Sampler rather than behind a tab.
    pub side_by_side: bool,
    pub grid_w: f32,
    pub pad_h: f32,
    /// The selected pad's Sampler body.
    pub sampler: Size,
}

pub fn arrange(size: Size) -> Layout {
    let side_by_side = size.width >= SIDE_BY_SIDE_W;
    let tabs = if side_by_side { 0.0 } else { TABS_H + 1.0 };
    let grid_h = (size.height - tabs - PAD * 2.0 - BANK_ROW_H - GAP).max(0.0);
    let pad_h = ((grid_h - GAP * 3.0) / 4.0).max(0.0);
    let grid_w = if side_by_side {
        let room = size.width - PAD * 2.0 - 1.0 - crate::widgets::sampler_device::COMPACT_W;
        (size.width * GRID_SHARE)
            .clamp(GRID_MIN_W, GRID_MAX_W)
            .min(room)
    } else {
        size.width - PAD * 2.0
    };
    let sampler_w = if side_by_side {
        size.width - grid_w - PAD * 2.0 - 1.0
    } else {
        size.width
    };
    Layout {
        side_by_side,
        grid_w,
        pad_h,
        sampler: Size::new(sampler_w, (size.height - tabs - STRIP_H - 1.0).max(0.0)),
    }
}

/// A name cut to what fits `width`, ending in an ellipsis when it was cut.
fn fitted(name: &str, width: f32, size: f32) -> String {
    let fits = (width / (size * CHAR_W)).floor().max(0.0) as usize;
    if name.chars().count() <= fits {
        return name.to_string();
    }
    let mut cut: String = name.chars().take(fits.saturating_sub(1)).collect();
    cut.push('\u{2026}');
    cut
}

/// One pad: its name, which scrolls while hovered when too long to show,
/// its note, and the part of its sample it plays.
pub struct PadView<'a> {
    pub name: Option<&'a str>,
    pub note: String,
    pub audio: Option<&'a Arc<DecodedAudio>>,
    /// Holds a sample, decoded or not.
    pub filled: bool,
    pub region: (f32, f32),
    pub selected: bool,
    pub firing: bool,
    pub drop_target: bool,
    pub accent: Color,
}

#[derive(Default)]
pub struct PadViewState {
    hovered_since: Option<Instant>,
    cache: canvas::Cache,
    /// (sample, region, width, colour, theme) the waveform was drawn for.
    drawn_for: Cell<(usize, u32, u32, u32, bool, u64)>,
}

impl PadView<'_> {
    fn wave_color(&self) -> Color {
        if self.selected {
            self.accent
        } else {
            th::with_alpha(self.accent, 0.55)
        }
    }

    fn draw_wave(&self, state: &PadViewState, renderer: &Renderer, size: Size) -> canvas::Geometry {
        let Some(audio) = self.audio else {
            return state.cache.draw(renderer, size, |_| {});
        };
        let key = (
            Arc::as_ptr(audio) as usize,
            self.region.0.to_bits(),
            self.region.1.to_bits(),
            size.width as u32,
            self.selected,
            th::epoch(),
        );
        if state.drawn_for.get() != key {
            state.cache.clear();
            state.drawn_for.set(key);
        }
        state.cache.draw(renderer, size, |frame| {
            let Some(samples) = audio.channels.first() else {
                return;
            };
            let frames = samples.len();
            let from = ((self.region.0.clamp(0.0, 1.0) * frames as f32) as usize).min(frames);
            let to = ((self.region.1.clamp(0.0, 1.0) * frames as f32) as usize).clamp(from, frames);
            let band = Rectangle::new(
                Point::new(6.0, size.height * 0.5),
                Size::new((size.width - 12.0).max(1.0), size.height * 0.5 - 4.0),
            );
            let columns = band.width as usize;
            let mid = band.y + band.height / 2.0;
            let half = band.height / 2.0;
            let color = self.wave_color();
            for (column, (low, high)) in column_peaks(&samples[from..to], columns).enumerate() {
                let x = band.x + column as f32;
                let top = mid - high.clamp(-1.0, 1.0) * half;
                let bottom = mid - low.clamp(-1.0, 1.0) * half;
                frame.fill_rectangle(
                    Point::new(x, top),
                    Size::new(1.0, (bottom - top).max(1.0)),
                    color,
                );
            }
        })
    }
}

impl canvas::Program<Message> for PadView<'_> {
    type State = PadViewState;

    fn update(
        &self,
        state: &mut PadViewState,
        event: canvas::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> (canvas::event::Status, Option<Message>) {
        if let canvas::Event::Mouse(mouse::Event::CursorMoved { .. } | mouse::Event::CursorLeft) =
            event
        {
            let over = cursor.is_over(bounds);
            if over != state.hovered_since.is_some() {
                state.hovered_since = over.then(Instant::now);
            }
        }
        // The pad's mouse area selects it and takes dropped samples.
        (canvas::event::Status::Ignored, None)
    }

    fn draw(
        &self,
        state: &PadViewState,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let size = bounds.size();
        let mut frame = canvas::Frame::new(renderer, size);
        let filled = self.filled;
        let body = canvas::Path::rounded_rectangle(
            Point::new(0.5, 0.5),
            Size::new(size.width - 1.0, size.height - 1.0),
            3.0.into(),
        );
        let background = if self.firing {
            th::with_alpha(self.accent, 0.45)
        } else if self.selected {
            th::with_alpha(self.accent, 0.16)
        } else if filled {
            th::bg_surface()
        } else {
            th::bg_dark()
        };
        frame.fill(&body, background);
        let edge = if self.drop_target || self.selected || self.firing {
            self.accent
        } else {
            th::border()
        };
        let mut stroke = canvas::Stroke::default()
            .with_color(edge)
            .with_width(if self.drop_target { 2.0 } else { 1.0 });
        if !filled && !self.drop_target {
            stroke.line_dash = canvas::LineDash {
                segments: &[3.0, 3.0],
                offset: 0,
            };
        }
        frame.stroke(&body, stroke);

        let size_px = LABEL_SIZE;
        let mut room = size.width - 12.0;
        if size.width >= NOTE_PAD_W {
            room -= self.note.chars().count() as f32 * size_px * CHAR_W + 6.0;
            frame.fill_text(canvas::Text {
                content: self.note.clone(),
                position: Point::new(size.width - 6.0, 4.0),
                color: th::text_muted(),
                size: size_px.into(),
                horizontal_alignment: iced::alignment::Horizontal::Right,
                ..Default::default()
            });
        }
        let name_area = Rectangle::new(
            Point::new(6.0, 3.0),
            Size::new(room.max(0.0), size_px + 4.0),
        );
        let hint = "Drop a sample";
        let (name, color) = match self.name {
            Some(name) => (name, th::text()),
            // The dashed edge marks an empty pad where the hint does not fit.
            None if fitted(hint, name_area.width, size_px) == hint => (hint, th::text_muted()),
            None => ("", th::text_muted()),
        };
        let name_w = name.chars().count() as f32 * size_px * CHAR_W;
        let scrolling = state.hovered_since.and_then(|since| {
            let resting = since.elapsed().as_secs_f32() - MARQUEE_DELAY_S;
            (name_w > name_area.width && resting > 0.0).then_some(resting)
        });
        frame.with_clip(name_area, |frame| {
            let line = |content: String, x: f32| canvas::Text {
                content,
                position: Point::new(x, 1.0),
                color,
                size: size_px.into(),
                ..Default::default()
            };
            match scrolling {
                Some(resting) => {
                    let lap = name_w + MARQUEE_GAP;
                    let x = -(resting * MARQUEE_PX_PER_S) % lap;
                    frame.fill_text(line(name.to_string(), x));
                    frame.fill_text(line(name.to_string(), x + lap));
                }
                None => frame.fill_text(line(fitted(name, name_area.width, size_px), 0.0)),
            }
        });
        let mut geometry = vec![frame.into_geometry()];
        if size.height >= WAVE_PAD_H {
            geometry.push(self.draw_wave(state, renderer, size));
        }
        geometry
    }

    fn mouse_interaction(
        &self,
        _state: &PadViewState,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if cursor.is_over(bounds) {
            mouse::Interaction::Pointer
        } else {
            mouse::Interaction::default()
        }
    }
}

/// A bank's letter beside a map of which of its pads hold samples.
struct BankView {
    letter: char,
    filled: [bool; DRUM_RACK_BANK_SIZE],
    selected: bool,
    firing: bool,
    accent: Color,
}

impl canvas::Program<Message> for BankView {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let lit = self.selected || self.firing;
        frame.fill_text(canvas::Text {
            content: self.letter.to_string(),
            position: Point::new(2.0, bounds.height / 2.0),
            color: if lit { th::text() } else { th::text_dim() },
            size: LABEL_SIZE.into(),
            vertical_alignment: iced::alignment::Vertical::Center,
            ..Default::default()
        });
        let dot = 3.0;
        let left = bounds.width - (dot + 1.0) * 4.0;
        let top = (bounds.height - (dot + 1.0) * 4.0) / 2.0;
        for (index, filled) in self.filled.iter().enumerate() {
            // Rows from the bottom, as the pads are laid out.
            let (column, row) = (index % 4, 3 - index / 4);
            frame.fill_rectangle(
                Point::new(
                    left + column as f32 * (dot + 1.0),
                    top + row as f32 * (dot + 1.0),
                ),
                Size::new(dot, dot),
                if *filled {
                    th::with_alpha(self.accent, if lit { 1.0 } else { 0.6 })
                } else {
                    th::border()
                },
            );
        }
        vec![frame.into_geometry()]
    }
}

pub struct DrumRackView<'a> {
    pub track_id: TrackId,
    pub pads: &'a [UiDrumPad],
    pub selected: usize,
    /// The selected pad's Sampler.
    pub device: DeviceParams<'a>,
    /// Which pads of the shown bank are sounding, in pad order.
    pub firing: [bool; DRUM_RACK_BANK_SIZE],
    pub banks_firing: [bool; DRUM_RACK_BANK_COUNT],
    pub drop_target: Option<usize>,
    pub accent: Color,
    pub panel: InstrumentPanel,
    pub entry: Option<&'a ValueEntry>,
}

impl<'a> DrumRackView<'a> {
    fn bank(&self) -> usize {
        (self.selected / DRUM_RACK_BANK_SIZE).min(DRUM_RACK_BANK_COUNT - 1)
    }

    pub fn view(self, preset_bar: Element<'a, Message>, size: Size) -> Element<'a, Message> {
        let layout = arrange(size);
        if layout.side_by_side {
            return row![
                container(self.pads(layout))
                    .padding(PAD)
                    .width(layout.grid_w + PAD * 2.0),
                vertical_rule(),
                self.editor(preset_bar, layout),
            ]
            .height(Length::Fill)
            .into();
        }
        let track_id = self.track_id;
        let options = [(false, "Pads"), (true, "Sampler")];
        let strip = container(tabs(
            &options,
            self.panel.pad_sampler,
            move |sampler| {
                Message::InstrumentCard(InstrumentCardMsg::ShowPadSampler(track_id, sampler))
            },
            self.accent,
        ))
        .padding([0.0, PAD])
        .height(TABS_H)
        .align_y(Alignment::Center);
        let shown = if self.panel.pad_sampler {
            self.editor(preset_bar, layout)
        } else {
            container(self.pads(layout)).padding(PAD).into()
        };
        column![strip, rule(), shown].height(Length::Fill).into()
    }

    fn pads(&self, layout: Layout) -> Element<'a, Message> {
        let bank = self.bank();
        let first = bank * DRUM_RACK_BANK_SIZE;
        let mut grid = column![].spacing(GAP);
        // The bank's lowest pads sit at the bottom, as on a pad controller.
        for pad_row in (0..4).rev() {
            let mut cells = row![].spacing(GAP);
            for pad_column in 0..4 {
                let offset = pad_row * 4 + pad_column;
                cells = cells.push(self.pad(first + offset, self.firing[offset], layout));
            }
            grid = grid.push(cells);
        }
        column![self.banks(layout.grid_w), grid]
            .spacing(GAP)
            .width(layout.grid_w)
            .into()
    }

    fn pad(&self, index: usize, firing: bool, layout: Layout) -> Element<'a, Message> {
        let pad = &self.pads[index];
        let (start, end) = (pad.param(param::START), pad.param(param::END));
        let view = PadView {
            name: pad.name.as_deref(),
            note: drum_rack_pad_pitch(index)
                .map(vibez_core::midi::pitch_name)
                .unwrap_or_default(),
            audio: pad.audio.as_ref(),
            filled: pad.holds_sample(),
            region: (start, end),
            selected: index == self.selected,
            firing,
            drop_target: self.drop_target == Some(index),
            accent: self.accent,
        };
        let track_id = self.track_id;
        mouse_area(canvas(view).width(Length::Fill).height(layout.pad_h))
            .on_enter(Message::Browser(BrowserMsg::DragHoverDrumRackPad {
                track_id,
                pad_index: index,
            }))
            .on_exit(Message::Browser(BrowserMsg::ClearDragTarget))
            // Selects the pad, or loads the sample being dragged onto it.
            .on_release(Message::DropSampleOnDrumPad {
                track_id,
                pad_index: index,
            })
            .into()
    }

    fn banks(&self, width: f32) -> Element<'a, Message> {
        let current = self.bank();
        let labelled = width >= LABELLED_BANKS_W;
        let mut strip = row![].spacing(6).align_y(Alignment::Center);
        if labelled {
            strip = strip.push(heading("Banks"));
        }
        for bank in 0..DRUM_RACK_BANK_COUNT {
            let first = bank * DRUM_RACK_BANK_SIZE;
            let filled = std::array::from_fn(|offset| self.pads[first + offset].holds_sample());
            let selected = bank == current;
            let accent = self.accent;
            let map = canvas(BankView {
                letter: char::from(b'A' + bank as u8),
                filled,
                selected,
                firing: self.banks_firing[bank],
                accent,
            })
            .width(30.0)
            .height(BANK_ROW_H - 6.0);
            strip = strip.push(
                button(map)
                    .on_press(Message::Devices(DevicesMsg::SelectDrumRackBank(
                        self.track_id,
                        bank,
                    )))
                    .padding([1, 4])
                    .style(move |_theme: &Theme, status| button::Style {
                        background:
                            Some(
                                if matches!(
                                    status,
                                    button::Status::Hovered | button::Status::Pressed
                                ) {
                                    th::bg_hover()
                                } else {
                                    th::bg_dark()
                                }
                                .into(),
                            ),
                        border: iced::Border {
                            color: if selected { accent } else { th::border() },
                            width: 1.0,
                            radius: 3.0.into(),
                        },
                        ..Default::default()
                    }),
            );
        }
        if labelled {
            strip = strip
                .push(iced::widget::horizontal_space())
                .push(heading(first_and_last(current)));
        }
        container(strip)
            .height(BANK_ROW_H)
            .align_y(Alignment::Center)
            .into()
    }

    fn editor(self, preset_bar: Element<'a, Message>, layout: Layout) -> Element<'a, Message> {
        let pad = &self.pads[self.selected];
        let track_id = self.track_id;
        let pad_index = self.selected;
        let accent = self.accent;
        let draft = self.panel.pad_name_draft(pad_index);
        let typing = draft.is_some();
        let name = text_input(
            "Pad name",
            draft.or(pad.name.as_deref()).unwrap_or_default(),
        )
        .on_input(move |name| {
            Message::InstrumentCard(InstrumentCardMsg::EditPadName(track_id, pad_index, name))
        })
        .on_submit(Message::InstrumentCard(InstrumentCardMsg::CommitPadName(
            track_id,
        )))
        .size(LABEL_SIZE + 2.0)
        .padding([2, 4])
        .style(move |_theme: &Theme, status| text_input::Style {
            background: th::bg_dark().into(),
            border: iced::Border {
                color: if matches!(status, text_input::Status::Focused) {
                    accent
                } else {
                    iced::Color::TRANSPARENT
                },
                width: 1.0,
                radius: 2.0.into(),
            },
            icon: th::text_dim(),
            placeholder: th::text_muted(),
            value: th::text(),
            selection: th::with_alpha(accent, 0.35),
        });
        let name = on_blur(
            name,
            typing,
            Message::InstrumentCard(InstrumentCardMsg::CommitPadName(track_id)),
        );
        let note = drum_rack_pad_pitch(pad_index)
            .map(vibez_core::midi::pitch_name)
            .unwrap_or_default();
        let choke = segmented(
            &CHOKES,
            pad.choke_group,
            move |choke_group| {
                Message::Devices(DevicesMsg::SetDrumPadChokeGroup {
                    track_id,
                    pad_index,
                    choke_group,
                })
            },
            accent,
        );
        let clear = button(text("Clear").size(LABEL_SIZE))
            .on_press_maybe(
                pad.holds_sample()
                    .then(|| Message::clear_drum_rack_pad(track_id, pad_index)),
            )
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
                text_color: if matches!(status, button::Status::Disabled) {
                    th::text_muted()
                } else {
                    th::text()
                },
                border: iced::Border {
                    color: th::border(),
                    width: 1.0,
                    radius: 3.0.into(),
                },
                ..Default::default()
            });
        let strip = container(
            row![
                container(name).width(Length::Fill).max_width(260.0),
                heading(note),
                iced::widget::horizontal_space(),
                heading("Choke"),
                choke,
                clear,
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        )
        .padding([0.0, PAD])
        .height(STRIP_H)
        .align_y(Alignment::Center);
        let sampler = SamplerView {
            track_id,
            device: self.device,
            load: Message::LoadDrumRackPadSample(track_id, pad_index),
            audio: pad.audio.as_ref(),
            sample_name: pad.source.as_ref().map(|source| source.display_name()),
            accent,
            panel: self.panel.clone(),
            entry: self.entry,
        }
        .view(preset_bar, layout.sampler);
        column![strip, rule(), sampler]
            .width(layout.sampler.width)
            .height(Length::Fill)
            .into()
    }
}

/// The notes a bank's pads play, lowest to highest.
fn first_and_last(bank: usize) -> String {
    let first = bank * DRUM_RACK_BANK_SIZE;
    let name = |pad: usize| {
        drum_rack_pad_pitch(pad)
            .map(vibez_core::midi::pitch_name)
            .unwrap_or_default()
    };
    format!(
        "{} to {}",
        name(first),
        name(first + DRUM_RACK_BANK_SIZE - 1)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pads_fit_and_a_side_by_side_sampler_keeps_its_compact_width() {
        for width in [
            COMPACT_W,
            SIDE_BY_SIDE_W - 1.0,
            SIDE_BY_SIDE_W,
            900.0,
            MAX_W,
        ] {
            for height in (120..=520).step_by(4).map(|height| height as f32) {
                let layout = arrange(Size::new(width, height));
                let context = format!("{width}x{height}: {layout:?}");
                let tabs = if layout.side_by_side {
                    0.0
                } else {
                    TABS_H + 1.0
                };
                let pads = tabs + PAD * 2.0 + BANK_ROW_H + GAP + layout.pad_h * 4.0 + GAP * 3.0;
                assert!(pads <= height + 0.01, "{context}");
                assert!(
                    layout.grid_w >= GRID_MIN_W.min(width - PAD * 2.0),
                    "{context}"
                );
                if layout.side_by_side {
                    assert!(
                        layout.sampler.width >= crate::widgets::sampler_device::COMPACT_W - 0.01,
                        "{context}"
                    );
                }
            }
        }
    }

    #[test]
    fn a_long_name_is_cut_with_an_ellipsis_and_a_short_one_kept() {
        assert_eq!(fitted("Kick", 100.0, 10.0), "Kick");
        let cut = fitted("Open Hat Long Tail 03", 50.0, 10.0);
        assert!(cut.ends_with('\u{2026}'), "{cut}");
        assert!(cut.chars().count() as f32 * 10.0 * CHAR_W <= 50.0, "{cut}");
    }

    #[test]
    fn a_bank_names_its_notes() {
        assert_eq!(first_and_last(0), "C2 to D#3");
        assert_eq!(first_and_last(3), "C6 to D#7");
    }
}
