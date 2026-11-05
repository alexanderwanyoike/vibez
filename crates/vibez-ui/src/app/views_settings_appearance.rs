//! Appearance settings: theme selection, colour strength and interface scale.

use iced::widget::{button, column, container, row, stack, text, text_input, Space};
use iced::{Element, Length, Theme};

use crate::message::Message;
use crate::theme as th;
use crate::ui_settings::{INTERFACE_SCALE_MAX, INTERFACE_SCALE_MIN};

use super::views_settings_rows::{action, group, hint, label_block, setting_row, slider_control};
use super::*;

const TILE_COLUMNS: usize = 5;
const TILE_H: f32 = 74.0;
const TILE_RADIUS: f32 = 8.0;
/// Each preview bar's lead-in and length, in parts of the tile's main panel.
const TILE_BARS: [(u16, u16); 3] = [(0, 78), (20, 54), (8, 66)];

/// A preview of `palette`: its frame, side and main panels, three of its
/// track colours and its accent, ringed in the accent while it is in use.
fn theme_tile(palette: &th::ThemePalette, selected: bool) -> Element<'static, Message> {
    let panel = palette.bg_surface;
    let rounded = |colour: iced::Color, radius: f32| {
        move |_theme: &Theme| container::Style {
            background: Some(colour.into()),
            border: iced::Border {
                radius: radius.into(),
                ..Default::default()
            },
            ..Default::default()
        }
    };
    let mut bars = column![].spacing(5);
    for ((lead, length), index) in TILE_BARS.into_iter().zip([0, 2, 5]) {
        let colour = th::colour_strength().track(palette.track_colors[index]);
        bars = bars.push(row![
            Space::with_width(Length::FillPortion(lead.max(1))).height(9),
            container(Space::new(Length::Fill, 9.0))
                .width(Length::FillPortion(length))
                .style(rounded(colour, 2.0)),
            Space::with_width(Length::FillPortion(100 - lead - length)),
        ]);
    }
    let preview = row![
        container(Space::new(Length::Fill, Length::Fill))
            .width(Length::FillPortion(22))
            .height(Length::Fill)
            .style(rounded(panel, 4.0)),
        container(bars)
            .padding(6)
            .width(Length::FillPortion(78))
            .height(Length::Fill)
            .style(rounded(panel, 4.0)),
    ]
    .spacing(4);
    let dot = container(Space::new(7.0, 7.0)).style(rounded(palette.accent, 3.5));
    let frame = palette.display_bg;
    let tile = container(stack![
        preview,
        container(dot)
            .width(Length::Fill)
            .align_x(iced::alignment::Horizontal::Right)
            .padding([3, 3]),
    ])
    .padding(6)
    .width(Length::Fill)
    .height(TILE_H)
    .style(move |_theme: &Theme| container::Style {
        background: Some(frame.into()),
        border: iced::Border {
            color: if selected {
                th::accent()
            } else {
                th::hairline(th::bg_elevated())
            },
            width: if selected { 2.0 } else { 1.0 },
            radius: TILE_RADIUS.into(),
        },
        ..Default::default()
    });
    let name = palette.name.clone();
    button(
        column![
            tile,
            text(name.clone())
                .size(11.5)
                .font(iced::Font {
                    weight: if selected {
                        iced::font::Weight::Semibold
                    } else {
                        iced::font::Weight::Medium
                    },
                    ..iced::Font::DEFAULT
                })
                .color(if selected { th::text() } else { th::text_dim() })
                .wrapping(iced::widget::text::Wrapping::None),
        ]
        .spacing(6),
    )
    .on_press(Message::SelectTheme(name))
    .padding(0)
    .width(Length::FillPortion(1))
    .style(|_theme: &Theme, _status| button::Style::default())
    .into()
}

impl App {
    /// `palettes` as rows of tiles.
    fn theme_grid<'p>(
        &self,
        palettes: impl IntoIterator<Item = &'p th::ThemePalette>,
    ) -> Element<'static, Message> {
        let palettes: Vec<&th::ThemePalette> = palettes.into_iter().collect();
        let mut grid = column![].spacing(12);
        for chunk in palettes.chunks(TILE_COLUMNS) {
            let mut tiles = row![].spacing(10);
            for palette in chunk {
                tiles = tiles.push(theme_tile(
                    palette,
                    self.state.current_theme_name == palette.name,
                ));
            }
            for _ in chunk.len()..TILE_COLUMNS {
                tiles = tiles.push(Space::with_width(Length::FillPortion(1)));
            }
            grid = grid.push(tiles);
        }
        grid.into()
    }

    pub(super) fn view_settings_appearance_tab(&self) -> Element<'_, Message> {
        let builtins = crate::themes::builtins();
        let themes = group([column![
            label_block(
                "Theme",
                Some(hint("Recolours the whole interface, track colours included.").into()),
            ),
            self.theme_grid(builtins.iter()),
        ]
        .spacing(14)
        .into()]);

        let user_palettes: Element<'_, Message> = if self.state.user_themes.is_empty() {
            hint("No .vzt themes yet. Save the current look below, or drop theme files in the folder.")
                .into()
        } else {
            self.theme_grid(self.state.user_themes.iter().map(|user| &user.palette))
        };
        let your_themes = group([
            setting_row(
                "Your themes",
                Some(hint(crate::themes::themes_dir().display().to_string()).into()),
                action("Rescan", Some(Message::RescanThemes), false),
            ),
            user_palettes,
            row![
                text_input("Theme name", &self.state.theme_save_name)
                    .on_input(Message::ThemeSaveNameChanged)
                    .on_submit(Message::SaveCurrentTheme)
                    .size(11.5)
                    .padding([6, 10])
                    .style(super::views_browser_style::browser_search_style),
                action(
                    "Save current as theme",
                    Some(Message::SaveCurrentTheme),
                    true
                ),
            ]
            .spacing(8)
            .align_y(iced::Alignment::Center)
            .into(),
        ]);

        // The readout is a percentage because that is how the setting is
        // named; the slider works in the multiplier iced consumes.
        let scale = self.interface_scale;
        let look = group([
            setting_row(
                "Colour strength",
                Some(
                    hint("How strongly track colours show on clips, headers, the mixer and Perform.")
                        .into(),
                ),
                crate::widgets::device_controls::segmented(
                    &[
                        (th::ColourStrength::Vivid, "Vivid"),
                        (th::ColourStrength::Medium, "Medium"),
                        (th::ColourStrength::Soft, "Soft"),
                    ],
                    th::colour_strength(),
                    Message::SetColourStrength,
                    th::accent(),
                ),
            ),
            setting_row(
                "Interface scale",
                Some(
                    hint("Resizes every panel, control and font together. Timeline zoom keeps its own setting.")
                        .into(),
                ),
                slider_control(
                    INTERFACE_SCALE_MIN..=INTERFACE_SCALE_MAX,
                    scale,
                    0.05,
                    Message::SetInterfaceScale,
                    format!("{}%", (scale * 100.0).round() as i32),
                ),
            ),
        ]);

        column![themes, look, your_themes].spacing(16).into()
    }
}
