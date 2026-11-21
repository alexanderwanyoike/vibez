//! The clip launcher's column beside its last track, where a drop adds a
//! track with its Clip in the hovered slot.

use iced::widget::{canvas, container, mouse_area, text};
use iced::{Element, Length};

use crate::domains::browser::BrowserMsg;
use crate::message::Message;
use crate::theme as th;
use crate::typography::PERFORM_LABEL;

use super::App;

/// `content` as the place a drop adds a track with its Clip in `row`.
/// Moving re-reports the slot, so leaving one slot for the next never
/// clears both.
fn new_track_slot_target<'a>(
    content: impl Into<Element<'a, Message>>,
    row: u32,
) -> Element<'a, Message> {
    super::desktop_pointer::drop_target(
        mouse_area(content)
            .on_move(move |_| Message::Browser(BrowserMsg::DragHoverNewTrackSlot { row }))
            .on_exit(Message::Browser(BrowserMsg::LeaveDragTarget(
                crate::state::BrowserDropTarget::NewTrackSlot { row },
            )))
            .on_release(Message::ImportLauncherClipOnNewTrack { row }),
        crate::domains::desktop_pointer::PointerTarget::NewTrackSlot { row },
    )
}

/// The new-track column's slot for `row`, `width` wide, with the empty
/// space right of it taking its drops too.
fn new_track_slot<'a>(
    content: impl Into<Element<'a, Message>>,
    width: f32,
    row: u32,
) -> Element<'a, Message> {
    new_track_slot_target(
        iced::widget::row![
            container(content).width(Length::Fixed(width)),
            iced::widget::horizontal_space(),
        ],
        row,
    )
}

impl App {
    /// The column beside the last track where a drop adds a track, the
    /// hovered slot showing the ghost.
    pub(super) fn view_new_track_column(
        &self,
        first_row: u32,
        header_height: f32,
        slot_height: f32,
        lane_width: f32,
        compact: bool,
    ) -> Element<'_, Message> {
        let ghost = self.state.browser.drop_ghost(self.state.transport.bpm);
        let label = self
            .state
            .browser
            .new_track_kind()
            .map_or("", crate::domains::drop_ghost::NewTrack::label);
        let ghost_row = match ghost.map(|ghost| ghost.place) {
            Some(crate::domains::drop_ghost::GhostPlace::NewTrackSlot { row }) => Some(row),
            _ => None,
        };
        // The header holds no slot, so a drop there takes the first row.
        let mut column = iced::widget::Column::new().push(new_track_slot(
            container(
                text(label)
                    .font(PERFORM_LABEL)
                    .size(11)
                    .color(if ghost_row.is_some() {
                        th::text()
                    } else {
                        th::text_dim()
                    }),
            )
            .padding([3.0, crate::widgets::clip_slot::TILE_GAP / 2.0])
            .width(Length::Fill)
            .height(header_height),
            lane_width,
            0,
        ));
        for offset in 0..super::views_clip_launcher::LAUNCHER_ROWS {
            let row = first_row + offset;
            let cell: Element<'_, Message> = match self.state.browser.drop_ghost_slot() {
                Some(slot) if ghost_row == Some(row) => {
                    canvas(crate::widgets::clip_slot::ClipSlot {
                        clip: Some(slot),
                        cell: (slot.track_id, row),
                        drag_source: None,
                        empty_action: Message::ImportLauncherClipOnNewTrack { row },
                        color: th::accent(),
                        key: "",
                        selected: false,
                        recording: false,
                        audio_recording: None,
                        progress: None,
                        playing: false,
                        queued: false,
                        compact,
                    })
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .into()
                }
                _ => iced::widget::Space::new(Length::Fill, Length::Fill).into(),
            };
            column = column.push(new_track_slot(
                container(cell).width(Length::Fill).height(slot_height),
                lane_width,
                row,
            ));
        }
        column.width(Length::Fill).into()
    }
}

/// The space under the launcher's rows, where a drop takes the slot of the
/// nearest row shown, the last.
pub(super) fn below_new_track_column<'a>(first_row: u32) -> Element<'a, Message> {
    new_track_slot_target(
        iced::widget::Space::new(Length::Fill, Length::Fill),
        first_row + super::views_clip_launcher::LAUNCHER_ROWS - 1,
    )
}
