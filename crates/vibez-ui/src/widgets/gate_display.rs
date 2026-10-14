//! Draws measured detector windows, effective thresholds, phase presence and wet attenuation.

use crate::{
    domains::gate_history::{GateWindow, HISTORY_WINDOWS},
    message::Message,
    state::{UiEffect, UndoGestureId},
    theme as th,
};
use iced::{mouse, widget::canvas, Color, Point, Rectangle, Renderer, Size, Theme};
use vibez_core::id::TrackId;
use vibez_dsp::gate::param;

pub struct EnvelopeHistory<'a> {
    pub track: TrackId,
    pub effect: &'a UiEffect,
    pub windows: Vec<GateWindow>,
    pub accent: Color,
}

#[derive(Default)]
pub struct HistoryState {
    gesture: Option<UndoGestureId>,
}

fn plot(bounds: Rectangle) -> Rectangle {
    Rectangle { x: 25.0, y: 4.0, width: (bounds.width - 33.0).max(1.0), height: (bounds.height - 34.0).max(1.0) }
}

fn y_of(db: f32, area: Rectangle) -> f32 {
    area.y + (-db.clamp(-96.0, 0.0) / 96.0) * area.height
}

fn threshold_at(y: f32, area: Rectangle) -> f32 {
    (-96.0 * (y - area.y) / area.height).clamp(-80.0, 0.0)
}

impl canvas::Program<Message> for EnvelopeHistory<'_> {
    type State = HistoryState;

    fn draw(&self, _: &HistoryState, renderer: &Renderer, _: &Theme, bounds: Rectangle, _: mouse::Cursor) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        frame.fill_rectangle(Point::ORIGIN, bounds.size(), th::bg_dark());
        frame.stroke(&canvas::Path::rectangle(Point::ORIGIN, bounds.size()), canvas::Stroke::default().with_color(th::border()));
        let area = plot(bounds);
        for db in [0.0, -24.0, -48.0, -72.0, -96.0] {
            let y = y_of(db, area);
            frame.stroke(&canvas::Path::line(Point::new(area.x, y), Point::new(area.x + area.width, y)),
                canvas::Stroke::default().with_color(th::divider()));
            frame.fill_text(canvas::Text {
                content: format!("{db:.0}"), position: Point::new(area.x - 4.0, y), color: th::text_dim(), size: 8.0.into(),
                horizontal_alignment: iced::alignment::Horizontal::Right,
                vertical_alignment: if db == 0.0 { iced::alignment::Vertical::Top } else if db == -96.0 { iced::alignment::Vertical::Bottom } else { iced::alignment::Vertical::Center },
                ..Default::default()
            });
        }
        for index in 0..=4 {
            let x = area.x + area.width * index as f32 / 4.0;
            frame.stroke(&canvas::Path::line(Point::new(x, area.y), Point::new(x, area.y + area.height)),
                canvas::Stroke::default().with_color(th::divider()));
            frame.fill_text(canvas::Text {
                content: if index == 4 { "0 s".into() } else { format!("-{} s", 4 - index) },
                position: Point::new(x, bounds.height - 2.0), color: th::text_dim(), size: 8.0.into(),
                horizontal_alignment: if index == 0 { iced::alignment::Horizontal::Left } else if index == 4 { iced::alignment::Horizontal::Right } else { iced::alignment::Horizontal::Center },
                vertical_alignment: iced::alignment::Vertical::Bottom, ..Default::default()
            });
        }
        let width = area.width / HISTORY_WINDOWS as f32;
        let start = area.x + area.width - width * self.windows.len() as f32;
        for (index, window) in self.windows.iter().enumerate() {
            let x = start + index as f32 * width;
            let bottom = area.y + area.height;
            let attenuation = window.reduction_db.clamp(0.0, 96.0) / 96.0 * area.height;
            frame.fill_rectangle(Point::new(x, bottom - attenuation), Size::new(width, attenuation), th::with_alpha(self.accent, 0.10));
            for (row, bit, color) in [(0, 4, self.accent), (1, 2, th::with_alpha(self.accent, 0.55)), (2, 1, th::text_muted())] {
                if window.gate.phases_seen & bit != 0 {
                    frame.fill_rectangle(Point::new(x, bottom + 3.0 + row as f32 * 2.0), Size::new(width, 2.0), color);
                }
            }
        }
        for (kind, color, line_width) in [(0, th::text_dim(), 1.2), (1, self.accent, 1.5),
            (2, th::with_alpha(self.accent, 0.5), 1.0), (3, th::with_alpha(self.accent, 0.35), 1.0)] {
            let line = canvas::Path::new(|path| {
                for (index, window) in self.windows.iter().enumerate() {
                    let db = match kind { 0 => window.gate.peak_envelope_db, 1 => window.gate.open_threshold_db,
                        2 => window.gate.close_threshold_db, _ => -window.reduction_db };
                    let x = start + index as f32 * width;
                    let y = y_of(db, area);
                    if index == 0 { path.move_to(Point::new(x, y)); } else { path.line_to(Point::new(x, y)); }
                    path.line_to(Point::new(x + width, y));
                }
            });
            frame.stroke(&line, canvas::Stroke::default().with_color(color).with_width(line_width));
        }
        if let Some(last) = self.windows.last() {
            frame.fill_text(canvas::Text {
                content: format!("Open {:.1} dB", last.gate.open_threshold_db),
                position: Point::new(area.x + 4.0, (y_of(last.gate.open_threshold_db, area) - 3.0).max(area.y + 10.0)),
                color: self.accent, size: 9.0.into(), vertical_alignment: iced::alignment::Vertical::Bottom, ..Default::default()
            });
            if last.gate.close_threshold_db < last.gate.open_threshold_db {
                frame.fill_text(canvas::Text {
                    content: format!("Close {:.1} dB", last.gate.close_threshold_db),
                    position: Point::new(area.x + area.width - 3.0, (y_of(last.gate.close_threshold_db, area) + 3.0).min(area.y + area.height - 9.0)),
                    color: th::text_dim(), size: 8.0.into(), horizontal_alignment: iced::alignment::Horizontal::Right, ..Default::default()
                });
            }
        } else {
            let threshold = self.effect.params.get(param::THRESHOLD).copied().unwrap_or(self.effect.descriptors[param::THRESHOLD].default);
            let y = y_of(threshold, area);
            frame.stroke(&canvas::Path::line(Point::new(area.x, y), Point::new(area.x + area.width, y)),
                canvas::Stroke::default().with_color(th::with_alpha(self.accent, 0.5)));
            frame.fill_text(canvas::Text { content: format!("Manual threshold {threshold:.1} dB · no detector windows"),
                position: Point::new(area.x + 4.0, area.y + 4.0), color: th::text_dim(), size: 8.0.into(), ..Default::default() });
        }
        vec![frame.into_geometry()]
    }

    fn update(&self, state: &mut HistoryState, event: canvas::Event, bounds: Rectangle, cursor: mouse::Cursor) -> (canvas::event::Status, Option<Message>) {
        match event {
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) if cursor.position_in(bounds).is_some() => {
                state.gesture = Some(UndoGestureId::new());
                return (canvas::event::Status::Captured, None);
            },
            canvas::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) if state.gesture.take().is_some() => {
                return (canvas::event::Status::Captured, None);
            },
            canvas::Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                if let (Some(gesture), Some(cursor)) = (state.gesture, cursor.position()) {
                    return (canvas::event::Status::Captured, Some(Message::set_effect_param(self.track, self.effect.id, param::THRESHOLD,
                        threshold_at(cursor.y - bounds.y, plot(bounds))).in_undo_gesture(gesture)));
                }
            },
            _ => {},
        }
        (canvas::event::Status::Ignored, None)
    }

    fn mouse_interaction(&self, state: &HistoryState, bounds: Rectangle, cursor: mouse::Cursor) -> mouse::Interaction {
        if state.gesture.is_some() { mouse::Interaction::Grabbing } else if cursor.position_in(bounds).is_some() { mouse::Interaction::Grab }
        else { mouse::Interaction::default() }
    }
}
