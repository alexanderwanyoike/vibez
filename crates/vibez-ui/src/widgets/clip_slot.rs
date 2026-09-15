//! Musical thumbnails share the clip editor's loop geometry and cached audio peaks.

use std::cell::{Cell, RefCell};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::Arc;

use iced::widget::canvas;
use iced::{mouse, Color, Point, Rectangle, Renderer, Size, Theme};

use crate::domains::perform::LauncherClip;
use crate::message::Message;
use crate::theme as th;
use crate::typography::{PERFORM_LABEL, PERFORM_TECH};

pub struct ClipSlot<'a> {
    pub clip: Option<&'a LauncherClip>,
    pub color: Color,
    pub key: &'a str,
    pub selected: bool,
    pub compact: bool,
}

#[derive(Default)]
pub struct ClipSlotState {
    cache: canvas::Cache,
    fingerprint: Cell<u64>,
    source: RefCell<Option<Arc<crate::state::ArrangementTimeline>>>,
}

fn label(
    frame: &mut canvas::Frame,
    content: &str,
    x: f32,
    y: f32,
    size: f32,
    color: Color,
    technical: bool,
) {
    frame.fill_text(canvas::Text {
        content: content.into(),
        position: Point::new(x, y),
        size: iced::Pixels(size),
        color,
        font: if technical {
            PERFORM_TECH
        } else {
            PERFORM_LABEL
        },
        ..Default::default()
    });
}

fn fit(text: &str, width: f32, size: f32) -> String {
    let limit = (width / (size * 0.61)).max(0.0) as usize;
    if text.chars().count() <= limit {
        text.into()
    } else if limit < 3 {
        String::new()
    } else {
        format!("{}..", text.chars().take(limit - 2).collect::<String>())
    }
}

fn preview_onsets(
    timeline: vibez_core::clip_timeline::BeatClipTimeline,
    source: f64,
    columns: usize,
) -> Vec<f64> {
    let mut occurrences = timeline.occurrences_of(source);
    let mut visible = Vec::new();
    let step = timeline.duration / columns.max(1) as f64;
    for _ in 0..columns {
        let Some(start) = occurrences.next() else {
            break;
        };
        visible.push(start);
        let boundary = ((start / step).floor() + 1.0) * step;
        occurrences = occurrences.starting_after(boundary - timeline.duration * f64::EPSILON);
    }
    visible
}

impl canvas::Program<Message> for ClipSlot<'_> {
    type State = ClipSlotState;

    fn draw(
        &self,
        state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let hovered = cursor.is_over(bounds);
        let mut hash = DefaultHasher::new();
        self.clip
            .map(|c| Arc::as_ptr(&c.timeline) as usize)
            .hash(&mut hash);
        self.key.hash(&mut hash);
        (self.selected, self.compact, hovered, th::epoch()).hash(&mut hash);
        for component in [self.color.r, self.color.g, self.color.b, self.color.a] {
            component.to_bits().hash(&mut hash);
        }
        let fingerprint = hash.finish();
        if state.fingerprint.replace(fingerprint) != fingerprint {
            state.cache.clear();
            // Retaining the source forces edits through copy-on-write and prevents
            // an allocator-reused address from keeping stale thumbnail geometry.
            *state.source.borrow_mut() = self.clip.map(|clip| Arc::clone(&clip.timeline));
        }
        vec![state.cache.draw(renderer, bounds.size(), |frame| {
            let w = frame.width();
            let h = frame.height();
            if w < 8.0 || h < 8.0 {
                return;
            }
            let Some(slot) = self.clip else {
                if hovered {
                    frame.fill_rectangle(Point::ORIGIN, Size::new(w, h), th::bg_hover());
                    label(
                        frame,
                        "+",
                        w / 2.0 - 4.0,
                        h / 2.0 - 9.0,
                        16.0,
                        th::text_dim(),
                        false,
                    );
                }
                frame.fill_rectangle(Point::new(0.0, h - 1.0), Size::new(w, 1.0), th::border());
                label(frame, self.key, w - 19.0, 6.0, 10.0, th::text_dim(), true);
                return;
            };
            let ink = self.color;
            let paper = if hovered {
                th::bg_elevated()
            } else {
                th::display_bg()
            };
            frame.fill_rectangle(Point::ORIGIN, Size::new(w, h), paper);
            frame.fill_rectangle(Point::ORIGIN, Size::new(w, 2.0), self.color);
            if self.selected {
                let edge =
                    canvas::Path::rectangle(Point::new(1.0, 1.0), Size::new(w - 2.0, h - 2.0));
                frame.stroke(
                    &edge,
                    canvas::Stroke::default()
                        .with_color(th::text())
                        .with_width(1.5),
                );
            }
            let tight = self.compact && h < 44.0;
            let title_width = if tight {
                (w * 0.42).max(25.0)
            } else {
                w - 38.0
            };
            label(
                frame,
                &fit(
                    slot.name(),
                    title_width - 12.0,
                    if tight { 10.0 } else { 12.0 },
                ),
                9.0,
                6.0,
                if tight { 10.0 } else { 12.0 },
                th::text(),
                false,
            );
            if !self.key.is_empty() {
                frame.fill_rectangle(Point::new(w - 24.0, 5.0), Size::new(18.0, 17.0), ink);
                label(frame, self.key, w - 19.0, 6.0, 11.0, paper, true);
            }
            let plot = if tight {
                Rectangle::new(
                    Point::new(title_width + 3.0, 5.0),
                    Size::new((w - title_width - 33.0).max(1.0), h - 10.0),
                )
            } else {
                Rectangle::new(
                    Point::new(9.0, 28.0),
                    Size::new(w - 18.0, (h - 38.0).max(3.0)),
                )
            };
            let Some(content) = slot.timeline.get(slot.track_id) else {
                return;
            };
            if let Some(clip) = content.note_clips.first() {
                let duration = clip.duration_beats;
                if !duration.is_finite() || duration <= 0.0 {
                    return;
                }
                for division in 0..=8 {
                    let x = plot.x + division as f32 * plot.width / 8.0;
                    frame.fill_rectangle(
                        Point::new(x, plot.y),
                        Size::new(1.0, plot.height),
                        Color { a: 0.10, ..ink },
                    );
                }
                if clip.notes.is_empty() {
                    if !tight {
                        label(
                            frame,
                            "NO NOTES",
                            plot.x,
                            plot.y + 3.0,
                            9.0,
                            Color { a: 0.65, ..ink },
                            true,
                        );
                    }
                } else {
                    let low = clip.notes.iter().map(|n| n.pitch).min().unwrap_or(60) as f32;
                    let high = clip.notes.iter().map(|n| n.pitch).max().unwrap_or(60) as f32;
                    let span = (high - low + 5.0).max(12.0);
                    let middle = (low + high) / 2.0;
                    for note in &clip.notes {
                        if !note.duration_beats.is_finite() || note.duration_beats <= 0.0 {
                            continue;
                        }
                        // Loop occurrences come from the same source as the piano-roll timeline.
                        // A dense loop cannot paint more than one onset per thumbnail pixel.
                        for start in preview_onsets(
                            clip.timeline(),
                            note.start_beat,
                            plot.width.ceil() as usize,
                        ) {
                            let width = (note.duration_beats.min(duration - start) / duration)
                                as f32
                                * plot.width;
                            let x = plot.x + (start / duration) as f32 * plot.width;
                            let y = plot.y
                                + (0.5 - (f32::from(note.pitch) - middle) / span)
                                    * (plot.height - 3.0);
                            frame.fill_rectangle(
                                Point::new(x, y),
                                Size::new(
                                    width.max(2.0).min(plot.x + plot.width - x),
                                    (plot.height / span).clamp(2.0, 4.0),
                                ),
                                Color {
                                    a: 0.55 + f32::from(note.velocity) / 127.0 * 0.45,
                                    ..ink
                                },
                            );
                        }
                    }
                }
            } else if let Some(clip) = content.clips.first() {
                let peaks = crate::widgets::timeline::compute_clip_peaks(clip);
                let columns = plot.width.floor().max(1.0) as usize;
                let mid = plot.y + plot.height / 2.0;
                for x in 0..columns {
                    let first = x * peaks.len() / columns;
                    let end = ((x + 1) * peaks.len()).div_ceil(columns).min(peaks.len());
                    let (low, high) = peaks[first..end]
                        .iter()
                        .fold((0.0f32, 0.0f32), |(low, high), &(a, b)| {
                            (low.min(a), high.max(b))
                        });
                    let top = mid - high.clamp(-1.0, 1.0) * plot.height * 0.48;
                    let bottom = mid - low.clamp(-1.0, 1.0) * plot.height * 0.48;
                    frame.fill_rectangle(
                        Point::new(plot.x + x as f32, top),
                        Size::new(1.0, (bottom - top).max(1.0)),
                        ink,
                    );
                }
            } else if !tight {
                label(frame, "MEDIA UNAVAILABLE", plot.x, plot.y, 9.0, ink, true);
            }
        })]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vibez_core::clip_timeline::BeatClipTimeline;

    #[test]
    fn dense_loop_preview_spans_the_whole_clip_with_bounded_work() {
        let onsets = preview_onsets(BeatClipTimeline::new(0.0, 0.0, 0.001, 16.0, true), 0.0, 64);
        assert_eq!(onsets.len(), 64);
        assert_eq!(onsets[0], 0.0);
        assert!(onsets[63] > 15.5);
    }

    #[test]
    fn preview_respects_start_marker_and_loop_repetitions() {
        let timeline = BeatClipTimeline::new(1.0, 0.0, 2.0, 4.0, true);
        assert_eq!(preview_onsets(timeline, 1.5, 64), vec![0.5, 2.5]);
        let once = BeatClipTimeline::new(1.0, 0.0, 2.0, 4.0, false);
        assert!(preview_onsets(once, 0.5, 64).is_empty());
    }
}
