//! Chrome shared by every device card in the device chain.

use iced::widget::{button, column, container, row, text, Space};
use iced::{Element, Length, Theme};
use vibez_plugin_host::gui::PluginGuiKey;

use crate::icons;
use crate::message::Message;
use crate::theme as th;
use crate::widgets::control_style;

/// Card width for hosted CLAP and VST3 devices, instrument or effect.
pub const PLUGIN_CARD_W: f32 = 196.0;

/// The widths a native device lays itself out for.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CardWidths {
    pub compact: f32,
    pub medium: f32,
    pub expanded: f32,
    pub max: f32,
}

impl CardWidths {
    /// Compact until the medium layout fits, then everything offered up to
    /// the maximum.
    pub fn card_width(self, available: f32) -> f32 {
        if available >= self.medium {
            available.min(self.max)
        } else {
            self.compact
        }
    }

    /// Width in a device chain: the share the other cards leave, but never
    /// squeezed below the widest layout the section could show. A crowded
    /// chain scrolls instead.
    pub fn chain_width(self, share: f32, section: f32) -> f32 {
        if section >= self.expanded {
            self.card_width(share.max(self.expanded))
        } else if section >= self.medium {
            self.card_width(share.max(self.medium))
        } else {
            self.compact
        }
    }
}

/// Thin vertical rule separating sections inside a device body.
pub fn divider() -> Element<'static, Message> {
    container(text(""))
        .width(Length::Fixed(1.0))
        .height(Length::Fill)
        .style(|_theme: &Theme| container::Style {
            background: Some(th::divider().into()),
            ..Default::default()
        })
        .into()
}

/// Tiny uppercase header above a group of controls inside a device body.
pub fn section_label<'a>(label: impl ToString) -> Element<'a, Message> {
    text(label.to_string().to_uppercase())
        .size(8)
        .color(th::text_dim())
        .into()
}

/// Height of the header bar every device card opens with.
pub const HEADER_H: f32 = 34.0;
const HEADER_PAD_LEFT: f32 = 12.0;
const HEADER_PAD_RIGHT: f32 = 8.0;
const HEADER_GAP: f32 = 8.0;
const GROUP_GAP: f32 = 2.0;
const POWER_W: f32 = 14.0;
const NAME_SIZE: f32 = 12.0;
const NAME_FONT: iced::Font = iced::Font {
    weight: iced::font::Weight::Semibold,
    ..iced::Font::DEFAULT
};
/// The small filled buttons in a header, for actions and preset steps.
pub const TINY_W: f32 = 24.0;
const TINY_H: f32 = 22.0;
/// The narrowest a name gets before the preset selector gives way.
pub const NAME_MIN_W: f32 = 32.0;
pub const PRESET_MIN_W: f32 = 72.0;
const PRESET_MAX_W: f32 = 200.0;
/// Previous, next, load and save beside the preset selector.
pub const PRESET_BUTTONS: usize = 4;
/// Advanced, move up, move down and remove: the most a header carries.
pub const MOST_ACTIONS: usize = 4;
pub const CARD_RADIUS: f32 = 8.0;

/// What a header's widths come to: its name and its preset selector.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HeaderFit {
    pub name: f32,
    pub preset: f32,
}

const fn group_w(buttons: usize) -> f32 {
    buttons as f32 * TINY_W + buttons.saturating_sub(1) as f32 * GROUP_GAP
}

/// Everything in a header but its name and preset selector.
const fn header_fixed_w(preset: bool, actions: usize) -> f32 {
    let preset = if preset {
        HEADER_GAP + PRESET_BUTTONS as f32 * (GROUP_GAP + TINY_W)
    } else {
        0.0
    };
    HEADER_PAD_LEFT
        + POWER_W
        + HEADER_GAP
        + preset
        + HEADER_GAP
        + group_w(actions)
        + HEADER_PAD_RIGHT
}

/// The narrowest card a header with a preset and `actions` fits.
pub const fn header_min_w(actions: usize) -> f32 {
    header_fixed_w(true, actions) + NAME_MIN_W + PRESET_MIN_W
}

/// Splits a `card_w` wide header between a name that wants `name_w` and
/// the preset selector: the name keeps its width while the selector keeps
/// its minimum, and the selector takes what is left up to its maximum.
pub fn fit_header(card_w: f32, name_w: f32, preset: bool, actions: usize) -> HeaderFit {
    let room = (card_w - header_fixed_w(preset, actions)).max(0.0);
    if !preset {
        return HeaderFit {
            name: name_w.min(room),
            preset: 0.0,
        };
    }
    let name = name_w.min((room - PRESET_MIN_W).max(NAME_MIN_W));
    HeaderFit {
        name,
        preset: (room - name).clamp(0.0, PRESET_MAX_W),
    }
}

/// A small filled button at the end of a header, lit while `lit`.
pub struct Action {
    pub icon: char,
    pub help: &'static str,
    pub message: Message,
    pub lit: Option<iced::Color>,
}

impl Action {
    pub fn new(icon: char, help: &'static str, message: Message) -> Self {
        Self {
            icon,
            help,
            message,
            lit: None,
        }
    }
}

/// The header switch that shows or hides a device's Advanced controls.
pub fn advanced(open: bool, toggle: Message, accent: iced::Color) -> Action {
    Action {
        icon: icons::SLIDERS_VERTICAL,
        help: "Advanced",
        message: toggle,
        lit: open.then_some(accent),
    }
}

/// The header bar every device card shares: power, name, preset and
/// actions, one height for instruments, effects and plugins alike.
pub struct Header<'a> {
    pub name: &'a str,
    pub accent: iced::Color,
    pub on: bool,
    /// Switches the device off and on; an instrument has none and is
    /// always on.
    pub power: Option<Message>,
    /// Shown when the pointer rests on the name, such as the latency.
    pub hint: String,
    /// Builds the preset selector and its buttons for the selector width
    /// the header can spare.
    pub preset: Option<Box<dyn FnOnce(f32) -> Element<'a, Message> + 'a>>,
    pub actions: Vec<Action>,
}

impl<'a> Header<'a> {
    pub fn new(name: &'a str, accent: iced::Color, hint: String) -> Self {
        Self {
            name,
            accent,
            on: true,
            power: None,
            hint,
            preset: None,
            actions: Vec::new(),
        }
    }

    pub fn power(mut self, on: bool, toggle: Message) -> Self {
        self.on = on;
        self.power = Some(toggle);
        self
    }

    pub fn preset(mut self, preset: impl FnOnce(f32) -> Element<'a, Message> + 'a) -> Self {
        self.preset = Some(Box::new(preset));
        self
    }

    pub fn action(mut self, action: Action) -> Self {
        self.actions.push(action);
        self
    }

    fn view(self, card_w: f32) -> Element<'a, Message> {
        let on = self.on;
        // Rounded up, as the renderer lays the name out on whole pixels.
        let name_w = crate::widgets::device_controls::text_width(self.name, NAME_SIZE, NAME_FONT)
            .ceil()
            + 1.0;
        let fit = fit_header(card_w, name_w, self.preset.is_some(), self.actions.len());
        let name = text(crate::typography::fit(
            self.name, fit.name, NAME_SIZE, NAME_FONT,
        ))
        .size(NAME_SIZE)
        .font(NAME_FONT)
        .color(if on { th::text() } else { th::text_dim() })
        .shaping(iced::widget::text::Shaping::Advanced)
        .wrapping(iced::widget::text::Wrapping::None);
        let name = crate::widgets::hint::with_hint(
            container(name).width(Length::Fixed(fit.name)).clip(true),
            self.hint,
        );
        let mut bar = row![power(on, self.power, self.accent), name]
            .spacing(HEADER_GAP)
            .align_y(iced::Alignment::Center);
        if let Some(preset) = self.preset {
            bar = bar.push(preset(fit.preset));
        }
        let mut actions = row![].spacing(GROUP_GAP);
        for action in self.actions {
            actions = actions.push(tiny_button(
                action.icon,
                action.help,
                Some(action.message),
                action.lit,
                self.accent,
            ));
        }
        bar = bar.push(row![iced::widget::horizontal_space(), actions].width(Length::Fill));
        column![
            container(bar)
                .padding(iced::Padding {
                    top: 0.0,
                    right: HEADER_PAD_RIGHT,
                    bottom: 0.0,
                    left: HEADER_PAD_LEFT,
                })
                .width(Length::Fill)
                .height(Length::Fixed(HEADER_H))
                .align_y(iced::Alignment::Center),
            crate::widgets::device_controls::rule(),
        ]
        .into()
    }
}

/// The lit dot that is a card's power switch, ringed while on.
fn power<'a>(on: bool, toggle: Option<Message>, accent: iced::Color) -> Element<'a, Message> {
    let core = if on { accent } else { th::text_muted() };
    let dot = container(
        container(Space::new(8.0, 8.0)).style(move |_theme: &Theme| container::Style {
            background: Some(core.into()),
            border: iced::Border {
                radius: 4.0.into(),
                ..Default::default()
            },
            ..Default::default()
        }),
    )
    .center(Length::Fixed(POWER_W))
    .style(move |_theme: &Theme| container::Style {
        background: on.then(|| th::with_alpha(accent, 0.18).into()),
        border: iced::Border {
            radius: (POWER_W / 2.0).into(),
            ..Default::default()
        },
        ..Default::default()
    });
    match toggle {
        Some(toggle) => crate::widgets::hint::with_hint(
            button(dot)
                .on_press(toggle)
                .padding(0)
                .style(|_theme: &Theme, _status| button::Style::default()),
            if on { "Turn off" } else { "Turn on" },
        ),
        None => dot.into(),
    }
}

/// A small filled icon button for a header or a preset group, lit in
/// `lit` while on and tinted `accent` under the pointer.
pub fn tiny_button<'a>(
    icon: char,
    help: &'static str,
    message: Option<Message>,
    lit: Option<iced::Color>,
    accent: iced::Color,
) -> Element<'a, Message> {
    let control = button(
        container(icons::icon(icon).size(12))
            .center_x(Length::Fixed(TINY_W))
            .center_y(Length::Fixed(TINY_H)),
    )
    .on_press_maybe(message)
    .padding(0)
    .style(move |_theme: &Theme, status| {
        let style = control_style::styled(lit, status);
        button::Style {
            text_color: if lit.is_none()
                && matches!(status, button::Status::Hovered | button::Status::Pressed)
            {
                accent
            } else {
                style.text_color
            },
            border: iced::Border {
                radius: 6.0.into(),
                ..Default::default()
            },
            ..style
        }
    });
    crate::widgets::hint::with_hint(control, help)
}

/// A device card `width` wide: its header over `body`, which fills the
/// rest of the panel's height.
pub fn shell<'a>(
    header: Header<'a>,
    body: impl Into<Element<'a, Message>>,
    width: f32,
) -> Element<'a, Message> {
    container(
        column![
            header.view(width),
            container(body).width(Length::Fill).height(Length::Fill)
        ]
        .width(Length::Fixed(width))
        .height(Length::Fill),
    )
    .height(Length::Fill)
    .style(|_theme: &Theme| container::Style {
        background: Some(th::bg_elevated().into()),
        border: iced::Border {
            radius: CARD_RADIUS.into(),
            ..Default::default()
        },
        ..Default::default()
    })
    .into()
}

/// Body of a hosted plugin's card. The plugin draws its own controls, so
/// the card names the format, shows its latency and opens its editor.
pub fn plugin_face<'a>(
    format: Option<&str>,
    latency: String,
    gui_key: Option<PluginGuiKey>,
) -> Element<'a, Message> {
    let format = format.map_or_else(|| "Plugin".to_string(), |format| format.to_uppercase());
    container(
        column![
            section_label(format!("{format} plugin")),
            text(latency).size(9).color(th::text_dim()),
            editor_button(gui_key),
        ]
        .spacing(6),
    )
    .padding([8, 10])
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

fn editor_button<'a>(gui_key: Option<PluginGuiKey>) -> Element<'a, Message> {
    let has_editor = gui_key.is_some();
    let label = if has_editor {
        "Open editor"
    } else {
        "No editor"
    };
    button(
        row![
            icons::icon(icons::SLIDERS_VERTICAL).size(11),
            text(label).size(10),
        ]
        .spacing(6)
        .align_y(iced::Alignment::Center),
    )
    .on_press_maybe(gui_key.map(Message::OpenPluginGui))
    .width(Length::Fill)
    .padding([6, 8])
    .style(control_style::style(None))
    .into()
}

/// The room a chain that fits leaves under its cards.
pub const CHAIN_PAD_BELOW: f32 = 12.0;

/// The device chain's scrollbar, and the gap it keeps from the cards: the
/// two fill the room a chain that fits leaves under its cards.
pub const CHAIN_BAR_W: f32 = super::quiet_scroll::THICKNESS;
pub const CHAIN_BAR_GAP: f32 = CHAIN_PAD_BELOW - CHAIN_BAR_W;

/// The room a chain `chain_width` wide keeps beneath its cards for the
/// scrollbar in a section `available` wide. Only one that overflows shows
/// the bar, in the room under its cards, so its cards stay as tall as a
/// chain's that fits and the bar never covers them.
pub fn chain_bar_room(chain_width: f32, available: f32) -> f32 {
    if chain_width > available + 0.5 {
        CHAIN_BAR_W + CHAIN_BAR_GAP
    } else {
        0.0
    }
}

/// Card for a hosted CLAP or VST3 instrument, matching hosted effects.
pub fn plugin_instrument_card<'a>(
    track_id: vibez_core::id::TrackId,
    name: &'a str,
    format: Option<&str>,
    has_editor: bool,
    latency: String,
    track_color: iced::Color,
) -> Element<'a, Message> {
    let header = Header::new(name, track_color, latency.clone()).action(Action::new(
        icons::X,
        "Remove instrument",
        Message::remove_track_instrument(track_id),
    ));
    shell(
        header,
        plugin_face(
            format,
            latency,
            has_editor.then_some(PluginGuiKey::Instrument { track_id }),
        ),
        PLUGIN_CARD_W,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const WIDTHS: CardWidths = CardWidths {
        compact: 340.0,
        medium: 520.0,
        expanded: 760.0,
        max: 1000.0,
    };

    #[test]
    fn at_its_narrowest_a_header_keeps_the_name_and_preset_minimums() {
        let fit = fit_header(header_min_w(MOST_ACTIONS), 500.0, true, MOST_ACTIONS);
        assert_eq!((fit.name, fit.preset), (NAME_MIN_W, PRESET_MIN_W));
    }

    #[test]
    fn a_long_name_gives_way_until_the_preset_selector_is_at_its_minimum() {
        let fit = fit_header(header_min_w(MOST_ACTIONS) + 40.0, 500.0, true, MOST_ACTIONS);
        assert_eq!((fit.name, fit.preset), (NAME_MIN_W + 40.0, PRESET_MIN_W));
    }

    #[test]
    fn a_short_name_leaves_the_preset_selector_room_up_to_its_maximum() {
        let fit = fit_header(1000.0, 40.0, true, MOST_ACTIONS);
        assert_eq!((fit.name, fit.preset), (40.0, PRESET_MAX_W));
    }

    #[test]
    fn a_header_without_presets_gives_its_name_the_rest() {
        let narrow = fit_header(PLUGIN_CARD_W, 500.0, false, 3);
        let wider = fit_header(PLUGIN_CARD_W + 30.0, 500.0, false, 3);
        assert_eq!(wider.name - narrow.name, 30.0);
        assert_eq!(fit_header(PLUGIN_CARD_W, 40.0, false, 3).name, 40.0);
    }

    #[test]
    fn a_scrolling_chain_keeps_its_cards_as_tall_as_one_that_fits() {
        assert_eq!(chain_bar_room(1200.0, 1000.0), CHAIN_PAD_BELOW);
    }

    #[test]
    fn only_an_overflowing_chain_keeps_room_for_its_scrollbar() {
        assert_eq!(chain_bar_room(900.0, 1000.0), 0.0);
        assert_eq!(chain_bar_room(1000.0, 1000.0), 0.0);
        assert!(chain_bar_room(1200.0, 1000.0) > CHAIN_BAR_W);
    }

    #[test]
    fn a_crowded_chain_keeps_each_device_expanded_and_scrolls() {
        assert_eq!(
            WIDTHS.chain_width(120.0, 1600.0),
            760.0,
            "never squeezed below expanded"
        );
        assert_eq!(
            WIDTHS.chain_width(900.0, 1600.0),
            900.0,
            "takes a larger share"
        );
        assert_eq!(
            WIDTHS.chain_width(5000.0, 6000.0),
            1000.0,
            "up to its maximum"
        );
    }

    #[test]
    fn a_section_too_narrow_for_expanded_uses_medium() {
        assert_eq!(WIDTHS.chain_width(120.0, 600.0), 520.0);
    }

    #[test]
    fn a_section_too_narrow_for_medium_uses_compact() {
        assert_eq!(WIDTHS.chain_width(120.0, 400.0), 340.0);
    }
}
