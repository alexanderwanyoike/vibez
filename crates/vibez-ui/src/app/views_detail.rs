//! Split out of app.rs; inherent methods on [`super::App`].

use iced::widget::{
    button, canvas, center, column, container, horizontal_space, pick_list, row, text,
};
use iced::{Color, Element, Length, Theme};

use crate::domains::piano_roll::PianoRollMsg;
use crate::domains::view::ViewMsg;
use vibez_core::id::{ClipId, SectionId, TrackId};

use crate::icons;
use crate::message::Message;
use crate::state::{ArrangementSelection, DetailPanelTab, TimelineEditorState};
use crate::theme as th;
use crate::widgets::piano_roll::{PianoRollWidget, VelocityLaneWidget};

use super::*;

const DETAIL_PANEL_MIN_HEIGHT: f32 = 180.0;
const AUDIO_DETAIL_PANEL_MIN_HEIGHT: f32 = 260.0;
const MIDI_DETAIL_PANEL_MIN_HEIGHT: f32 = 360.0;
const SHELL_AND_WORKSPACE_MIN_HEIGHT: f32 = 480.0;
const DETAIL_PANEL_MAX_WINDOW_FRACTION: f32 = 0.52;
const STATUS_BAR_HEIGHT: f32 = 24.0;

pub(super) fn resolved_detail_playhead_samples(
    editing_perform: bool,
    selected_section: Option<SectionId>,
    playing_section: Option<SectionId>,
    arrange_samples: u64,
    section_samples: u64,
) -> Option<u64> {
    if !editing_perform {
        return Some(arrange_samples);
    }
    selected_section
        .filter(|selected| Some(*selected) == playing_section)
        .map(|_| section_samples)
}

fn effective_detail_panel_height(
    preferred_height: f32,
    window_height: f32,
    editor_min_height: f32,
) -> f32 {
    let maximum_by_workspace = window_height - SHELL_AND_WORKSPACE_MIN_HEIGHT;
    let maximum_by_fraction = window_height * DETAIL_PANEL_MAX_WINDOW_FRACTION;
    let maximum = maximum_by_workspace
        .min(maximum_by_fraction)
        .max(DETAIL_PANEL_MIN_HEIGHT);
    let preferred_height = preferred_height.max(editor_min_height);
    preferred_height.clamp(DETAIL_PANEL_MIN_HEIGHT, maximum)
}

fn visible_note_clip_for_track(editor: &TimelineEditorState, track_id: TrackId) -> Option<ClipId> {
    editor
        .selected_note_clip
        .filter(|(selected_track, _)| *selected_track == track_id)
        .map(|(_, clip_id)| clip_id)
        .or_else(|| {
            editor.selected_clips.iter().find_map(|selection| {
                if let ArrangementSelection::NoteClip {
                    track_id: selected_track,
                    clip_id,
                } = selection
                {
                    (*selected_track == track_id).then_some(*clip_id)
                } else {
                    None
                }
            })
        })
}

fn focused_note_clip_for_track(editor: &TimelineEditorState, track_id: TrackId) -> Option<ClipId> {
    editor
        .selected_note_clip
        .filter(|(selected_track, _)| *selected_track == track_id)
        .map(|(_, clip_id)| clip_id)
}

fn single_selected_audio_clip_for_track(
    editor: &TimelineEditorState,
    track_id: TrackId,
) -> Option<ClipId> {
    let mut selections = editor.selected_clips.iter();
    match (selections.next(), selections.next()) {
        (
            Some(ArrangementSelection::AudioClip {
                track_id: selected_track,
                clip_id,
            }),
            None,
        ) if *selected_track == track_id => Some(*clip_id),
        _ => None,
    }
}

impl App {
    // ── Detail panel (Ableton-style device chain) ──

    /// The selected MIDI track that can host the piano-roll detail view.
    ///
    /// Visibility and keyboard focus intentionally share this structural
    /// gate, then resolve their clip from different selection state.
    fn piano_roll_track(&self) -> Option<TrackId> {
        if self.state.view.detail_panel_tab != DetailPanelTab::Clip {
            return None;
        }

        let editor = self.state.active_timeline_editor();
        let track_id = editor.selected_track?;
        self.state
            .find_track(track_id)
            .is_some_and(|track| track.kind.is_midi())
            .then_some(track_id)
    }

    /// A note clip the piano roll can render. Arrangement selection may
    /// supply the clip without transferring keyboard focus into the editor.
    pub(super) fn visible_piano_roll_clip(&self) -> Option<(TrackId, ClipId)> {
        let track_id = self.piano_roll_track()?;
        let clip_id = visible_note_clip_for_track(self.state.active_timeline_editor(), track_id)?;
        Some((track_id, clip_id))
    }

    /// The explicitly opened note clip that owns editor shortcuts.
    pub(super) fn focused_piano_roll_clip(&self) -> Option<(TrackId, ClipId)> {
        let track_id = self.piano_roll_track()?;
        let clip_id = focused_note_clip_for_track(self.state.active_timeline_editor(), track_id)?;
        Some((track_id, clip_id))
    }

    fn midi_clip_editor_visible(&self) -> bool {
        self.visible_piano_roll_clip().is_some()
    }

    fn detail_editor_min_height(&self) -> f32 {
        if self.midi_clip_editor_visible() {
            return MIDI_DETAIL_PANEL_MIN_HEIGHT;
        }
        let editor = self.state.active_timeline_editor();
        let audio_selected = editor.selected_track.is_some_and(|track_id| {
            single_selected_audio_clip_for_track(editor, track_id).is_some()
        });
        if audio_selected {
            AUDIO_DETAIL_PANEL_MIN_HEIGHT
        } else {
            DETAIL_PANEL_MIN_HEIGHT
        }
    }

    pub(super) fn view_detail_panel(&self) -> Element<'_, Message> {
        let detail_content: Element<'_, Message> = if let Some(track) = self
            .state
            .active_timeline_editor()
            .selected_track
            .and_then(|id| self.state.find_track(id))
        {
            let track_id = track.id;
            let track_color = th::track_color(track.color_index);

            // Tab bar
            let clip_tab = {
                let active = self.state.view.detail_panel_tab == DetailPanelTab::Clip;
                let (bg, text_color, border_color) = if active {
                    (th::bg_elevated(), th::accent(), th::accent_dim())
                } else {
                    (
                        iced::Color::TRANSPARENT,
                        th::text_dim(),
                        iced::Color::TRANSPARENT,
                    )
                };
                button(text("Clip").size(12).color(text_color))
                    .on_press(Message::View(ViewMsg::SwitchDetailTab(
                        DetailPanelTab::Clip,
                    )))
                    .padding([4, 12])
                    .style(move |_theme: &Theme, _status| button::Style {
                        background: Some(bg.into()),
                        text_color,
                        border: iced::Border {
                            color: border_color,
                            width: if active { 1.0 } else { 0.0 },
                            radius: 4.0.into(),
                        },
                        ..Default::default()
                    })
            };
            let devices_tab = {
                let active = self.state.view.detail_panel_tab == DetailPanelTab::Devices;
                let (bg, text_color, border_color) = if active {
                    (th::bg_elevated(), th::accent(), th::accent_dim())
                } else {
                    (
                        iced::Color::TRANSPARENT,
                        th::text_dim(),
                        iced::Color::TRANSPARENT,
                    )
                };
                button(text("Devices").size(12).color(text_color))
                    .on_press(Message::View(ViewMsg::SwitchDetailTab(
                        DetailPanelTab::Devices,
                    )))
                    .padding([4, 12])
                    .style(move |_theme: &Theme, _status| button::Style {
                        background: Some(bg.into()),
                        text_color,
                        border: iced::Border {
                            color: border_color,
                            width: if active { 1.0 } else { 0.0 },
                            radius: 4.0.into(),
                        },
                        ..Default::default()
                    })
            };
            let tab_bar = row![clip_tab, devices_tab].spacing(4).padding([4, 8]);

            // Tab content
            let tab_content: Element<'_, Message> = match self.state.view.detail_panel_tab {
                DetailPanelTab::Clip => {
                    let is_midi = track.kind.is_midi();

                    if self.visible_piano_roll_clip().is_some() {
                        self.view_piano_roll_panel(track_id, track_color)
                    } else if is_midi {
                        self.view_midi_track_clip_placeholder(track_id, track_color)
                    } else {
                        // Find a single selected audio clip on this track
                        let audio_sel = single_selected_audio_clip_for_track(
                            self.state.active_timeline_editor(),
                            track_id,
                        );
                        if let Some(sel_cid) = audio_sel {
                            if let Some(clip) = self
                                .state
                                .active_timeline_content(track_id)
                                .and_then(|content| content.clips.iter().find(|c| c.id == sel_cid))
                            {
                                self.view_audio_clip_panel(track_id, clip, track_color)
                            } else {
                                self.view_clip_placeholder()
                            }
                        } else {
                            self.view_clip_placeholder()
                        }
                    }
                }
                DetailPanelTab::Devices => self.view_device_chain(track_id, track, track_color),
            };

            column![tab_bar, tab_content].height(Length::Fill).into()
        } else {
            let label = text("Select a track to view devices")
                .size(14)
                .color(th::text_dim());
            center(label)
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
        };

        let panel_height = effective_detail_panel_height(
            self.state.view.detail_panel_height,
            self.state.view.window_height,
            self.detail_editor_min_height(),
        );
        container(detail_content)
            .width(Length::Fill)
            .height(Length::Fixed(panel_height))
            .style(|_theme: &Theme| container::Style {
                background: Some(th::bg_dark().into()),
                border: iced::Border {
                    color: th::border(),
                    width: 1.0,
                    radius: 0.0.into(),
                },
                ..Default::default()
            })
            .into()
    }

    pub(super) fn detail_panel_drag_height(&self, cursor_y: f32) -> f32 {
        effective_detail_panel_height(
            self.state.view.window_height - cursor_y - STATUS_BAR_HEIGHT,
            self.state.view.window_height,
            self.detail_editor_min_height(),
        )
    }

    pub(super) fn view_clip_placeholder(&self) -> Element<'_, Message> {
        let label = text("Select a clip to view details")
            .size(14)
            .color(th::text_dim());
        center(label)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    /// Piano roll panel for the detail panel split view.
    pub(super) fn view_piano_roll_panel(
        &self,
        track_id: TrackId,
        track_color: Color,
    ) -> Element<'_, Message> {
        use crate::state::PianoRollEditMode;

        let playhead_beats = resolved_detail_playhead_samples(
            self.state.view.workspace == crate::state::Workspace::Perform,
            self.state.perform.selected_section,
            self.state.perform.playing_section,
            self.state.transport.position_samples,
            self.state.perform.section_playhead_samples,
        )
        .map(|samples| {
            samples as f64 * self.state.transport.bpm
                / (f64::from(self.state.transport.sample_rate.max(1)) * 60.0)
        })
        .unwrap_or(-1.0);

        let visible_clip = self
            .visible_piano_roll_clip()
            .filter(|(open_track_id, _)| *open_track_id == track_id)
            .and_then(|(_, clip_id)| {
                let content = self.state.active_timeline_content(track_id)?;
                content.note_clips.iter().find(|clip| clip.id == clip_id)
            });

        let (piano_widget, velocity_widget) = match visible_clip {
            Some(clip) => {
                let clip_relative_playhead = playhead_beats - clip.position_beats;
                (
                    PianoRollWidget::from_clip(
                        track_id,
                        clip,
                        clip_relative_playhead,
                        clip.duration_beats,
                        track_color,
                        self.state.view.grid_config(),
                        self.state.piano_roll.scroll_y,
                        self.state.piano_roll.edit_mode,
                    ),
                    VelocityLaneWidget::from_clip(
                        track_id,
                        clip,
                        clip.duration_beats,
                        track_color,
                        self.state.view.grid_config(),
                    ),
                )
            }
            None => (
                PianoRollWidget::empty(track_id, playhead_beats, track_color),
                VelocityLaneWidget::empty(track_id, track_color),
            ),
        };

        let piano_canvas: Element<'_, Message> = canvas(piano_widget)
            .width(Length::Fill)
            .height(Length::Fill)
            .into();
        let velocity_canvas: Element<'_, Message> = canvas(velocity_widget)
            .width(Length::Fill)
            .height(Length::Fixed(VelocityLaneWidget::HEIGHT))
            .into();

        // ── Clip properties bar (shown when a clip is selected) ──
        let mut content_col = column![].spacing(2).padding(4);

        if let Some(clip) = visible_clip {
            let clip_id = clip.id;
            let clip_loop = clip.loop_enabled;
            let groove_grid = clip.groove_grid;
            let clip_name = text(clip.name.clone()).size(11).color(th::text());
            let pos_label = text(format!("Pos: {:.1}", clip.position_beats))
                .size(10)
                .color(th::text_dim());
            let dur_label = text(format!("Dur: {:.1}", clip.duration_beats))
                .size(10)
                .color(th::text_dim());

            let swing_relationship = self.view_clip_swing_relationship(
                track_id,
                track_color,
                Some((clip_id, groove_grid)),
            );

            // Loop toggle
            let loop_icon_color = if clip_loop {
                th::accent()
            } else {
                th::text_dim()
            };
            let loop_btn = button(icons::icon(icons::REPEAT).size(10).color(loop_icon_color))
                .on_press(Message::PianoRoll(PianoRollMsg::ToggleNoteClipLoop(
                    track_id, clip_id,
                )))
                .padding([2, 4])
                .style(move |_theme: &Theme, _status| button::Style {
                    background: if clip_loop {
                        Some(th::accent_dim().into())
                    } else {
                        Some(th::bg_elevated().into())
                    },
                    text_color: loop_icon_color,
                    border: iced::Border {
                        color: if clip_loop {
                            th::accent_dim()
                        } else {
                            th::border()
                        },
                        width: 1.0,
                        radius: 3.0.into(),
                    },
                    ..Default::default()
                });

            // Clip operation buttons
            let op_btn_style = |_theme: &Theme, _status| button::Style {
                background: Some(th::bg_elevated().into()),
                text_color: th::text_dim(),
                border: iced::Border {
                    color: th::border(),
                    width: 1.0,
                    radius: 3.0.into(),
                },
                ..Default::default()
            };

            let dup_btn = button(
                row![
                    icons::icon(icons::COPY).size(10).color(th::text_dim()),
                    text("Dup").size(10).color(th::text_dim())
                ]
                .spacing(2)
                .align_y(iced::Alignment::Center),
            )
            .on_press(Message::duplicate_note_clip(track_id, clip_id))
            .padding([2, 6])
            .style(op_btn_style);

            let double_btn = button(text("2x").size(10).color(th::text_dim()))
                .on_press(Message::PianoRoll(PianoRollMsg::DoubleNoteClip(
                    track_id, clip_id,
                )))
                .padding([2, 6])
                .style(op_btn_style);

            let halve_btn = button(text("\u{00BD}x").size(10).color(th::text_dim()))
                .on_press(Message::PianoRoll(PianoRollMsg::HalveNoteClip(
                    track_id, clip_id,
                )))
                .padding([2, 6])
                .style(op_btn_style);

            let crop_btn = button(
                row![
                    icons::icon(icons::SCISSORS).size(10).color(th::text_dim()),
                    text("Crop").size(10).color(th::text_dim())
                ]
                .spacing(2)
                .align_y(iced::Alignment::Center),
            )
            .on_press(Message::PianoRoll(PianoRollMsg::CropNoteClip(
                track_id, clip_id,
            )))
            .padding([2, 6])
            .style(op_btn_style);

            let props_row = row![
                clip_name,
                swing_relationship,
                horizontal_space(),
                pos_label,
                dur_label,
                loop_btn,
                dup_btn,
                double_btn,
                halve_btn,
                crop_btn,
            ]
            .spacing(6)
            .align_y(iced::Alignment::Center);

            content_col = content_col.push(props_row);
        }

        // ── Header row: label, edit mode toggle, snap grid ──
        let label = text("Piano Roll").size(11).color(th::text_dim());

        // Edit mode toggle: Select / Draw
        let select_active = self.state.piano_roll.edit_mode == PianoRollEditMode::Select;
        let draw_active = self.state.piano_roll.edit_mode == PianoRollEditMode::Draw;

        let select_btn = {
            let (bg, tc) = if select_active {
                (th::accent_dim(), th::accent())
            } else {
                (th::bg_elevated(), th::text_dim())
            };
            button(icons::icon(icons::MOUSE_POINTER).size(10).color(tc))
                .on_press(Message::PianoRoll(PianoRollMsg::ToggleEditMode))
                .padding([2, 5])
                .style(move |_theme: &Theme, _status| button::Style {
                    background: Some(bg.into()),
                    text_color: tc,
                    border: iced::Border {
                        color: if select_active {
                            th::accent_dim()
                        } else {
                            th::border()
                        },
                        width: 1.0,
                        radius: 3.0.into(),
                    },
                    ..Default::default()
                })
        };

        let draw_btn = {
            let (bg, tc) = if draw_active {
                (th::accent_dim(), th::accent())
            } else {
                (th::bg_elevated(), th::text_dim())
            };
            button(icons::icon(icons::PENCIL).size(10).color(tc))
                .on_press(Message::PianoRoll(PianoRollMsg::ToggleEditMode))
                .padding([2, 5])
                .style(move |_theme: &Theme, _status| button::Style {
                    background: Some(bg.into()),
                    text_color: tc,
                    border: iced::Border {
                        color: if draw_active {
                            th::accent_dim()
                        } else {
                            th::border()
                        },
                        width: 1.0,
                        radius: 3.0.into(),
                    },
                    ..Default::default()
                })
        };

        let mode_row = row![select_btn, draw_btn].spacing(1);

        let snap_picker = pick_list(
            crate::state::SnapGrid::all(),
            Some(
                self.state
                    .view
                    .grid_config()
                    .effective_grid(self.active_editor_pixels_per_beat()),
            ),
            |grid| Message::View(ViewMsg::SetSnapGrid(grid)),
        )
        .width(Length::Fixed(90.0));
        let snap_label = text("Snap:").size(10).color(th::text_dim());
        let header_row = row![label, mode_row, horizontal_space(), snap_label, snap_picker]
            .spacing(4)
            .align_y(iced::Alignment::Center);

        content_col = content_col
            .push(header_row)
            .push(piano_canvas)
            .push(velocity_canvas);

        container(content_col.height(Length::Fill))
            .width(Length::FillPortion(1))
            .height(Length::Fill)
            .style(|_theme: &Theme| container::Style {
                background: Some(th::bg_dark().into()),
                border: iced::Border {
                    color: th::border(),
                    width: 1.0,
                    radius: 0.0.into(),
                },
                ..Default::default()
            })
            .into()
    }
}

#[cfg(test)]
mod tests {
    use super::{
        effective_detail_panel_height, focused_note_clip_for_track,
        resolved_detail_playhead_samples, single_selected_audio_clip_for_track,
        visible_note_clip_for_track,
    };
    use crate::state::{ArrangementSelection, TimelineEditorState};
    use vibez_core::id::{ClipId, SectionId, TrackId};

    #[test]
    fn detail_panel_height_preserves_the_workspace_at_small_windows() {
        assert_eq!(effective_detail_panel_height(80.0, 900.0, 180.0), 180.0);
        assert_eq!(effective_detail_panel_height(360.0, 900.0, 180.0), 360.0);
        assert_eq!(effective_detail_panel_height(800.0, 900.0, 180.0), 420.0);
        assert_eq!(effective_detail_panel_height(800.0, 1_000.0, 180.0), 520.0);
        assert_eq!(effective_detail_panel_height(320.0, 520.0, 180.0), 180.0);
    }

    #[test]
    fn visible_midi_clip_keeps_both_note_and_velocity_editors_usable() {
        assert_eq!(effective_detail_panel_height(280.0, 900.0, 360.0), 360.0);
        assert_eq!(effective_detail_panel_height(420.0, 900.0, 360.0), 420.0);
        assert_eq!(effective_detail_panel_height(280.0, 520.0, 360.0), 180.0);
    }

    #[test]
    fn audio_inspector_gets_enough_height_for_its_compact_controls() {
        assert_eq!(effective_detail_panel_height(180.0, 900.0, 260.0), 260.0);
        assert_eq!(effective_detail_panel_height(320.0, 900.0, 260.0), 320.0);
    }

    #[test]
    fn detail_playhead_resolves_arrange_and_section_clocks_without_crossing_targets() {
        let playing = SectionId::new();
        let other = SectionId::new();

        assert_eq!(
            resolved_detail_playhead_samples(false, None, None, 96_000, 12_000),
            Some(96_000)
        );
        assert_eq!(
            resolved_detail_playhead_samples(true, Some(playing), Some(playing), 96_000, 12_000,),
            Some(12_000)
        );
        assert_eq!(
            resolved_detail_playhead_samples(true, Some(other), Some(playing), 96_000, 12_000,),
            None
        );
    }

    #[test]
    fn audio_inspector_never_chooses_an_arbitrary_clip_from_multi_selection() {
        let track_id = TrackId::new();
        let first = ClipId::new();
        let second = ClipId::new();
        let mut editor = TimelineEditorState::default();
        editor
            .selected_clips
            .insert(ArrangementSelection::AudioClip {
                track_id,
                clip_id: first,
            });
        assert_eq!(
            single_selected_audio_clip_for_track(&editor, track_id),
            Some(first)
        );
        editor
            .selected_clips
            .insert(ArrangementSelection::AudioClip {
                track_id,
                clip_id: second,
            });
        assert_eq!(
            single_selected_audio_clip_for_track(&editor, track_id),
            None
        );
    }

    #[test]
    fn marquee_selected_note_clip_is_the_visible_piano_roll_clip() {
        let track_id = TrackId::new();
        let other_track = TrackId::new();
        let marquee_clip = ClipId::new();
        let explicit_clip = ClipId::new();
        let mut editor = TimelineEditorState::default();
        editor
            .selected_clips
            .insert(ArrangementSelection::NoteClip {
                track_id,
                clip_id: marquee_clip,
            });

        assert_eq!(
            visible_note_clip_for_track(&editor, track_id),
            Some(marquee_clip)
        );
        assert_eq!(visible_note_clip_for_track(&editor, other_track), None);

        editor.selected_note_clip = Some((track_id, explicit_clip));
        assert_eq!(
            visible_note_clip_for_track(&editor, track_id),
            Some(explicit_clip)
        );
    }

    #[test]
    fn arrangement_selection_does_not_focus_the_piano_roll_for_select_all() {
        let track_id = TrackId::new();
        let selected_clip = ClipId::new();
        let explicitly_open_clip = ClipId::new();
        let mut editor = TimelineEditorState::default();
        editor
            .selected_clips
            .insert(ArrangementSelection::NoteClip {
                track_id,
                clip_id: selected_clip,
            });

        // This is the state after the first arrangement Command+A. A
        // second press must remain arrangement select-all.
        assert_eq!(focused_note_clip_for_track(&editor, track_id), None);

        editor.selected_note_clip = Some((track_id, explicitly_open_clip));
        assert_eq!(
            focused_note_clip_for_track(&editor, track_id),
            Some(explicitly_open_clip)
        );
    }
}
