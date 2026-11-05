//! An Arrange lane height that is always one the track header fits in.

use serde::{Deserialize, Serialize};

/// An Arrange lane height in logical pixels. Every way of making one holds
/// it inside the range, so a hand-edited project cannot squeeze a header.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(from = "f32", into = "f32")]
pub struct TrackHeight(f32);

impl TrackHeight {
    pub const DEFAULT: Self = Self(70.0);
    /// The header lays its name, mute, solo and volume rows out in this.
    pub const MIN: Self = Self(62.0);
    pub const MAX: Self = Self(420.0);

    pub fn new(pixels: f32) -> Self {
        Self(pixels.clamp(Self::MIN.0, Self::MAX.0))
    }

    pub const fn pixels(self) -> f32 {
        self.0
    }
}

impl Default for TrackHeight {
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl From<f32> for TrackHeight {
    fn from(pixels: f32) -> Self {
        Self::new(pixels)
    }
}

impl From<TrackHeight> for f32 {
    fn from(height: TrackHeight) -> Self {
        height.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_height_is_held_inside_the_range_the_header_fits() {
        assert_eq!(TrackHeight::new(10.0), TrackHeight::MIN);
        assert_eq!(TrackHeight::new(9_000.0), TrackHeight::MAX);
        assert_eq!(TrackHeight::new(140.0).pixels(), 140.0);
    }

    #[test]
    fn a_saved_height_out_of_range_loads_inside_it() {
        let loaded: Vec<TrackHeight> = serde_json::from_str("[5.0, 140.0, 5000.0]").unwrap();

        assert_eq!(
            loaded,
            vec![TrackHeight::MIN, TrackHeight::new(140.0), TrackHeight::MAX]
        );
        assert_eq!(serde_json::to_string(&loaded[1]).unwrap(), "140.0");
    }
}
