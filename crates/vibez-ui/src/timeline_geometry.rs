//! Shared beat/pixel geometry for every timeline editor surface.

/// Arrange's baseline horizontal scale before zoom is applied.
pub const BASE_PIXELS_PER_BEAT: f32 = 20.0;
const WHEEL_LINE_PIXELS: f32 = 40.0;
const ZOOM_SENSITIVITY: f32 = 0.003;
pub const EDGE_SCROLL_ZONE_PIXELS: f32 = 64.0;
pub const MAX_EDGE_SCROLL_BEATS_PER_SECOND: f64 = 8.0;

/// Normalise line- and pixel-based wheel input to physical-ish pixels.
pub fn wheel_delta_pixels(delta: iced::mouse::ScrollDelta) -> (f32, f32) {
    match delta {
        iced::mouse::ScrollDelta::Lines { x, y } => (x * WHEEL_LINE_PIXELS, y * WHEEL_LINE_PIXELS),
        iced::mouse::ScrollDelta::Pixels { x, y } => (x, y),
    }
}

/// Continuous zoom factor for a wheel or trackpad delta.
pub fn zoom_factor_from_pixels(pixels: f32) -> f32 {
    (pixels * ZOOM_SENSITIVITY).clamp(-0.5, 0.5).exp()
}

/// Signed edge-scroll speed for a screen-space lane viewport.
///
/// The quadratic ramp is deliberately gentle at the inner edge while still
/// reaching a fixed musical maximum at and beyond the viewport boundary.
pub fn edge_scroll_velocity(cursor_x: f32, left: f32, right: f32) -> f64 {
    if !cursor_x.is_finite() || !left.is_finite() || !right.is_finite() || right <= left {
        return 0.0;
    }
    let left_proximity =
        ((left + EDGE_SCROLL_ZONE_PIXELS - cursor_x) / EDGE_SCROLL_ZONE_PIXELS).clamp(0.0, 1.0);
    if left_proximity > 0.0 {
        return -MAX_EDGE_SCROLL_BEATS_PER_SECOND * f64::from(left_proximity * left_proximity);
    }
    let right_proximity =
        ((cursor_x - (right - EDGE_SCROLL_ZONE_PIXELS)) / EDGE_SCROLL_ZONE_PIXELS).clamp(0.0, 1.0);
    MAX_EDGE_SCROLL_BEATS_PER_SECOND * f64::from(right_proximity * right_proximity)
}

/// Resolve a move drag after the viewport has panned since pointer-down.
pub fn compensated_drag_beat(
    original_beat: f64,
    pointer_delta_pixels: f32,
    pixels_per_beat: f32,
    start_scroll_beats: f64,
    current_scroll_beats: f64,
) -> f64 {
    let pointer_delta_beats =
        f64::from(pointer_delta_pixels) / f64::from(pixels_per_beat.max(f32::EPSILON));
    original_beat + pointer_delta_beats + (current_scroll_beats - start_scroll_beats)
}

/// Runtime horizontal navigation state for one timeline editor.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TimelineViewport {
    pub zoom_level: f32,
    pub scroll_offset_beats: f64,
}

impl TimelineViewport {
    pub const fn new(zoom_level: f32, scroll_offset_beats: f64) -> Self {
        Self {
            zoom_level,
            scroll_offset_beats,
        }
    }

    pub fn geometry(self) -> TimelineGeometry {
        TimelineGeometry::from_zoom(self.zoom_level, self.scroll_offset_beats)
    }

    pub fn scroll_by(&mut self, delta_beats: f64, total_beats: f64, viewport_width: f32) {
        self.scroll_offset_beats = (self.scroll_offset_beats + delta_beats)
            .clamp(0.0, self.max_scroll(total_beats, viewport_width));
    }

    pub fn zoom_around(
        &mut self,
        factor: f32,
        anchor_x: f32,
        total_beats: f64,
        viewport_width: f32,
    ) {
        if !factor.is_finite() || factor <= 0.0 {
            return;
        }
        let anchor_x = anchor_x.clamp(0.0, viewport_width.max(0.0));
        let anchor_beat = self.geometry().x_to_beat(anchor_x);
        self.zoom_level = (self.zoom_level * factor).clamp(0.01, 16.0);
        self.scroll_offset_beats = anchor_beat - self.geometry().beats_for_width(anchor_x);
        self.clamp(total_beats, viewport_width);
    }

    pub fn clamp(&mut self, total_beats: f64, viewport_width: f32) {
        self.scroll_offset_beats = self
            .scroll_offset_beats
            .clamp(0.0, self.max_scroll(total_beats, viewport_width));
    }

    pub fn max_scroll(self, total_beats: f64, viewport_width: f32) -> f64 {
        (total_beats.max(0.0) - self.geometry().visible_beats(viewport_width)).max(0.0)
    }
}

impl Default for TimelineViewport {
    fn default() -> Self {
        Self::new(2.0, 0.0)
    }
}

/// A resolved horizontal timeline viewport.
///
/// Widgets may use a zoom-derived scale (Arrange) or a fitted scale (piano
/// roll), but all beat/pixel conversion goes through this value type.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TimelineGeometry {
    pixels_per_beat: f32,
    scroll_offset_beats: f64,
    origin_x: f32,
}

impl TimelineGeometry {
    pub fn new(pixels_per_beat: f32, scroll_offset_beats: f64) -> Self {
        Self {
            pixels_per_beat: pixels_per_beat.max(f32::EPSILON),
            scroll_offset_beats,
            origin_x: 0.0,
        }
    }

    pub fn from_zoom(zoom_level: f32, scroll_offset_beats: f64) -> Self {
        Self::new(BASE_PIXELS_PER_BEAT * zoom_level, scroll_offset_beats)
    }

    pub fn fitted(total_beats: f64, viewport_width: f32, origin_x: f32) -> Self {
        let usable_width = (viewport_width - origin_x).max(1.0);
        Self {
            pixels_per_beat: usable_width / total_beats.max(1.0) as f32,
            scroll_offset_beats: 0.0,
            origin_x,
        }
    }

    pub fn pixels_per_beat(self) -> f32 {
        self.pixels_per_beat
    }

    pub fn visible_beats(self, width: f32) -> f64 {
        (width - self.origin_x).max(0.0) as f64 / self.pixels_per_beat as f64
    }

    pub fn beat_to_x(self, beat: f64) -> f32 {
        self.origin_x + ((beat - self.scroll_offset_beats) * self.pixels_per_beat as f64) as f32
    }

    pub fn x_to_beat(self, x: f32) -> f64 {
        (x - self.origin_x) as f64 / self.pixels_per_beat as f64 + self.scroll_offset_beats
    }

    pub fn width_for_beats(self, beats: f64) -> f32 {
        (beats * self.pixels_per_beat as f64) as f32
    }

    pub fn beats_for_width(self, pixels: f32) -> f64 {
        pixels as f64 / self.pixels_per_beat as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zoomed_geometry_round_trips_with_scroll() {
        let geometry = TimelineGeometry::from_zoom(2.0, 8.0);
        assert_eq!(geometry.pixels_per_beat(), 40.0);
        assert_eq!(geometry.beat_to_x(10.0), 80.0);
        assert_eq!(geometry.x_to_beat(80.0), 10.0);
        assert_eq!(geometry.visible_beats(400.0), 10.0);
    }

    #[test]
    fn fitted_geometry_accounts_for_a_fixed_header() {
        let geometry = TimelineGeometry::fitted(16.0, 852.0, 52.0);
        assert_eq!(geometry.pixels_per_beat(), 50.0);
        assert_eq!(geometry.beat_to_x(4.0), 252.0);
        assert_eq!(geometry.x_to_beat(252.0), 4.0);
        assert_eq!(geometry.width_for_beats(2.0), 100.0);
    }

    #[test]
    fn pixel_wheel_zoom_is_continuous_and_reversible() {
        let one_pixel = zoom_factor_from_pixels(1.0);
        assert!(one_pixel > 1.0 && one_pixel < 1.01);
        assert!((one_pixel * zoom_factor_from_pixels(-1.0) - 1.0).abs() < 1.0e-6);
    }

    #[test]
    fn edge_scroll_velocity_is_zero_outside_the_zone_and_conservatively_bounded() {
        assert_eq!(edge_scroll_velocity(400.0, 100.0, 900.0), 0.0);
        assert_eq!(edge_scroll_velocity(200.0, 100.0, 900.0), 0.0);
        assert!(edge_scroll_velocity(890.0, 100.0, 900.0) > 0.0);
        assert!(edge_scroll_velocity(110.0, 100.0, 900.0) < 0.0);
        assert!(edge_scroll_velocity(2_000.0, 100.0, 900.0).abs() <= 8.0);
    }

    #[test]
    fn move_drag_compensates_for_viewport_motion_under_a_stationary_pointer() {
        assert_eq!(
            compensated_drag_beat(12.0, 160.0, 80.0, 4.0, 7.0),
            17.0,
            "two pointer beats plus three auto-scrolled beats must move the Clip five beats"
        );
    }

    #[test]
    fn explicit_viewport_zoom_keeps_the_pointer_on_the_same_beat() {
        let mut viewport = TimelineViewport::new(2.0, 4.0);
        let before = viewport.geometry().x_to_beat(300.0);

        viewport.zoom_around(2.0, 300.0, 64.0, 800.0);

        assert!((viewport.geometry().x_to_beat(300.0) - before).abs() < 1.0e-9);
    }

    #[test]
    fn explicit_viewport_clamps_to_the_last_full_view() {
        let mut viewport = TimelineViewport::new(2.0, 0.0);
        viewport.scroll_by(1_000.0, 64.0, 800.0);
        assert_eq!(viewport.scroll_offset_beats, 44.0);
        viewport.scroll_by(-1_000.0, 64.0, 800.0);
        assert_eq!(viewport.scroll_offset_beats, 0.0);
    }
}
