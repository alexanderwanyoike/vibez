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
    pub cell: (vibez_core::id::TrackId, u32),
    /// The clip being dragged across the launcher, from any cell.
    pub drag_source: Option<vibez_core::id::ClipId>,
    pub empty_action: Message,
    pub color: Color,
    pub key: &'a str,
    pub selected: bool,
    pub recording: bool,
    pub audio_recording: Option<ClipRecordingWaveform>,
    pub playing: bool,
    pub progress: Option<f32>,
    pub queued: bool,
    pub compact: bool,
}

pub struct ClipRecordingWaveform {
    pub preview: crate::domains::audio_recording::AudioRecordingPreview,
    pub offset: u64,
    pub loop_length: Option<u64>,
}

#[derive(Default)]
pub struct ClipSlotState {
    cache: canvas::Cache,
    double_click: crate::widgets::double_click::DoubleClick,
    fingerprint: Cell<u64>,
    source: RefCell<Option<Arc<crate::state::ArrangementTimeline>>>,
    press: Option<Point>,
    dragging: bool,
    modifiers: iced::keyboard::Modifiers,
}

/// How far a press travels before it drags rather than clicks.
const DRAG_START: f32 = 6.0;

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

pub(crate) fn fit(text: &str, width: f32, size: f32) -> String {
    let limit = (width / (size * 0.61)).max(0.0) as usize;
    if text.chars().count() <= limit {
        text.into()
    } else if limit < 3 {
        String::new()
    } else {
        format!("{}..", text.chars().take(limit - 2).collect::<String>())
    }
}

/// The grid is glass: each cell is a well sunk into it, and a clip is a
/// film laid inside the well, so glass shows around every clip.
const WELL_INSET: f32 = 2.0;
pub(crate) const FILM_INSET: f32 = WELL_INSET + 4.0;
const FILM_RADIUS: f32 = 3.0;
const HEADER_H: f32 = 18.0;
/// Room along the film's bottom for the cell's buttons.
const BUTTON_ROOM: f32 = 21.0;

fn inset(size: Size, by: f32) -> Rectangle {
    Rectangle::new(
        Point::new(by, by),
        Size::new(
            (size.width - 2.0 * by).max(1.0),
            (size.height - 2.0 * by).max(1.0),
        ),
    )
}

fn rounded(area: Rectangle, radius: impl Into<iced::border::Radius>) -> canvas::Path {
    canvas::Path::rounded_rectangle(area.position(), area.size(), radius.into())
}

fn vertical(area: Rectangle, top: Color, bottom: Color) -> canvas::Fill {
    canvas::gradient::Linear::new(
        Point::new(area.x, area.y),
        Point::new(area.x, area.y + area.height),
    )
    .add_stop(0.0, top)
    .add_stop(1.0, bottom)
    .into()
}

fn film_base() -> Color {
    th::darken(th::bg_dark(), 0.47)
}

/// The film tinted toward its track colour. Opaque on purpose: the renderer
/// blends alpha in linear light, which makes a faint alpha tint read about
/// twice as strong as the same tint mixed here.
fn tint(ink: Color, amount: f32) -> Color {
    th::blend(film_base(), ink, amount)
}

/// In a short cell the title shares the film's one line with the thumbnail.
fn tight_title_width(film_width: f32, has_key: bool) -> f32 {
    let key_space = if has_key { 22.0 } else { 0.0 };
    (film_width * 0.42)
        .max(25.0)
        .min((film_width - 38.0 - key_space).max(0.0))
}

fn thumbnail_plot(size: Size, compact: bool, has_key: bool) -> Rectangle {
    let film = inset(size, FILM_INSET);
    if compact && size.height < 44.0 {
        let title_width = tight_title_width(film.width, has_key);
        Rectangle::new(
            Point::new(film.x + title_width + 20.0, film.y + 3.0),
            Size::new(
                (film.width - title_width - 70.0).max(1.0),
                (film.height - 6.0).max(1.0),
            ),
        )
    } else {
        let top = film.y + HEADER_H + 2.0;
        Rectangle::new(
            Point::new(film.x + 7.0, top),
            Size::new(
                (film.width - 14.0).max(1.0),
                (film.y + film.height - BUTTON_ROOM - top).max(3.0),
            ),
        )
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

    fn update(
        &self,
        state: &mut Self::State,
        event: canvas::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> (canvas::event::Status, Option<Message>) {
        use crate::domains::perform::{ClipMsg, PerformMsg};
        match event {
            canvas::Event::Keyboard(iced::keyboard::Event::ModifiersChanged(modifiers)) => {
                state.modifiers = modifiers;
            }
            canvas::Event::Mouse(mouse::Event::CursorMoved { position }) => {
                if let (Some(clip), Some(press)) = (self.clip, state.press) {
                    if !state.dragging && press.distance(position) > DRAG_START {
                        state.dragging = true;
                        return (
                            canvas::event::Status::Captured,
                            Some(Message::Perform(PerformMsg::Clips(ClipMsg::BeginDrag(
                                clip.id,
                            )))),
                        );
                    }
                }
            }
            canvas::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                state.press = None;
                // Ignored, so the cell's button under this canvas still
                // sees its own release and resets.
                if std::mem::take(&mut state.dragging) {
                    return (
                        canvas::event::Status::Ignored,
                        Some(Message::Perform(PerformMsg::Clips(ClipMsg::EndDrag))),
                    );
                }
                if let Some(source) = self.drag_source.filter(|_| cursor.is_over(bounds)) {
                    if self.clip.is_none_or(|clip| clip.id != source) {
                        return (
                            canvas::event::Status::Captured,
                            Some(Message::Perform(PerformMsg::Clips(ClipMsg::Drop {
                                clip: source,
                                track_id: self.cell.0,
                                row: self.cell.1,
                                copy: crate::app::command_held(
                                    state.modifiers,
                                    crate::app::ON_MACOS,
                                ),
                            }))),
                        );
                    }
                }
            }
            _ => {}
        }
        if matches!(
            event,
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
        ) {
            if self.clip.is_some() && cursor.is_over(bounds) {
                state.press = cursor.position();
            }
            if let Some(point) = cursor.position_in(bounds) {
                let title = point.y < 25.0 && point.x >= 6.0 && point.x < bounds.width - 48.0;
                if self.clip.is_none() || title {
                    if state.double_click.press(
                        std::time::Instant::now(),
                        point,
                        std::time::Duration::from_millis(300),
                        Some(5.0),
                    ) {
                        state.double_click.clear();
                        return (
                            canvas::event::Status::Captured,
                            Some(self.clip.map_or_else(
                                || self.empty_action.clone(),
                                |clip| {
                                    Message::View(
                                        crate::domains::view::ViewMsg::StartEditingClipName(
                                            clip.track_id,
                                            clip.id,
                                        ),
                                    )
                                },
                            )),
                        );
                    }
                } else {
                    state.double_click.clear();
                }
            }
        }

        (canvas::event::Status::Ignored, None)
    }

    fn draw(
        &self,
        state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let hovered = cursor.is_over(bounds);
        let drop_target = hovered
            && self
                .drag_source
                .is_some_and(|source| self.clip.is_none_or(|clip| clip.id != source));
        let mut hash = DefaultHasher::new();
        self.clip
            .map(|c| Arc::as_ptr(&c.timeline) as usize)
            .hash(&mut hash);
        self.key.hash(&mut hash);
        (
            self.selected,
            self.recording,
            self.compact,
            self.playing,
            self.queued,
            hovered,
            drop_target,
            th::epoch(),
        )
            .hash(&mut hash);
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
        let mut geometry = vec![state.cache.draw(renderer, bounds.size(), |frame| {
            let w = frame.width();
            let h = frame.height();
            if w < 8.0 || h < 8.0 {
                return;
            }
            let size = Size::new(w, h);
            let well = inset(size, WELL_INSET);
            let well_path = rounded(well, FILM_RADIUS);
            frame.fill(
                &well_path,
                th::with_alpha(Color::BLACK, if hovered { 0.28 } else { 0.38 }),
            );
            frame.fill_rectangle(
                Point::new(well.x + FILM_RADIUS, well.y),
                Size::new((well.width - 2.0 * FILM_RADIUS).max(0.0), 1.0),
                th::with_alpha(Color::BLACK, 0.65),
            );
            if drop_target {
                frame.stroke(
                    &well_path,
                    canvas::Stroke::default()
                        .with_color(th::accent())
                        .with_width(1.5),
                );
                let ghost = rounded(inset(size, FILM_INSET), FILM_RADIUS);
                frame.fill(&ghost, th::with_alpha(th::accent(), 0.035));
                frame.stroke(
                    &ghost,
                    canvas::Stroke {
                        line_dash: canvas::LineDash {
                            segments: &[4.0, 3.0],
                            offset: 0,
                        },
                        ..canvas::Stroke::default()
                            .with_color(th::with_alpha(th::accent(), 0.7))
                            .with_width(1.0)
                    },
                );
            }
            let Some(slot) = self.clip else {
                label(
                    frame,
                    self.key,
                    w - 22.0 - WELL_INSET,
                    6.0,
                    10.0,
                    th::text_dim(),
                    true,
                );
                if self.selected && !drop_target {
                    frame.stroke(
                        &well_path,
                        canvas::Stroke::default()
                            .with_color(th::text_dim())
                            .with_width(1.0),
                    );
                }
                return;
            };
            let ink = if self.recording {
                th::danger()
            } else {
                self.color
            };
            let tight = self.compact && h < 44.0;
            let film = inset(size, FILM_INSET);
            let film_path = rounded(film, FILM_RADIUS);
            frame.fill(
                &rounded(
                    Rectangle {
                        y: film.y + 2.0,
                        ..film
                    },
                    FILM_RADIUS,
                ),
                th::with_alpha(Color::BLACK, 0.6),
            );
            frame.fill(&film_path, vertical(film, tint(ink, 0.17), tint(ink, 0.07)));
            let (band_top, band_bottom) = if self.playing {
                (0.63, 0.42)
            } else {
                (0.48, 0.31)
            };
            let r = FILM_RADIUS;
            let (header, corners) = if tight {
                (
                    Rectangle {
                        width: tight_title_width(film.width, !self.key.is_empty()) + 6.0,
                        ..film
                    },
                    iced::border::Radius {
                        top_left: r,
                        top_right: 0.0,
                        bottom_right: 0.0,
                        bottom_left: r,
                    },
                )
            } else {
                (
                    Rectangle {
                        height: HEADER_H,
                        ..film
                    },
                    iced::border::Radius {
                        top_left: r,
                        top_right: r,
                        bottom_right: 0.0,
                        bottom_left: 0.0,
                    },
                )
            };
            frame.fill(
                &rounded(header, corners),
                vertical(header, tint(ink, band_top), tint(ink, band_bottom)),
            );
            if !tight {
                frame.fill_rectangle(
                    Point::new(film.x, film.y + HEADER_H - 1.0),
                    Size::new(film.width, 1.0),
                    tint(ink, 0.55),
                );
            }
            frame.stroke(
                &film_path,
                canvas::Stroke::default()
                    .with_color(tint(ink, if self.playing { 0.8 } else { 0.4 }))
                    .with_width(1.0),
            );
            frame.fill_rectangle(
                Point::new(film.x + r, film.y),
                Size::new((film.width - 2.0 * r).max(0.0), 1.0),
                tint(ink, if self.playing { 1.0 } else { 0.85 }),
            );
            if self.selected {
                frame.stroke(
                    &rounded(inset(size, FILM_INSET - 2.0), r + 1.0),
                    canvas::Stroke::default()
                        .with_color(th::text())
                        .with_width(1.5),
                );
            }
            let key_space = if self.key.is_empty() { 0.0 } else { 22.0 };
            let title_width = if tight {
                tight_title_width(film.width, !self.key.is_empty())
            } else {
                w - 50.0 - key_space - 2.0 * FILM_INSET
            };
            label(
                frame,
                &fit(
                    if self.recording && slot.timeline.get(slot.track_id).is_none() {
                        "Recording audio"
                    } else {
                        slot.name()
                    },
                    title_width - 12.0,
                    if tight { 10.0 } else { 12.0 },
                ),
                film.x + 6.0,
                film.y + if tight { 4.0 } else { 3.0 },
                if tight { 10.0 } else { 12.0 },
                th::text(),
                false,
            );
            if !self.key.is_empty() {
                label(
                    frame,
                    self.key,
                    w - 45.0,
                    film.y + 4.0,
                    10.0,
                    th::with_alpha(th::text(), 0.75),
                    true,
                );
            }
            let plot = thumbnail_plot(bounds.size(), self.compact, !self.key.is_empty());
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
        })];
        if let Some(recording) = &self.audio_recording {
            let preview = &recording.preview;
            let elapsed = preview.duration.saturating_sub(recording.offset);
            let duration = recording.loop_length.unwrap_or(elapsed).max(1);
            let plot = thumbnail_plot(bounds.size(), self.compact, !self.key.is_empty());
            let width = plot.width as usize;
            let height = plot.height;
            let mut overlay = canvas::Frame::new(renderer, bounds.size());
            overlay.fill_rectangle(plot.position(), plot.size(), film_base());
            for x in 0..width {
                let local = x as u64 * duration / width as u64;
                let pass = elapsed.saturating_sub(local + 1) / duration;
                let sample = local + pass * duration;
                if sample >= elapsed {
                    continue;
                }
                let index = (recording.offset + sample) as usize / preview.frames_per_peak;
                if let Some(&(low, high)) = preview.peaks.get(index) {
                    let top = plot.y + height * (0.5 - high.clamp(-1.0, 1.0) * 0.48);
                    let bottom = plot.y + height * (0.5 - low.clamp(-1.0, 1.0) * 0.48);
                    overlay.fill_rectangle(
                        Point::new(plot.x + x as f32, top),
                        Size::new(1.0, (bottom - top).max(1.0)),
                        self.color,
                    );
                }
            }
            geometry.push(overlay.into_geometry());
        }
        if self.progress.is_some() || self.queued {
            let mut overlay = canvas::Frame::new(renderer, bounds.size());
            let film = inset(bounds.size(), FILM_INSET);
            if let Some(progress) = self.progress {
                let played = (film.width - 2.0) * progress.clamp(0.0, 1.0);
                overlay.fill_rectangle(
                    Point::new(film.x + 1.0, film.y + 1.0),
                    Size::new(played, film.height - 2.0),
                    th::with_alpha(self.color, 0.07),
                );
                overlay.fill_rectangle(
                    Point::new(film.x + 1.0 + played, film.y + 1.0),
                    Size::new(1.0, film.height - 2.0),
                    th::with_alpha(self.color, 0.7),
                );
            }
            if self.queued {
                overlay.fill_rectangle(
                    Point::new(film.x + FILM_RADIUS, film.y),
                    Size::new((film.width - 2.0 * FILM_RADIUS).max(0.0), 2.0),
                    th::text(),
                );
            }
            geometry.push(overlay.into_geometry());
        }
        geometry
    }
}

pub struct LauncherTrackName<'a> {
    pub track_id: vibez_core::id::TrackId,
    pub name: &'a str,
    pub color: Color,
}

impl canvas::Program<Message> for LauncherTrackName<'_> {
    type State = crate::widgets::double_click::DoubleClick;
    fn draw(
        &self,
        _: &Self::State,
        renderer: &Renderer,
        _: &Theme,
        bounds: Rectangle,
        _: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        label(
            &mut frame,
            &fit(self.name, bounds.width, 13.0),
            0.0,
            0.0,
            13.0,
            self.color,
            false,
        );
        vec![frame.into_geometry()]
    }
    fn update(
        &self,
        state: &mut Self::State,
        event: canvas::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> (canvas::event::Status, Option<Message>) {
        if matches!(
            event,
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
        ) {
            if let Some(point) = cursor.position_in(bounds) {
                if state.press(
                    std::time::Instant::now(),
                    point,
                    std::time::Duration::from_millis(300),
                    Some(5.0),
                ) {
                    state.clear();
                    return (
                        canvas::event::Status::Captured,
                        Some(Message::View(
                            crate::domains::view::ViewMsg::StartEditingTrackName {
                                track_id: self.track_id,
                                name: self.name.into(),
                            },
                        )),
                    );
                }
                return (canvas::event::Status::Captured, None);
            }
        }
        (canvas::event::Status::Ignored, None)
    }
    fn mouse_interaction(
        &self,
        _: &Self::State,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if cursor.is_over(bounds) {
            mouse::Interaction::Text
        } else {
            mouse::Interaction::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vibez_core::clip_timeline::BeatClipTimeline;

    #[test]
    fn the_thumbnail_sits_inside_the_film_between_its_header_and_buttons() {
        for height in [48.0, 56.0, 64.0, 72.0] {
            let size = Size::new(240.0, height);
            let film = inset(size, FILM_INSET);
            let plot = thumbnail_plot(size, false, true);
            assert!(plot.x >= film.x && plot.x + plot.width <= film.x + film.width);
            assert!(plot.y >= film.y + HEADER_H, "{height}: below the header");
            if height >= 64.0 {
                assert!(
                    plot.y + plot.height <= film.y + film.height - BUTTON_ROOM,
                    "{height}: clear of the buttons"
                );
            }
        }
    }

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

#[cfg(test)]
mod name_tests {
    use super::*;
    use iced::widget::canvas::Program;
    #[test]
    fn a_track_name_needs_two_clicks_to_start_editing() {
        let track_id = vibez_core::id::TrackId::new();
        let name = LauncherTrackName {
            track_id,
            name: "Bass",
            color: Color::WHITE,
        };
        let bounds = Rectangle::new(Point::ORIGIN, Size::new(120.0, 20.0));
        let cursor = mouse::Cursor::Available(Point::new(10.0, 10.0));
        let mut state = Default::default();
        let click = || canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left));
        assert!(name.update(&mut state, click(), bounds, cursor).1.is_none());
        assert!(
            matches!(name.update(&mut state, click(), bounds, cursor).1, Some(Message::View(crate::domains::view::ViewMsg::StartEditingTrackName { track_id: id, .. })) if id == track_id)
        );
    }
}

#[cfg(test)]
mod empty_cell_tests {
    use super::*;
    use crate::domains::perform::{ClipMsg, PerformMsg};
    use iced::widget::canvas::Program;

    #[test]
    fn empty_cell_creation_requires_a_double_click() {
        let track = vibez_core::id::TrackId::new();
        let slot = ClipSlot {
            clip: None,
            cell: (track, 2),
            drag_source: None,
            empty_action: Message::Perform(PerformMsg::Clips(ClipMsg::CreateMidi {
                track_id: track,
                row: 2,
            })),
            color: Color::WHITE,
            key: "A",
            selected: false,
            recording: false,
            audio_recording: None,
            playing: false,
            progress: None,
            queued: false,
            compact: false,
        };
        let mut state = ClipSlotState::default();
        let bounds = Rectangle::new(Point::ORIGIN, Size::new(150.0, 72.0));
        let cursor = mouse::Cursor::Available(Point::new(70.0, 35.0));
        let press = canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left));
        let (status, message) = slot.update(&mut state, press.clone(), bounds, cursor);
        assert_eq!(status, canvas::event::Status::Ignored);
        assert!(message.is_none());
        let (status, message) = slot.update(&mut state, press, bounds, cursor);
        assert_eq!(status, canvas::event::Status::Captured);
        assert!(
            matches!(message, Some(Message::Perform(PerformMsg::Clips(ClipMsg::CreateMidi {track_id, row: 2}))) if track_id == track)
        );
    }
}

#[cfg(test)]
mod drag_tests {
    use super::*;
    use crate::domains::perform::{ClipMsg, PerformMsg};
    use iced::widget::canvas::Program;
    use vibez_core::id::{ClipId, TrackId};

    fn slot(
        clip: Option<&LauncherClip>,
        cell: (TrackId, u32),
        drag_source: Option<ClipId>,
    ) -> ClipSlot<'_> {
        ClipSlot {
            clip,
            cell,
            drag_source,
            empty_action: Message::Perform(PerformMsg::Clips(ClipMsg::StopAll)),
            color: Color::WHITE,
            key: "",
            selected: false,
            recording: false,
            audio_recording: None,
            playing: false,
            progress: None,
            queued: false,
            compact: false,
        }
    }

    fn mouse(event: mouse::Event) -> canvas::Event {
        canvas::Event::Mouse(event)
    }

    #[test]
    fn dragging_a_clip_past_a_few_pixels_starts_a_drag_that_release_ends() {
        let track = TrackId::new();
        let clip = crate::domains::perform::clip_record::empty_midi_clip(
            ClipId::new(),
            track,
            0,
            "Pattern 1".into(),
            4.0,
        );
        let source = slot(Some(&clip), (track, 0), None);
        let mut state = ClipSlotState::default();
        let bounds = Rectangle::new(Point::ORIGIN, Size::new(150.0, 72.0));
        let at = |x, y| mouse::Cursor::Available(Point::new(x, y));
        let press = mouse(mouse::Event::ButtonPressed(mouse::Button::Left));
        source.update(&mut state, press, bounds, at(70.0, 40.0));
        let nudge = mouse(mouse::Event::CursorMoved {
            position: Point::new(72.0, 41.0),
        });
        assert!(source
            .update(&mut state, nudge, bounds, at(72.0, 41.0))
            .1
            .is_none());
        let away = mouse(mouse::Event::CursorMoved {
            position: Point::new(200.0, 40.0),
        });
        assert!(matches!(
            source.update(&mut state, away, bounds, at(200.0, 40.0)).1,
            Some(Message::Perform(PerformMsg::Clips(ClipMsg::BeginDrag(id)))) if id == clip.id
        ));
        let release = mouse(mouse::Event::ButtonReleased(mouse::Button::Left));
        assert!(matches!(
            source
                .update(&mut state, release, bounds, at(200.0, 40.0))
                .1,
            Some(Message::Perform(PerformMsg::Clips(ClipMsg::EndDrag)))
        ));
    }

    #[test]
    fn releasing_a_drag_over_another_cell_drops_there_and_command_copies() {
        let (track, dragged) = (TrackId::new(), ClipId::new());
        let bounds = Rectangle::new(Point::ORIGIN, Size::new(150.0, 72.0));
        let inside = mouse::Cursor::Available(Point::new(70.0, 40.0));
        let release = mouse(mouse::Event::ButtonReleased(mouse::Button::Left));
        let command = if crate::app::ON_MACOS {
            iced::keyboard::Modifiers::LOGO
        } else {
            iced::keyboard::Modifiers::CTRL
        };
        for (modifiers, copy) in [(iced::keyboard::Modifiers::empty(), false), (command, true)] {
            let target = slot(None, (track, 2), Some(dragged));
            let mut state = ClipSlotState::default();
            target.update(
                &mut state,
                canvas::Event::Keyboard(iced::keyboard::Event::ModifiersChanged(modifiers)),
                bounds,
                inside,
            );
            let dropped = target.update(&mut state, release.clone(), bounds, inside).1;
            assert!(
                matches!(
                    dropped,
                    Some(Message::Perform(PerformMsg::Clips(ClipMsg::Drop {
                        clip,
                        track_id,
                        row: 2,
                        copy: copied,
                    }))) if clip == dragged && track_id == track && copied == copy
                ),
                "{modifiers:?} drops with copy {copy}"
            );
        }
    }
}
