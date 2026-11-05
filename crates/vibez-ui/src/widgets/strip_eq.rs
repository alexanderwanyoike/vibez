//! The channel strip's EQ laid out like a Z&H V250 faceplate: four gain
//! knobs down the left, each band's frequency, Q and switches staggered half
//! a step lower on the right, with Z-shaped rules between the bands.

use iced::mouse;
use iced::widget::{button, canvas, column, container, stack, text};
use iced::{Color, Element, Length, Point, Rectangle, Renderer, Theme};
use vibez_core::effect::EffectType;
use vibez_core::id::TrackId;

use crate::message::Message;
use crate::state::{ProjectTrack, UiEffect};
use crate::theme as th;
use crate::typography::PERFORM_LABEL;
use crate::widgets::control_style;
use crate::widgets::effect_knob::{EffectKnobWidget, KnobCap};

pub const EQ_W: f32 = 96.0;
pub const EQ_H: f32 = 240.0;

const GAIN_W: f32 = 48.0;
const RIGHT_X: f32 = 50.0;
const RIGHT_W: f32 = EQ_W - RIGHT_X;
const GAIN_KNOB: f32 = 30.0;
const OUTER_FREQ_KNOB: f32 = 24.0;
const MID_FREQ_KNOB: f32 = 20.0;
const Q_KNOB: f32 = 18.0;
const MID_FREQ_W: f32 = 24.0;
const Q_X: f32 = RIGHT_X + MID_FREQ_W;
const Q_W: f32 = EQ_W - Q_X;

/// Where each Z rule leaves the gain column and where it lands on the
/// right, half a step lower, in block pixels from the top.
const Z_RULES: [(f32, f32); 3] = [(54.0, 62.0), (113.0, 118.0), (175.0, 180.0)];
/// Where the rule steps down, between the two columns.
const Z_STEP: (f32, f32) = (40.0, RIGHT_X);

/// Parameter indices of the built-in EQ, by band.
struct Band {
    label: &'static str,
    gain: usize,
    freq: usize,
    /// Q on the mid bands, the bell switch on the shelving ones.
    third: usize,
    shelving: bool,
    /// Top of the band's gain knob, label and all.
    top: f32,
    /// How far below `top` the mid bands' frequency and Q sit; LMF's sit
    /// lower to leave the EQ IN switch room above them.
    right_drop: f32,
}

const BANDS: [Band; 4] = [
    Band {
        label: "HF",
        gain: 9,
        freq: 10,
        third: 11,
        shelving: true,
        top: 0.0,
        right_drop: 0.0,
    },
    Band {
        label: "HMF",
        gain: 6,
        freq: 7,
        third: 8,
        shelving: false,
        top: 58.0,
        right_drop: 10.0,
    },
    Band {
        label: "LMF",
        gain: 3,
        freq: 4,
        third: 5,
        shelving: false,
        top: 118.0,
        right_drop: 22.0,
    },
    Band {
        label: "LF",
        gain: 0,
        freq: 1,
        third: 2,
        shelving: true,
        top: 182.0,
        right_drop: 0.0,
    },
];

fn band_colour(band: &Band) -> Color {
    match band.label {
        "HF" => th::eq_hf(),
        "HMF" => th::eq_hmf(),
        "LMF" => th::eq_lmf(),
        _ => th::eq_lf(),
    }
}

/// The track's first built-in EQ as a V250 faceplate, or a button adding
/// one in its place.
pub fn view_strip_eq(track: &ProjectTrack) -> Element<'_, Message> {
    let eq = track
        .effects
        .iter()
        .find(|e| e.effect_type == EffectType::Eq && e.plugin_ref.is_none());
    let Some(eq) = eq else {
        return container(pill(
            "+ EQ",
            None,
            Message::add_effect(track.id, EffectType::Eq),
        ))
        .center_x(Length::Fixed(EQ_W))
        .center_y(Length::Fixed(EQ_H))
        .into();
    };

    let mut layers: Vec<Element<'_, Message>> = vec![canvas(ZRules)
        .width(Length::Fixed(EQ_W))
        .height(Length::Fixed(EQ_H))
        .into()];
    for band in &BANDS {
        layers.extend(band_items(track.id, eq, band));
    }
    layers.push(place(
        RIGHT_X,
        122.0,
        RIGHT_W,
        pill(
            "EQ IN",
            (!eq.bypass).then(th::meter_green),
            Message::toggle_effect_bypass(track.id, eq.id),
        ),
    ));
    stack(layers)
        .width(Length::Fixed(EQ_W))
        .height(Length::Fixed(EQ_H))
        .into()
}

fn band_items<'a>(track_id: TrackId, eq: &'a UiEffect, band: &Band) -> Vec<Element<'a, Message>> {
    let colour = band_colour(band);
    let light = KnobCap {
        body: th::knob_cap_light(),
        pointer: th::cap_ink(),
    };
    let dark = KnobCap {
        body: th::knob_cap_dark(),
        pointer: th::text(),
    };
    let knob = |index: usize, cap: KnobCap| {
        let d = &eq.descriptors[index];
        EffectKnobWidget::new(
            track_id,
            eq.id,
            index,
            eq.params.get(index).copied().unwrap_or(d.default),
            d.min,
            d.max,
            d.default,
            colour,
        )
        .with_cap(cap)
    };
    let gain_cap = if band.shelving { light } else { dark };
    let unit = if band.freq >= 7 { "kHz" } else { "Hz" };
    let mut items = vec![place(
        0.0,
        band.top,
        GAIN_W,
        column![
            text(band.label)
                .font(PERFORM_LABEL)
                .size(9)
                .color(colour)
                .height(11),
            canvas(knob(band.gain, gain_cap).bipolar())
                .width(GAIN_KNOB)
                .height(GAIN_KNOB),
            caption(gain_range(&eq.descriptors[band.gain])),
        ]
        .align_x(iced::Alignment::Center)
        .into(),
    )];
    if band.shelving {
        let bell_on = eq.params.get(band.third).copied().unwrap_or(0.0) >= 0.5;
        let bell = pill(
            "BELL",
            bell_on.then_some(colour),
            Message::set_effect_param(track_id, eq.id, band.third, if bell_on { 0.0 } else { 1.0 }),
        );
        // HF's switch sits over its frequency knob, LF's under it.
        let (bell_top, freq_top) = if band.label == "HF" {
            (band.top + 4.0, band.top + 22.0)
        } else {
            (band.top + 40.0, band.top + 4.0)
        };
        items.push(place(RIGHT_X, bell_top, RIGHT_W, bell));
        items.push(place(
            RIGHT_X,
            freq_top,
            RIGHT_W,
            knob_with_caption(knob(band.freq, light), OUTER_FREQ_KNOB, unit),
        ));
    } else {
        let coloured = KnobCap {
            body: colour,
            pointer: th::cap_ink(),
        };
        items.push(place(
            RIGHT_X,
            band.top + band.right_drop,
            MID_FREQ_W,
            knob_with_caption(knob(band.freq, coloured), MID_FREQ_KNOB, unit),
        ));
        items.push(place(
            Q_X,
            band.top + band.right_drop + 2.0,
            Q_W,
            knob_with_caption(knob(band.third, dark), Q_KNOB, "Q"),
        ));
    }
    items
}

fn knob_with_caption<'a>(
    knob: EffectKnobWidget,
    size: f32,
    unit: &'static str,
) -> Element<'a, Message> {
    column![canvas(knob).width(size).height(size), caption(unit)]
        .align_x(iced::Alignment::Center)
        .into()
}

/// The gain scale printed under a gain knob, like a faceplate's.
fn gain_range(gain: &vibez_core::effect::ParamDescriptor) -> String {
    format!("{:.0} dB {:.0}", gain.min, gain.max)
}

fn caption<'a>(label: impl text::IntoFragment<'a>) -> Element<'a, Message> {
    text(label)
        .font(PERFORM_LABEL)
        .size(7)
        .color(th::text_dim())
        .wrapping(text::Wrapping::None)
        .into()
}

/// `content` centred in a column `width` wide whose top left corner is at
/// `x`, `y` in the block.
fn place<'a>(x: f32, y: f32, width: f32, content: Element<'a, Message>) -> Element<'a, Message> {
    container(container(content).center_x(Length::Fixed(width)))
        .padding(iced::Padding {
            top: y,
            left: x,
            ..iced::Padding::ZERO
        })
        .into()
}

/// A small rounded switch, tinted in `lit` while it is on.
pub fn pill<'a>(label: &'a str, lit: Option<Color>, on_press: Message) -> Element<'a, Message> {
    button(
        text(label)
            .font(PERFORM_LABEL)
            .size(7.5)
            .wrapping(text::Wrapping::None),
    )
    .on_press(on_press)
    .padding([2, 7])
    .style(move |_: &Theme, status| {
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        let (background, ink) = match lit {
            Some(colour) => (th::blend(th::bg_elevated(), colour, 0.22), colour),
            None if hovered => (th::bg_hover(), th::text()),
            None => (th::control(), th::text()),
        };
        button::Style {
            background: Some(background.into()),
            text_color: ink,
            border: iced::Border {
                radius: 9.0.into(),
                ..Default::default()
            },
            ..control_style::styled(None, status)
        }
    })
    .into()
}

/// The Z-shaped rules between the bands.
struct ZRules;

impl canvas::Program<Message> for ZRules {
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
        for (left, right) in Z_RULES {
            let rule = canvas::Path::new(|path| {
                path.move_to(Point::new(0.0, left + 0.5));
                path.line_to(Point::new(Z_STEP.0, left + 0.5));
                path.line_to(Point::new(Z_STEP.1, right + 0.5));
                path.line_to(Point::new(bounds.width, right + 0.5));
            });
            frame.stroke(
                &rule,
                canvas::Stroke::default()
                    .with_color(th::border_light())
                    .with_width(1.0),
            );
        }
        vec![frame.into_geometry()]
    }
}
