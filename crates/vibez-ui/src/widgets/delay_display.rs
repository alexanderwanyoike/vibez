//! Draws production-kernel wet-reference peaks separately from live Delay movement.

use iced::{mouse, widget::canvas, Color, Point, Rectangle, Renderer, Theme};
use vibez_core::{delay_state::DelayState, level::amplitude_to_db};
use vibez_dsp::delay::param;
use crate::{domains::{delay_layout::Graph, delay_response::DelayResponse}, message::Message,
    theme as th, widgets::level_meter::{self, scale_position}};

pub struct DelayDisplay<'a> {
    pub response: Option<&'a DelayResponse>,
    pub snapshot: Option<&'a DelayState>,
    pub layout: Graph,
    pub accent: Color,
    pub status: &'a str,
}

fn label(frame: &mut canvas::Frame, value: String, position: Point, color: Color) {
    frame.fill_text(canvas::Text { content: value, position, color, size: 8.0.into(),
        ..Default::default() });
}

impl canvas::Program<Message> for DelayDisplay<'_> {
    type State = ();

    fn draw(&self, _: &(), renderer: &Renderer, _: &Theme, bounds: Rectangle,
        _: mouse::Cursor) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let top = self.layout.plot.y - self.layout.bounds.y;
        let plot = Rectangle { x: 27.0, y: top, width: (bounds.width - 31.0).max(1.0),
            height: self.layout.plot.height };
        let bottom = plot.y + plot.height;
        frame.fill_rectangle(Point::new(plot.x, plot.y), plot.size(), th::bg_dark());
        label(&mut frame, "Reference wet: unit L impulse, pre-Mix, Duck bypassed".into(),
            Point::ORIGIN, th::text_dim());
        let y = |db| plot.y + plot.height * scale_position(db);
        for db in level_meter::scale_marks(plot.height) {
            let at = y(*db);
            frame.stroke(&canvas::Path::line(Point::new(plot.x, at),
                Point::new(plot.x + plot.width, at)),
                canvas::Stroke::default().with_color(th::divider()));
            level_meter::draw_scale_label(&mut frame, *db, Point::new(plot.x - 3.0, at));
        }
        let snapshot = self.snapshot;
        let response = self.response.filter(|_| snapshot.is_some_and(|s| !s.reference_moving));
        let seconds = response.map(|r| (r.frames.saturating_sub(1)) as f64 / f64::from(r.sample_rate))
            .or_else(|| snapshot.map(|s| 4.0 * s.lines.iter().map(|line|
                line.old_samples.max(line.destination_samples).max(line.target_samples))
                .fold(1.0_f64, f64::max) / f64::from(s.sample_rate))).unwrap_or(1.0);
        let beats = snapshot.filter(|s| s.targets[param::SYNC] == 1.0).map(|s| seconds * s.bpm / 60.0);
        for tick in 0..=4 {
            let fraction = tick as f32 / 4.0;
            let x = plot.x + fraction * plot.width;
            frame.stroke(&canvas::Path::line(Point::new(x, plot.y), Point::new(x, bottom)),
                canvas::Stroke::default().with_color(th::divider()));
            frame.fill_text(canvas::Text {
                content: format!("{:.2}{}", beats.unwrap_or(seconds) * f64::from(fraction),
                    if tick == 4 { if beats.is_some() { " beats" } else { " s" } } else { "" }),
                position: Point::new(x, self.layout.ruler.y - self.layout.bounds.y),
                color: th::text_dim(), size: 8.0.into(),
                horizontal_alignment: if tick == 4 { iced::alignment::Horizontal::Right }
                    else { iced::alignment::Horizontal::Left }, ..Default::default()
            });
        }
        if let Some(response) = response {
            let endpoint = response.frames.saturating_sub(1).max(1) as f32;
            for (index, bin) in response.bins.iter().enumerate() {
                for (peak, at, right) in [(bin.left, bin.left_frame, false),
                    (bin.right, bin.right_frame, true)] {
                    if peak <= 0.0 || at == u32::MAX { continue; }
                    let x = plot.x + plot.width * (at as f32 / endpoint);
                    let peak_y = y(amplitude_to_db(peak));
                    let start = index as u64 * response.frames / response.bins.len() as u64;
                    let end = (index + 1) as u64 * response.frames / response.bins.len() as u64;
                    let bin_x = plot.x + plot.width * (start as f32 / endpoint);
                    let bin_width = plot.width * (end.saturating_sub(start + 1)) as f32 / endpoint;
                    frame.fill_rectangle(Point::new(bin_x, peak_y),
                        iced::Size::new(bin_width, bottom - peak_y), th::with_alpha(self.accent, 0.035));
                    if right {
                        // Dashed R stems distinguish coincident channels without inventing a second colour.
                        let mut at = peak_y;
                        while at < bottom {
                            frame.stroke(&canvas::Path::line(Point::new(x, at),
                                Point::new(x, (at + 3.0).min(bottom))),
                                canvas::Stroke::default().with_color(th::with_alpha(self.accent, 0.65)));
                            at += 5.0;
                        }
                        frame.stroke(&canvas::Path::circle(Point::new(x, peak_y), 2.0),
                            canvas::Stroke::default().with_color(self.accent));
                    } else {
                        frame.stroke(&canvas::Path::line(Point::new(x, peak_y), Point::new(x, bottom)),
                            canvas::Stroke::default().with_color(self.accent));
                        frame.fill(&canvas::Path::circle(Point::new(x, peak_y), 1.8), self.accent);
                    }
                }
            }
            label(&mut frame, "L solid / R dashed; four-repeat window".into(),
                Point::new(plot.x + 5.0, plot.y + 3.0), th::text_dim());
        } else if let Some(state) = snapshot.filter(|s| s.reference_moving) {
            label(&mut frame, self.status.into(), Point::new(plot.x + 5.0, plot.y + 3.0), th::text_dim());
            for (index, line) in state.lines.iter().enumerate().filter(|(_, line)| line.moving) {
                for samples in [line.old_samples, line.destination_samples] {
                    let x = plot.x + plot.width * (samples / f64::from(state.sample_rate) / seconds) as f32;
                    frame.stroke(&canvas::Path::line(Point::new(x, plot.y), Point::new(x, bottom)),
                        canvas::Stroke::default().with_color(th::with_alpha(self.accent, 0.5)));
                }
                label(&mut frame, format!("{} {}: {:.1} ms remaining", if index == 0 { "L" } else { "R" },
                    if line.gliding { "Pitch glide" } else { "Crossfade" },
                    line.remaining_frames as f64 * 1000.0 / f64::from(state.sample_rate)),
                    Point::new(plot.x + 5.0, plot.y + 12.0 + index as f32 * 10.4), self.accent);
            }
        } else {
            label(&mut frame, self.status.into(), Point::new(plot.x + 5.0, plot.y + 4.0), th::text_dim());
        }
        vec![frame.into_geometry()]
    }
}
