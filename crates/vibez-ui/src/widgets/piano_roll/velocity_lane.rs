//! Time-aligned MIDI Note Velocity editing beneath the piano roll.

use iced::keyboard;
use iced::mouse;
use iced::widget::canvas;
use iced::{Point, Rectangle, Renderer, Theme};

use crate::state::UndoGestureId;
use crate::theme;
use crate::widgets::local_drag::LocalDrag;

use super::*;

const TOP_PADDING: f32 = 8.0;
const BOTTOM_PADDING: f32 = 14.0;
const BAR_MIN_WIDTH: f32 = 5.0;
const BAR_MAX_WIDTH: f32 = 12.0;
const BAR_HEAD_HEIGHT: f32 = 4.0;
const HIT_SLOP: f32 = 3.0;

/// Fixed-height editor for the attack Velocity stored on MIDI Notes.
pub struct VelocityLaneWidget {
    track_id: TrackId,
    clip: Option<PianoRollClipData>,
    total_beats: f64,
    track_color: Color,
    grid: GridConfig,
}

impl VelocityLaneWidget {
    pub fn from_clip(
        track_id: TrackId,
        clip: &UiNoteClip,
        total_beats: f64,
        track_color: Color,
        grid: GridConfig,
    ) -> Self {
        Self {
            track_id,
            clip: Some(PianoRollClipData {
                clip_id: clip.id,
                notes: clip.notes.clone(),
                selected_notes: clip.selected_notes.clone(),
                loop_enabled: clip.loop_enabled,
                loop_start_beats: clip.loop_start_beats,
                loop_end_beats: clip.loop_end_beats,
            }),
            total_beats,
            track_color,
            grid,
        }
    }

    pub fn empty(track_id: TrackId, track_color: Color) -> Self {
        Self {
            track_id,
            clip: None,
            total_beats: 16.0,
            track_color,
            grid: GridConfig::new(SnapGrid::EIGHTH, true, false, 0),
        }
    }

    fn geometry(&self, bounds: &Rectangle) -> TimelineGeometry {
        TimelineGeometry::fitted(self.total_beats.max(1.0), bounds.width, KEY_WIDTH)
    }

    fn baseline(bounds: &Rectangle) -> f32 {
        (bounds.height - BOTTOM_PADDING).max(TOP_PADDING)
    }

    fn usable_height(bounds: &Rectangle) -> f32 {
        (Self::baseline(bounds) - TOP_PADDING).max(1.0)
    }

    fn velocity_y(velocity: u8, bounds: &Rectangle) -> f32 {
        let normalized = f32::from(velocity.clamp(1, 127) - 1) / 126.0;
        Self::baseline(bounds) - normalized * Self::usable_height(bounds)
    }

    fn bar_width(&self, note: &MidiNote, bounds: &Rectangle) -> f32 {
        self.geometry(bounds)
            .width_for_beats(note.duration_beats)
            .clamp(BAR_MIN_WIDTH, BAR_MAX_WIDTH)
    }

    fn hit_test_bar(&self, position: Point, bounds: &Rectangle) -> Option<usize> {
        let clip = self.clip.as_ref()?;
        let geometry = self.geometry(bounds);
        let baseline = Self::baseline(bounds);

        // Match piano-roll stacking: the last drawn Note wins when Notes share
        // an onset and their Velocity Bars overlap.
        clip.notes
            .iter()
            .enumerate()
            .rev()
            .find_map(|(index, note)| {
                let x = geometry.beat_to_x(note.start_beat);
                let top = Self::velocity_y(note.velocity, bounds) - HIT_SLOP;
                let width = self.bar_width(note, bounds);
                (position.x >= x - HIT_SLOP
                    && position.x <= x + width + HIT_SLOP
                    && position.y >= top
                    && position.y <= baseline + HIT_SLOP)
                    .then_some(index)
            })
    }

    fn draw_lane(
        &self,
        renderer: &Renderer,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let geometry = self.geometry(&bounds);
        let baseline = Self::baseline(&bounds);

        frame.fill_rectangle(Point::ORIGIN, bounds.size(), theme::bg_dark());
        frame.fill_rectangle(
            Point::ORIGIN,
            iced::Size::new(KEY_WIDTH, bounds.height),
            theme::bg_elevated(),
        );

        frame.fill_text(canvas::Text {
            content: "VEL".into(),
            position: Point::new(5.0, 13.0),
            color: theme::text_dim(),
            size: iced::Pixels(9.0),
            ..Default::default()
        });
        for (velocity, label) in [(127_u8, "127"), (64, "64"), (1, "1")] {
            let y = Self::velocity_y(velocity, &bounds);
            let line = canvas::Path::line(Point::new(KEY_WIDTH, y), Point::new(bounds.width, y));
            frame.stroke(
                &line,
                canvas::Stroke::default()
                    .with_color(theme::grid_sub())
                    .with_width(if velocity == 64 { 1.0 } else { 0.5 }),
            );
            frame.fill_text(canvas::Text {
                content: label.into(),
                position: Point::new(34.0, (y + 3.0).min(bounds.height - 2.0)),
                color: theme::text_muted(),
                size: iced::Pixels(8.0),
                ..Default::default()
            });
        }

        let grid_step = self
            .grid
            .effective_grid(geometry.pixels_per_beat())
            .beat_size();
        let num_steps = (self.total_beats.max(1.0) / grid_step).ceil() as usize;
        for step in 0..=num_steps {
            let beat = step as f64 * grid_step;
            let x = geometry.beat_to_x(beat).floor() + 0.5;
            if x > bounds.width {
                break;
            }
            let beat_millis = (beat * 1000.0).round() as i64;
            let (color, width) = if beat_millis % 4000 == 0 {
                (theme::grid_bar(), 1.5)
            } else if beat_millis % 1000 == 0 {
                (theme::grid_beat(), 1.0)
            } else {
                (theme::grid_sub(), 0.5)
            };
            let line = canvas::Path::line(Point::new(x, 0.0), Point::new(x, baseline));
            frame.stroke(
                &line,
                canvas::Stroke::default()
                    .with_color(color)
                    .with_width(width),
            );
        }

        let hovered = cursor
            .position_in(bounds)
            .and_then(|position| self.hit_test_bar(position, &bounds));
        if let Some(clip) = &self.clip {
            for (index, note) in clip.notes.iter().enumerate() {
                let x = geometry.beat_to_x(note.start_beat);
                if x + BAR_MAX_WIDTH < KEY_WIDTH || x > bounds.width {
                    continue;
                }
                let y = Self::velocity_y(note.velocity, &bounds);
                let width = self.bar_width(note, &bounds);
                let selected = clip.selected_notes.contains(&index);
                let color = if selected {
                    theme::solo_active()
                } else if hovered == Some(index) {
                    theme::accent()
                } else {
                    self.track_color
                };

                frame.fill_rectangle(
                    Point::new(x, y),
                    iced::Size::new(width, (baseline - y).max(1.0)),
                    theme::with_alpha(color, if selected { 0.85 } else { 0.6 }),
                );
                frame.fill_rectangle(
                    Point::new(x, y - BAR_HEAD_HEIGHT / 2.0),
                    iced::Size::new(width, BAR_HEAD_HEIGHT),
                    color,
                );
            }
        }

        let separator = canvas::Path::line(
            Point::new(KEY_WIDTH, 0.0),
            Point::new(KEY_WIDTH, bounds.height),
        );
        frame.stroke(
            &separator,
            canvas::Stroke::default()
                .with_color(theme::border())
                .with_width(1.0),
        );
        let bottom = canvas::Path::line(
            Point::new(KEY_WIDTH, baseline),
            Point::new(bounds.width, baseline),
        );
        frame.stroke(
            &bottom,
            canvas::Stroke::default()
                .with_color(theme::border())
                .with_width(1.0),
        );

        vec![frame.into_geometry()]
    }
}

#[derive(Debug, Clone)]
struct VelocityDrag {
    start_y: f32,
    original_velocities: Vec<(usize, u8)>,
    undo_gesture: UndoGestureId,
}

#[derive(Debug, Default)]
pub struct VelocityLaneState {
    drag: Option<VelocityDrag>,
    shift_held: bool,
}

impl canvas::Program<Message> for VelocityLaneWidget {
    type State = VelocityLaneState;

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        self.draw_lane(renderer, bounds, cursor)
    }

    fn mouse_interaction(
        &self,
        state: &Self::State,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if state.drag.is_some() {
            mouse::Interaction::ResizingVertically
        } else if cursor
            .position_in(bounds)
            .and_then(|position| self.hit_test_bar(position, &bounds))
            .is_some()
        {
            mouse::Interaction::Grab
        } else {
            mouse::Interaction::default()
        }
    }

    fn update(
        &self,
        state: &mut Self::State,
        event: canvas::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> (canvas::event::Status, Option<Message>) {
        match event {
            canvas::Event::Keyboard(keyboard::Event::ModifiersChanged(modifiers)) => {
                state.shift_held = modifiers.shift();
            }
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let Some(position) = cursor.position_in(bounds) else {
                    return (canvas::event::Status::Ignored, None);
                };
                let Some(clip) = &self.clip else {
                    return (canvas::event::Status::Ignored, None);
                };
                let Some(note_index) = self.hit_test_bar(position, &bounds) else {
                    return (
                        canvas::event::Status::Captured,
                        Some(Message::PianoRoll(PianoRollMsg::SelectNote(
                            self.track_id,
                            clip.clip_id,
                            None,
                            false,
                        ))),
                    );
                };

                if state.shift_held && clip.selected_notes.contains(&note_index) {
                    return (
                        canvas::event::Status::Captured,
                        Some(Message::PianoRoll(PianoRollMsg::SelectNote(
                            self.track_id,
                            clip.clip_id,
                            Some(note_index),
                            true,
                        ))),
                    );
                }

                let selected = if state.shift_held {
                    let mut selected = clip.selected_notes.clone();
                    selected.insert(note_index);
                    selected
                } else if clip.selected_notes.contains(&note_index) {
                    clip.selected_notes.clone()
                } else {
                    HashSet::from([note_index])
                };
                let mut original_velocities: Vec<(usize, u8)> = selected
                    .into_iter()
                    .filter_map(|index| clip.notes.get(index).map(|note| (index, note.velocity)))
                    .collect();
                original_velocities.sort_unstable_by_key(|(index, _)| *index);
                state.drag = Some(VelocityDrag {
                    start_y: position.y,
                    original_velocities,
                    undo_gesture: UndoGestureId::new(),
                });

                let selection_message = if clip.selected_notes.contains(&note_index) {
                    None
                } else {
                    Some(Message::PianoRoll(PianoRollMsg::SelectNote(
                        self.track_id,
                        clip.clip_id,
                        Some(note_index),
                        state.shift_held,
                    )))
                };
                return (canvas::event::Status::Captured, selection_message);
            }
            canvas::Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                let Some(drag) = &state.drag else {
                    return (canvas::event::Status::Ignored, None);
                };
                let Some(position) = LocalDrag::unclamped().position(cursor, bounds) else {
                    return (canvas::event::Status::Captured, None);
                };
                let velocities = velocities_after_drag(
                    &drag.original_velocities,
                    drag.start_y,
                    position.y,
                    VelocityLaneWidget::usable_height(&bounds),
                );
                if velocities == drag.original_velocities {
                    return (canvas::event::Status::Captured, None);
                }
                let Some(clip) = &self.clip else {
                    return (canvas::event::Status::Captured, None);
                };
                return (
                    canvas::event::Status::Captured,
                    Some(
                        Message::PianoRoll(PianoRollMsg::SetNoteVelocities {
                            track_id: self.track_id,
                            clip_id: clip.clip_id,
                            velocities,
                        })
                        .in_undo_gesture(drag.undo_gesture),
                    ),
                );
            }
            canvas::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
                if state.drag.take().is_some() =>
            {
                return (canvas::event::Status::Captured, None);
            }
            _ => {}
        }

        (canvas::event::Status::Ignored, None)
    }
}

fn velocities_after_drag(
    originals: &[(usize, u8)],
    start_y: f32,
    current_y: f32,
    usable_height: f32,
) -> Vec<(usize, u8)> {
    if originals.is_empty() {
        return Vec::new();
    }
    let requested_delta = (((start_y - current_y) / usable_height.max(1.0)) * 126.0).round() as i16;
    let min_velocity = originals
        .iter()
        .map(|(_, velocity)| i16::from(*velocity))
        .min()
        .unwrap_or(1);
    let max_velocity = originals
        .iter()
        .map(|(_, velocity)| i16::from(*velocity))
        .max()
        .unwrap_or(127);
    let delta = requested_delta.clamp(1 - min_velocity, 127 - max_velocity);

    originals
        .iter()
        .map(|(index, velocity)| (*index, (i16::from(*velocity) + delta) as u8))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::widget::canvas::Program;
    use iced::Size;

    fn note(start_beat: f64, velocity: u8) -> MidiNote {
        MidiNote {
            pitch: 60,
            velocity,
            start_beat,
            duration_beats: 0.5,
        }
    }

    fn clip(notes: Vec<MidiNote>, selected_notes: HashSet<usize>) -> UiNoteClip {
        UiNoteClip {
            id: ClipId::new(),
            name: "Velocity test".into(),
            position_beats: 0.0,
            duration_beats: 4.0,
            notes,
            selected_notes,
            loop_enabled: false,
            loop_start_beats: 0.0,
            loop_end_beats: 4.0,
            groove_grid: vibez_core::perform::GrooveGrid::Off,
        }
    }

    fn piano_roll_message(message: Option<Message>) -> Option<PianoRollMsg> {
        match message {
            Some(Message::UndoGesture { edit, .. }) => piano_roll_message(Some(*edit)),
            Some(Message::PianoRoll(message)) => Some(message),
            _ => None,
        }
    }

    fn gesture_id(message: &Option<Message>) -> Option<UndoGestureId> {
        match message {
            Some(Message::UndoGesture { id, .. }) => Some(*id),
            _ => None,
        }
    }

    #[test]
    fn relative_drag_preserves_differences_and_clamps_the_group_together() {
        let originals = vec![(0, 40), (1, 72), (2, 100)];
        assert_eq!(
            velocities_after_drag(&originals, 80.0, 70.0, 126.0),
            vec![(0, 50), (1, 82), (2, 110)]
        );
        assert_eq!(
            velocities_after_drag(&originals, 80.0, -80.0, 126.0),
            vec![(0, 67), (1, 99), (2, 127)]
        );
        assert_eq!(
            velocities_after_drag(&originals, 80.0, 300.0, 126.0),
            vec![(0, 1), (1, 33), (2, 61)]
        );
    }

    #[test]
    fn selected_bar_drag_edits_the_complete_selection_with_one_gesture_id() {
        let track_id = TrackId::new();
        let clip = clip(vec![note(0.0, 50), note(1.0, 80)], HashSet::from([0, 1]));
        let widget = VelocityLaneWidget::from_clip(
            track_id,
            &clip,
            4.0,
            Color::WHITE,
            GridConfig::new(SnapGrid::SIXTEENTH, true, false, 0),
        );
        let bounds = Rectangle::new(Point::ORIGIN, Size::new(452.0, 92.0));
        let x = widget.geometry(&bounds).beat_to_x(0.0) + 2.0;
        let start_y = VelocityLaneWidget::velocity_y(50, &bounds) + 2.0;
        let mut state = VelocityLaneState::default();
        let at = |y| mouse::Cursor::Available(Point::new(x, y));

        let press = widget
            .update(
                &mut state,
                canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
                bounds,
                at(start_y),
            )
            .1;
        assert!(
            press.is_none(),
            "already-selected bar keeps shared selection"
        );

        let first = widget
            .update(
                &mut state,
                canvas::Event::Mouse(mouse::Event::CursorMoved {
                    position: Point::new(x, start_y - 10.0),
                }),
                bounds,
                at(start_y - 10.0),
            )
            .1;
        let second = widget
            .update(
                &mut state,
                canvas::Event::Mouse(mouse::Event::CursorMoved {
                    position: Point::new(x, start_y - 20.0),
                }),
                bounds,
                at(start_y - 20.0),
            )
            .1;

        assert!(matches!(
            piano_roll_message(first.clone()),
            Some(PianoRollMsg::SetNoteVelocities { ref velocities, .. })
                if velocities.len() == 2 && velocities[1].1 - velocities[0].1 == 30
        ));
        assert_eq!(gesture_id(&first), gesture_id(&second));
        assert!(gesture_id(&first).is_some());

        widget.update(
            &mut state,
            canvas::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
            bounds,
            at(start_y - 20.0),
        );
        widget.update(
            &mut state,
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            bounds,
            at(start_y),
        );
        let later_drag = widget
            .update(
                &mut state,
                canvas::Event::Mouse(mouse::Event::CursorMoved {
                    position: Point::new(x, start_y - 10.0),
                }),
                bounds,
                at(start_y - 10.0),
            )
            .1;
        assert_ne!(gesture_id(&first), gesture_id(&later_drag));
    }

    #[test]
    fn unselected_bar_press_selects_only_that_note_before_dragging() {
        let track_id = TrackId::new();
        let clip = clip(vec![note(0.0, 50), note(1.0, 80)], HashSet::from([0]));
        let widget = VelocityLaneWidget::from_clip(
            track_id,
            &clip,
            4.0,
            Color::WHITE,
            GridConfig::new(SnapGrid::SIXTEENTH, true, false, 0),
        );
        let bounds = Rectangle::new(Point::ORIGIN, Size::new(452.0, 92.0));
        let x = widget.geometry(&bounds).beat_to_x(1.0) + 2.0;
        let y = VelocityLaneWidget::velocity_y(80, &bounds) + 2.0;
        let mut state = VelocityLaneState::default();

        let message = widget
            .update(
                &mut state,
                canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
                bounds,
                mouse::Cursor::Available(Point::new(x, y)),
            )
            .1;

        assert!(matches!(
            piano_roll_message(message),
            Some(PianoRollMsg::SelectNote(_, _, Some(1), false))
        ));
        assert_eq!(
            state.drag.as_ref().unwrap().original_velocities,
            vec![(1, 80)]
        );
    }
}
