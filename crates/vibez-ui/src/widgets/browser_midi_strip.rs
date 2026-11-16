//! The selected MIDI file's notes as a compact piano-roll strip in the Browser Audition footer.

use std::cell::Cell;

use iced::mouse;
use iced::widget::canvas;
use iced::{Rectangle, Renderer, Theme};

use crate::domains::midi_audition::MidiStrip;
use crate::message::Message;
use crate::note_rows::note_alpha;
use crate::theme;

use super::playhead::playhead_geometry;

/// A note stays visible however short it is.
const MIN_NOTE_WIDTH: f32 = 1.5;
const VERTICAL_INSET: f32 = 2.0;

pub struct BrowserMidiStrip {
    pub strip: Option<MidiStrip>,
    /// Changes whenever a different file's strip is shown.
    pub identity: u64,
    pub playhead_fraction: Option<f32>,
}

pub struct BrowserMidiStripState {
    cache: canvas::Cache,
    /// (strip identity, theme epoch): a palette swap repaints the strip too.
    fingerprint: Cell<(u64, u64)>,
}

impl Default for BrowserMidiStripState {
    fn default() -> Self {
        Self {
            cache: canvas::Cache::new(),
            fingerprint: Cell::new((u64::MAX, u64::MAX)),
        }
    }
}

impl canvas::Program<Message> for BrowserMidiStrip {
    type State = BrowserMidiStripState;

    fn draw(
        &self,
        state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let fingerprint = (self.identity, theme::epoch());
        if state.fingerprint.get() != fingerprint {
            state.cache.clear();
            state.fingerprint.set(fingerprint);
        }

        let geometry = state.cache.draw(renderer, bounds.size(), |frame| {
            let width = frame.width();
            let height = frame.height();
            frame.fill_rectangle(
                iced::Point::ORIGIN,
                iced::Size::new(width, height),
                theme::display_bg(),
            );
            let Some(strip) = &self.strip else {
                return;
            };
            for &line in &strip.bar_lines {
                let x = (line * width).round() + 0.5;
                frame.stroke(
                    &canvas::Path::line(iced::Point::new(x, 0.0), iced::Point::new(x, height)),
                    canvas::Stroke::default()
                        .with_color(theme::border())
                        .with_width(1.0),
                );
            }
            let plot_height = (height - 2.0 * VERTICAL_INSET).max(1.0);
            let note_height = strip.rows.note_height(plot_height);
            for note in &strip.notes {
                let x = note.start * width;
                let y = VERTICAL_INSET + note.depth * (plot_height - 3.0);
                frame.fill_rectangle(
                    iced::Point::new(x, y),
                    iced::Size::new(
                        ((note.end - note.start) * width).max(MIN_NOTE_WIDTH),
                        note_height,
                    ),
                    theme::with_alpha(theme::waveform(), note_alpha(note.velocity)),
                );
            }
        });

        let playhead_x = self
            .playhead_fraction
            .map(|fraction| fraction.clamp(0.0, 1.0) * bounds.width);
        let mut geometry = vec![geometry];
        geometry.extend(playhead_geometry(renderer, bounds, playhead_x));
        geometry
    }
}
