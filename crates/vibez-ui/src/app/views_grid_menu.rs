//! The grid section of the Arrange and Section timelines' right-click menu.

use iced::widget::{column, container, row, text, Row};
use iced::{Element, Length, Size, Theme};

use crate::domains::view::{GridTarget, ViewMsg};
use crate::message::{MenuOverlay, Message};
use crate::state::GridCommand;
use crate::theme as th;
use crate::typography;

use super::views_control_bar_style::segment_button;
use super::App;

/// The timeline right-click menus' width, wide enough that a quarter-row
/// grid chip holds a triplet bar label like "8 Bars T".
pub(super) const TIMELINE_MENU_WIDTH: f32 = 220.0;
const CHIP_GAP: f32 = 2.0;
const CHIP_HEIGHT: f32 = 24.0;
const CHIPS_PER_ROW: usize = 4;

impl App {
    pub(super) fn view_grid_menu(&self) -> Element<'_, Message> {
        let grid = self.state.view.arrange_grid;
        let pixels_per_beat = self.timeline_pixels_per_beat();
        let current = grid.choice(pixels_per_beat);
        let auto = grid.auto_choice(pixels_per_beat);
        let width =
            (TIMELINE_MENU_WIDTH - (CHIPS_PER_ROW - 1) as f32 * CHIP_GAP) / CHIPS_PER_ROW as f32;
        let chip = |label: String, span: f32, lit: bool, command: GridCommand| {
            segment_button(
                text(label).size(11),
                Size::new(width * span + CHIP_GAP * (span - 1.0), CHIP_HEIGHT),
                lit.then(th::accent),
                Message::menu_item(
                    MenuOverlay::ArrangementContext,
                    Message::View(ViewMsg::Grid(GridTarget::Arrange, command)),
                ),
            )
        };

        let mut section = column![
            container(
                container("")
                    .width(Length::Fill)
                    .height(1)
                    .style(|_theme: &Theme| container::Style {
                        background: Some(th::border().into()),
                        ..Default::default()
                    })
            )
            .padding([4, 0]),
            container(
                text("GRID")
                    .font(typography::PERFORM_TECH)
                    .size(10)
                    .color(th::text_dim())
            )
            .padding([2, 4]),
            row![
                chip(
                    auto.to_string(),
                    2.0,
                    grid.adaptive,
                    GridCommand::Choose(auto)
                ),
                chip(
                    "Triplet".into(),
                    1.0,
                    grid.selected.is_triplet(),
                    GridCommand::ToggleTriplet
                ),
                chip(
                    "Snap".into(),
                    1.0,
                    grid.snap_enabled,
                    GridCommand::ToggleSnap
                ),
            ]
            .spacing(CHIP_GAP),
        ]
        .spacing(CHIP_GAP)
        .width(Length::Fixed(TIMELINE_MENU_WIDTH));

        let fixed: Vec<_> = grid.fixed_choices().collect();
        for choices in fixed.chunks(CHIPS_PER_ROW) {
            section = section.push(
                Row::with_children(choices.iter().map(|&choice| {
                    chip(
                        choice.to_string(),
                        1.0,
                        choice == current,
                        GridCommand::Choose(choice),
                    )
                }))
                .spacing(CHIP_GAP),
            );
        }
        section.into()
    }
}
