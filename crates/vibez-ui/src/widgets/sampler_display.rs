//! The Sampler's waveform: the loaded sample with its Start and End trim,
//! and its loop while looping, each marker dragged to move it.

use std::cell::Cell;
use std::sync::Arc;

use iced::mouse;
use iced::widget::canvas;
use iced::{Color, Point, Rectangle, Renderer, Size, Theme};
use vibez_core::audio_buffer::DecodedAudio;
use vibez_core::id::TrackId;
use vibez_instruments::sampler::param;

use crate::domains::device_params::DeviceSlot;
use crate::message::Message;
use crate::state::UndoGestureId;
use crate::theme as th;
use crate::widgets::mini_waveform::column_peaks;

const LABEL_H: f32 = 14.0;
/// How close the pointer must be to a marker to pick it up.
const GRAB_PX: f32 = 6.0;
/// Markers keep this share of the sample apart.
const MIN_GAP: f32 = 0.001;

/// Where each marker sits, 0 to 1 through the sample.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Markers {
    pub start: f32,
    pub end: f32,
    /// The loop's start and end, when looping.
    pub looped: Option<(f32, f32)>,
}

impl Markers {
    /// The markers in drawing order: their parameter, name and place.
    fn each(&self) -> Vec<(usize, &'static str, f32)> {
        let mut markers = vec![(param::START, "Start", self.start)];
        if let Some((loop_start, loop_end)) = self.looped {
            markers.push((param::LOOP_START, "Loop Start", loop_start));
            markers.push((param::LOOP_END, "Loop End", loop_end));
        }
        markers.push((param::END, "End", self.end));
        markers
    }

    /// The markers the pointer can pick up, the loop's first: they sit
    /// inside the trim's, so they win a tie, as when the loop fills it.
    fn by_grab_order(&self) -> Vec<(usize, &'static str, f32)> {
        let mut markers = self.each();
        markers.sort_by_key(|(index, _, _)| !matches!(*index, param::LOOP_START | param::LOOP_END));
        markers
    }

    /// Where a marker may go: inside its neighbours, so the trim keeps the
    /// loop and each pair stays in order.
    pub fn limit(&self, index: usize, value: f32) -> f32 {
        let (loop_start, loop_end) = self.looped.unwrap_or((self.start, self.end));
        let (low, high) = match index {
            param::START => (0.0, self.end - MIN_GAP),
            param::END => (self.start + MIN_GAP, 1.0),
            param::LOOP_START => (self.start, loop_end - MIN_GAP),
            _ => (loop_start + MIN_GAP, self.end),
        };
        value.clamp(low, high.max(low))
    }
}

/// Seconds with no more digits than a sample's length needs.
fn time_label(seconds: f32) -> String {
    if seconds >= 10.0 {
        format!("{seconds:.0}")
    } else {
        format!("{seconds:.1}")
    }
}

pub struct SampleView<'a> {
    pub track_id: TrackId,
    pub slot: DeviceSlot,
    pub audio: Option<&'a Arc<DecodedAudio>>,
    pub markers: Markers,
    pub accent: Color,
}

#[derive(Default)]
pub struct SampleViewState {
    dragging: Option<usize>,
    gesture: Option<UndoGestureId>,
    cache: canvas::Cache,
    /// (sample, width, theme) the cached waveform was drawn for.
    drawn_for: Cell<(usize, u32, u64)>,
}

impl SampleView<'_> {
    fn plot(bounds: Size) -> Rectangle {
        let labels = bounds.height >= 60.0;
        Rectangle::new(
            Point::new(4.0, if labels { LABEL_H } else { 4.0 }),
            Size::new(
                (bounds.width - 8.0).max(1.0),
                (bounds.height - if labels { LABEL_H * 2.0 } else { 8.0 }).max(1.0),
            ),
        )
    }

    fn x(plot: Rectangle, position: f32) -> f32 {
        plot.x + plot.width * position.clamp(0.0, 1.0)
    }
}

impl canvas::Program<Message> for SampleView<'_> {
    type State = SampleViewState;

    fn update(
        &self,
        state: &mut SampleViewState,
        event: canvas::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> (canvas::event::Status, Option<Message>) {
        let plot = Self::plot(bounds.size());
        match event {
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let Some(at) = cursor.position_in(bounds) else {
                    return (canvas::event::Status::Ignored, None);
                };
                if self.audio.is_none() {
                    return (canvas::event::Status::Ignored, None);
                }
                let grabbed = self
                    .markers
                    .by_grab_order()
                    .into_iter()
                    .map(|(index, _, position)| (index, (Self::x(plot, position) - at.x).abs()))
                    .filter(|(_, distance)| *distance <= GRAB_PX)
                    .min_by(|a, b| a.1.total_cmp(&b.1))
                    .map(|(index, _)| index);
                if grabbed.is_some() {
                    state.dragging = grabbed;
                    state.gesture = Some(UndoGestureId::new());
                    return (canvas::event::Status::Captured, None);
                }
            }
            canvas::Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                if let (Some(index), Some(at)) = (state.dragging, cursor.position()) {
                    let position = (at.x - bounds.x - plot.x) / plot.width;
                    let value = self.markers.limit(index, position);
                    let gesture = *state.gesture.get_or_insert_with(UndoGestureId::new);
                    return (
                        canvas::event::Status::Captured,
                        Some(
                            self.slot
                                .set(self.track_id, index, value)
                                .in_undo_gesture(gesture),
                        ),
                    );
                }
            }
            canvas::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
                if state.dragging.take().is_some() =>
            {
                state.gesture = None;
                return (canvas::event::Status::Captured, None);
            }
            _ => {}
        }
        (canvas::event::Status::Ignored, None)
    }

    fn mouse_interaction(
        &self,
        state: &SampleViewState,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        let plot = Self::plot(bounds.size());
        let near = cursor.position_in(bounds).is_some_and(|at| {
            self.markers
                .each()
                .iter()
                .any(|(_, _, position)| (Self::x(plot, *position) - at.x).abs() <= GRAB_PX)
        });
        if state.dragging.is_some() || (near && self.audio.is_some()) {
            mouse::Interaction::ResizingHorizontally
        } else {
            mouse::Interaction::default()
        }
    }

    fn draw(
        &self,
        state: &SampleViewState,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let size = bounds.size();
        let plot = Self::plot(size);
        let labels = size.height >= 60.0;
        let drawn_for = (
            self.audio.map_or(0, |audio| Arc::as_ptr(audio) as usize),
            size.width as u32,
            th::epoch(),
        );
        if state.drawn_for.get() != drawn_for {
            state.cache.clear();
            state.drawn_for.set(drawn_for);
        }
        let accent = self.accent;
        let audio = self.audio;
        let waveform = state.cache.draw(renderer, size, |frame| {
            frame.fill(
                &canvas::Path::rounded_rectangle(Point::ORIGIN, size, 3.0.into()),
                th::bg_dark(),
            );
            let mid = plot.y + plot.height / 2.0;
            let Some(audio) = audio else {
                frame.stroke(
                    &canvas::Path::line(
                        Point::new(plot.x, mid),
                        Point::new(plot.x + plot.width, mid),
                    ),
                    canvas::Stroke::default()
                        .with_color(th::border())
                        .with_width(1.0),
                );
                return;
            };
            let Some(samples) = audio.channels.first().filter(|c| !c.is_empty()) else {
                return;
            };
            let half = plot.height / 2.0 - 1.0;
            for (column, (min, max)) in column_peaks(samples, plot.width as usize).enumerate() {
                let top = mid - max.clamp(-1.0, 1.0) * half;
                let bottom = mid - min.clamp(-1.0, 1.0) * half;
                frame.fill_rectangle(
                    Point::new(plot.x + column as f32, top.min(bottom)),
                    Size::new(1.0, (bottom - top).abs().max(1.0)),
                    accent,
                );
            }
            if labels {
                let seconds = samples.len() as f32 / audio.sample_rate.max(1) as f32;
                let step = [0.1f32, 0.25, 0.5, 1.0, 2.0, 5.0, 10.0, 30.0]
                    .into_iter()
                    .find(|step| seconds / step <= 8.0)
                    .unwrap_or(60.0);
                let mut at = 0.0;
                while at <= seconds + 1e-3 {
                    let x = plot.x + plot.width * at / seconds;
                    frame.stroke(
                        &canvas::Path::line(
                            Point::new(x, plot.y),
                            Point::new(x, plot.y + plot.height),
                        ),
                        canvas::Stroke::default()
                            .with_color(Color {
                                a: 0.35,
                                ..th::border()
                            })
                            .with_width(1.0),
                    );
                    frame.fill_text(canvas::Text {
                        content: time_label(at),
                        position: Point::new(x + 2.0, plot.y + plot.height + LABEL_H / 2.0),
                        color: th::text_muted(),
                        size: 9.0.into(),
                        vertical_alignment: iced::alignment::Vertical::Center,
                        ..canvas::Text::default()
                    });
                    at += step;
                }
            }
        });

        let mut frame = canvas::Frame::new(renderer, size);
        if self.audio.is_some() {
            let Markers { start, end, looped } = self.markers;
            // Outside the trim is dimmed, the loop is shaded.
            for (from, to) in [(0.0, start), (end, 1.0)] {
                frame.fill_rectangle(
                    Point::new(Self::x(plot, from), plot.y),
                    Size::new(Self::x(plot, to) - Self::x(plot, from), plot.height),
                    Color {
                        a: 0.6,
                        ..th::bg_dark()
                    },
                );
            }
            if let Some((loop_start, loop_end)) = looped {
                frame.fill_rectangle(
                    Point::new(Self::x(plot, loop_start), plot.y),
                    Size::new(
                        Self::x(plot, loop_end) - Self::x(plot, loop_start),
                        plot.height,
                    ),
                    Color {
                        a: 0.16,
                        ..self.accent
                    },
                );
            }
            for (index, name, position) in self.markers.each() {
                let x = Self::x(plot, position);
                let lit = state.dragging == Some(index);
                let color = if lit { th::text() } else { self.accent };
                frame.stroke(
                    &canvas::Path::line(Point::new(x, plot.y), Point::new(x, plot.y + plot.height)),
                    canvas::Stroke::default().with_color(color).with_width(1.5),
                );
                frame.fill_rectangle(Point::new(x - 3.0, plot.y), Size::new(6.0, 6.0), color);
                if labels {
                    frame.fill_text(canvas::Text {
                        content: name.into(),
                        position: Point::new(x + 4.0, LABEL_H / 2.0),
                        color: th::text_dim(),
                        size: 9.0.into(),
                        vertical_alignment: iced::alignment::Vertical::Center,
                        ..canvas::Text::default()
                    });
                }
            }
        } else if labels {
            frame.fill_text(canvas::Text {
                content: "Load a sample".into(),
                position: Point::new(size.width / 2.0, size.height / 2.0 - 10.0),
                color: th::text_muted(),
                size: 11.0.into(),
                horizontal_alignment: iced::alignment::Horizontal::Center,
                vertical_alignment: iced::alignment::Vertical::Center,
                ..canvas::Text::default()
            });
        }
        vec![waveform, frame.into_geometry()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markers_stay_in_order_and_inside_the_trim() {
        let markers = Markers {
            start: 0.1,
            end: 0.9,
            looped: Some((0.3, 0.6)),
        };
        assert_eq!(markers.limit(param::START, 0.95), 0.9 - MIN_GAP);
        assert_eq!(markers.limit(param::END, 0.05), 0.1 + MIN_GAP);
        assert_eq!(markers.limit(param::LOOP_START, 0.0), 0.1);
        assert_eq!(markers.limit(param::LOOP_START, 0.7), 0.6 - MIN_GAP);
        assert_eq!(markers.limit(param::LOOP_END, 0.2), 0.3 + MIN_GAP);
        assert_eq!(markers.limit(param::LOOP_END, 1.0), 0.9);
        assert_eq!(markers.limit(param::START, 0.2), 0.2, "free inside");
    }

    #[test]
    fn a_loop_that_fills_the_trim_is_picked_up_before_the_trim() {
        let markers = Markers {
            start: 0.0,
            end: 1.0,
            looped: Some((0.0, 1.0)),
        };
        let order: Vec<usize> = markers
            .by_grab_order()
            .iter()
            .map(|(index, _, _)| *index)
            .collect();
        assert_eq!(&order[..2], &[param::LOOP_START, param::LOOP_END]);
    }
}
