//! Shared beat/pixel geometry for every timeline editor surface.

/// Arrange's baseline horizontal scale before zoom is applied.
pub const BASE_PIXELS_PER_BEAT: f32 = 20.0;

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
}
