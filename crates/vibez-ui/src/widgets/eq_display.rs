//! Channel EQ display: frequency response curve over a live spectrum
//! analyser, with draggable band handles (drag sets frequency and
//! gain, scroll trims Q on the parametric mids, double-click zeroes
//! the band).

use std::time::{Duration, Instant};

use iced::mouse;
use iced::widget::canvas;
use iced::{Color, Point, Rectangle, Renderer, Theme};

use crate::message::Message;
use crate::spectrum::{freq_to_norm, norm_to_freq, DISPLAY_BINS, FLOOR_DB};
use crate::state::UndoGestureId;
use crate::theme as th;
use crate::widgets::double_click::DoubleClick;
use vibez_core::effect::ParamDescriptor;
use vibez_core::id::{EffectId, TrackId};

/// Vertical range of the response axis in dB (curve clamps to it).
const RANGE_DB: f32 = 18.0;
/// Handle hit/draw radius.
const HANDLE_R: f32 = 6.0;
/// Double-click window in milliseconds.
const DOUBLE_CLICK_MS: u64 = 300;
/// Points sampled along the response curve.
const CURVE_POINTS: usize = 128;

/// One EQ band's parameter wiring: (gain index, freq index, Q index
/// when the band has a sweepable Q).
struct Band {
    gain_i: usize,
    freq_i: usize,
    q_i: Option<usize>,
}

const BANDS: [Band; 4] = [
    Band {
        gain_i: 0,
        freq_i: 1,
        q_i: None,
    },
    Band {
        gain_i: 3,
        freq_i: 4,
        q_i: Some(5),
    },
    Band {
        gain_i: 6,
        freq_i: 7,
        q_i: Some(8),
    },
    Band {
        gain_i: 9,
        freq_i: 10,
        q_i: Some(11),
    },
];

/// Band color from the current theme, by BANDS index (LF..HF).
fn band_color(idx: usize) -> Color {
    match idx {
        0 => th::eq_lf(),
        1 => th::eq_lmf(),
        2 => th::eq_hmf(),
        _ => th::eq_hf(),
    }
}

pub struct EqDisplayWidget {
    pub track_id: TrackId,
    pub effect_id: EffectId,
    pub params: Vec<f32>,
    pub descriptors: &'static [ParamDescriptor],
    pub bypass: bool,
    pub sample_rate: f32,
    /// Smoothed analyser bins in dB (FLOOR_DB..0), log-spaced.
    pub spectrum: Vec<f32>,
    pub spectrum_active: bool,
}

#[derive(Debug, Default)]
pub struct EqDisplayState {
    drag: Option<usize>,
    undo_gesture: Option<UndoGestureId>,
    hover: Option<usize>,
    double_click: DoubleClick,
}

impl EqDisplayWidget {
    fn param(&self, i: usize) -> f32 {
        self.params
            .get(i)
            .copied()
            .unwrap_or_else(|| self.descriptors.get(i).map(|d| d.default).unwrap_or(0.0))
    }

    fn db_to_y(&self, db: f32, h: f32) -> f32 {
        let half = h / 2.0;
        half - (db.clamp(-RANGE_DB, RANGE_DB) / RANGE_DB) * (half - 8.0)
    }

    fn y_to_db(&self, y: f32, h: f32) -> f32 {
        let half = h / 2.0;
        ((half - y) / (half - 8.0) * RANGE_DB).clamp(-RANGE_DB, RANGE_DB)
    }

    fn handle_pos(&self, band: &Band, bounds: Rectangle) -> Point {
        let x = freq_to_norm(self.param(band.freq_i)) * bounds.width;
        let y = self.db_to_y(self.param(band.gain_i), bounds.height);
        Point::new(x, y)
    }

    /// Band index whose handle sits under `pos` (widget-local).
    fn band_at(&self, pos: Point, bounds: Rectangle) -> Option<usize> {
        BANDS
            .iter()
            .enumerate()
            .map(|(i, b)| (i, self.handle_pos(b, bounds)))
            .filter(|(_, hp)| {
                let (dx, dy) = (hp.x - pos.x, hp.y - pos.y);
                (dx * dx + dy * dy).sqrt() <= HANDLE_R + 3.0
            })
            .min_by(|a, b| {
                let d = |hp: &Point| (hp.x - pos.x).powi(2) + (hp.y - pos.y).powi(2);
                d(&a.1).total_cmp(&d(&b.1))
            })
            .map(|(i, _)| i)
    }

    fn drag_message(&self, band_idx: usize, pos: Point, bounds: Rectangle) -> Message {
        let band = &BANDS[band_idx];
        let freq = norm_to_freq((pos.x / bounds.width).clamp(0.0, 1.0));
        let gain = self.y_to_db(pos.y, bounds.height);
        // Descriptor clamps happen again in the domain; pre-clamp so
        // the handle never visually detaches from the cursor's band.
        let fd = &self.descriptors[band.freq_i];
        let gd = &self.descriptors[band.gain_i];
        Message::set_effect_params(
            self.track_id,
            self.effect_id,
            vec![
                (band.freq_i, freq.clamp(fd.min, fd.max)),
                (band.gain_i, gain.clamp(gd.min, gd.max)),
            ],
        )
    }
}

impl canvas::Program<Message> for EqDisplayWidget {
    type State = EqDisplayState;

    fn draw(
        &self,
        state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let (w, h) = (bounds.width, bounds.height);

        // ── Panel ──
        frame.fill(
            &canvas::Path::rounded_rectangle(Point::ORIGIN, bounds.size(), 3.0.into()),
            th::bg_dark(),
        );

        // ── Grid: decade frequency lines + dB lines ──
        let grid = Color {
            a: 0.35,
            ..th::border()
        };
        let labeled: [(f32, &str); 3] = [(100.0, "100"), (1_000.0, "1k"), (10_000.0, "10k")];
        for f in [
            30.0, 50.0, 100.0, 200.0, 300.0, 500.0, 1e3, 2e3, 3e3, 5e3, 10e3,
        ] {
            let x = freq_to_norm(f) * w;
            frame.stroke(
                &canvas::Path::line(Point::new(x, 0.0), Point::new(x, h)),
                canvas::Stroke::default().with_color(grid).with_width(1.0),
            );
        }
        for (f, label) in labeled {
            frame.fill_text(canvas::Text {
                content: label.to_string(),
                position: Point::new(freq_to_norm(f) * w + 3.0, h - 2.0),
                color: th::text_muted(),
                size: 8.0.into(),
                vertical_alignment: iced::alignment::Vertical::Bottom,
                ..canvas::Text::default()
            });
        }
        for db in [-12.0f32, -6.0, 6.0, 12.0] {
            let y = self.db_to_y(db, h);
            frame.stroke(
                &canvas::Path::line(Point::new(0.0, y), Point::new(w, y)),
                canvas::Stroke::default().with_color(grid).with_width(1.0),
            );
        }
        // Unity line, slightly brighter.
        let y0 = self.db_to_y(0.0, h);
        frame.stroke(
            &canvas::Path::line(Point::new(0.0, y0), Point::new(w, y0)),
            canvas::Stroke::default()
                .with_color(Color {
                    a: 0.8,
                    ..th::border()
                })
                .with_width(1.0),
        );

        // ── Spectrum (behind the curve) ──
        if self.spectrum_active && self.spectrum.len() == DISPLAY_BINS {
            let bin_y = |db: f32| -> f32 { h - ((db - FLOOR_DB) / -FLOOR_DB) * h };
            let path = canvas::Path::new(|b| {
                b.move_to(Point::new(0.0, h));
                for (i, &db) in self.spectrum.iter().enumerate() {
                    let x = (i as f32 + 0.5) / DISPLAY_BINS as f32 * w;
                    b.line_to(Point::new(x, bin_y(db)));
                }
                b.line_to(Point::new(w, h));
                b.close();
            });
            frame.fill(
                &path,
                Color {
                    a: 0.13,
                    ..th::text_dim()
                },
            );
            frame.stroke(
                &path,
                canvas::Stroke::default()
                    .with_color(Color {
                        a: 0.30,
                        ..th::text_dim()
                    })
                    .with_width(1.0),
            );
        }

        // ── Response curve ──
        let eq = vibez_dsp::eq::EqEffect::from_params(self.sample_rate, &self.params);
        let curve_pts: Vec<Point> = (0..CURVE_POINTS)
            .map(|i| {
                let pos = i as f32 / (CURVE_POINTS - 1) as f32;
                let db = eq.response_db(norm_to_freq(pos));
                Point::new(pos * w, self.db_to_y(db, h))
            })
            .collect();

        if !self.bypass {
            // Soft fill between the curve and unity.
            let fill = canvas::Path::new(|b| {
                b.move_to(Point::new(0.0, y0));
                for p in &curve_pts {
                    b.line_to(*p);
                }
                b.line_to(Point::new(w, y0));
                b.close();
            });
            frame.fill(
                &fill,
                Color {
                    a: 0.10,
                    ..th::accent()
                },
            );
        }

        let curve = canvas::Path::new(|b| {
            b.move_to(curve_pts[0]);
            for p in &curve_pts[1..] {
                b.line_to(*p);
            }
        });
        let (curve_color, curve_w) = if self.bypass {
            (th::text_muted(), 1.5)
        } else {
            (th::text(), 2.0)
        };
        frame.stroke(
            &curve,
            canvas::Stroke {
                line_join: canvas::LineJoin::Round,
                ..canvas::Stroke::default()
            }
            .with_color(curve_color)
            .with_width(curve_w),
        );

        // ── Band handles ──
        for (i, band) in BANDS.iter().enumerate() {
            let p = self.handle_pos(band, bounds);
            let engaged = state.drag == Some(i) || (state.drag.is_none() && state.hover == Some(i));
            let color = if self.bypass {
                th::text_muted()
            } else {
                band_color(i)
            };
            frame.fill(&canvas::Path::circle(p, HANDLE_R), color);
            frame.fill(&canvas::Path::circle(p, HANDLE_R - 2.5), th::bg_dark());
            if engaged {
                frame.stroke(
                    &canvas::Path::circle(p, HANDLE_R + 2.0),
                    canvas::Stroke::default()
                        .with_color(th::text())
                        .with_width(1.0),
                );
                // Readout: freq / gain / Q next to the handle.
                let freq = self.param(band.freq_i);
                let mut readout = format!(
                    "{}  {:+.1} dB",
                    crate::widgets::effect_knob::format_value(freq, "Hz"),
                    self.param(band.gain_i),
                );
                if let Some(q_i) = band.q_i {
                    readout.push_str(&format!("  Q {:.2}", self.param(q_i)));
                }
                let above = p.y > 24.0;
                frame.fill_text(canvas::Text {
                    content: readout,
                    position: Point::new(
                        p.x.clamp(60.0, w - 80.0),
                        if above { p.y - 12.0 } else { p.y + 12.0 },
                    ),
                    color: th::text(),
                    size: 9.0.into(),
                    horizontal_alignment: iced::alignment::Horizontal::Center,
                    vertical_alignment: if above {
                        iced::alignment::Vertical::Bottom
                    } else {
                        iced::alignment::Vertical::Top
                    },
                    ..canvas::Text::default()
                });
            }
        }

        // ── Frame border ──
        frame.stroke(
            &canvas::Path::rounded_rectangle(Point::ORIGIN, bounds.size(), 3.0.into()),
            canvas::Stroke::default()
                .with_color(th::border())
                .with_width(1.0),
        );

        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        state: &Self::State,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if state.drag.is_some() {
            mouse::Interaction::Grabbing
        } else if cursor
            .position_in(bounds)
            .and_then(|p| self.band_at(p, bounds))
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
        let local = cursor.position_in(bounds);
        match event {
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                if let Some(pos) = local {
                    if let Some(band_idx) = self.band_at(pos, bounds) {
                        if state.double_click.press(
                            Instant::now(),
                            pos,
                            Duration::from_millis(DOUBLE_CLICK_MS),
                            None,
                        ) {
                            // Double-click: zero the band's gain.
                            state.double_click.clear();
                            state.drag = None;
                            return (
                                canvas::event::Status::Captured,
                                Some(Message::set_effect_param(
                                    self.track_id,
                                    self.effect_id,
                                    BANDS[band_idx].gain_i,
                                    0.0,
                                )),
                            );
                        }
                        state.drag = Some(band_idx);
                        state.undo_gesture = Some(UndoGestureId::new());
                        return (canvas::event::Status::Captured, None);
                    }
                }
            }
            canvas::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                if state.drag.take().is_some() {
                    state.undo_gesture = None;
                    return (canvas::event::Status::Captured, None);
                }
            }
            canvas::Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                if let Some(band_idx) = state.drag {
                    // Track the cursor even slightly outside bounds.
                    if let Some(abs) = cursor.position() {
                        let pos = Point::new(abs.x - bounds.x, abs.y - bounds.y);
                        return (
                            canvas::event::Status::Captured,
                            Some(self.drag_message(band_idx, pos, bounds).in_undo_gesture(
                                *state.undo_gesture.get_or_insert_with(UndoGestureId::new),
                            )),
                        );
                    }
                }
                if let Some(pos) = local {
                    state.hover = self.band_at(pos, bounds);
                }
            }
            canvas::Event::Mouse(mouse::Event::WheelScrolled { delta }) => {
                if let Some(pos) = local {
                    if let Some(band_idx) = self.band_at(pos, bounds).or(state.drag) {
                        if let Some(q_i) = BANDS[band_idx].q_i {
                            let scroll_y = match delta {
                                mouse::ScrollDelta::Lines { y, .. } => y,
                                mouse::ScrollDelta::Pixels { y, .. } => y / 20.0,
                            };
                            let d = &self.descriptors[q_i];
                            let q = (self.param(q_i) * 1.10f32.powf(scroll_y)).clamp(d.min, d.max);
                            return (
                                canvas::event::Status::Captured,
                                Some(Message::set_effect_param(
                                    self.track_id,
                                    self.effect_id,
                                    q_i,
                                    q,
                                )),
                            );
                        }
                        // Bands without a Q swallow the scroll so the
                        // panel underneath doesn't pan.
                        return (canvas::event::Status::Captured, None);
                    }
                }
            }
            _ => {}
        }
        (canvas::event::Status::Ignored, None)
    }
}
