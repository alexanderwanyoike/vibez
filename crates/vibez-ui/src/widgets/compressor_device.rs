//! Arranges native Compressor controls and measured dynamics displays by available size.

use iced::Size;

pub const COMPACT_W: f32 = 430.0;
pub const MEDIUM_W: f32 = 620.0;
pub const MAX_W: f32 = 900.0;
const TOOLBAR_H: f32 = 26.0;

#[derive(Debug, Clone, Copy)]
pub struct Arrangement {
    pub plot: f32,
    pub controls: f32,
    pub knobs: Option<f32>,
    pub primary_beside: bool,
    pub sidechain_column: bool,
    pub both_plots: bool,
}

pub fn arrange(_size: Size, _advanced: bool) -> Arrangement {
    todo!("Measured Compressor layout is not implemented")
}
pub fn card_width(_available: f32) -> f32 {
    todo!("Measured Compressor width is not implemented")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn measured_compressor_tiers_fit_actual_control_and_caption_geometry() {
        for width in [430.0, 620.0, 900.0] {
            for height in [
                64.0, 72.0, 90.0, 108.0, 113.0, 114.0, 130.0, 160.0, 240.0, 304.0, 400.0,
            ] {
                for advanced in [false, true] {
                    let layout = arrange(Size::new(width, height), advanced);
                    assert!(
                        TOOLBAR_H + layout.plot + layout.controls <= height + 0.001,
                        "{width}x{height} Advanced={advanced}"
                    );
                    let field = 8.0 * 1.3
                        + 3.0
                        + 10.0 * 1.3
                        + 2.0
                        + layout.knobs.map_or(0.0, |size| size + 3.0);
                    assert!(
                        layout.controls >= field,
                        "controls clip at {width}x{height}"
                    );
                    assert!(layout.controls >= 36.0);
                    let caption = 8.0 * 1.3 + 3.0;
                    assert!(
                        layout.plot == 0.0 || layout.plot >= 4.0 + caption + 32.0,
                        "no miniature plots"
                    );
                    assert_eq!(layout.primary_beside, advanced && width >= 820.0);
                    assert!(!layout.sidechain_column || width >= 820.0 && layout.plot >= 150.0);
                    assert!(!layout.both_plots || width >= MEDIUM_W);
                }
            }
        }
    }
    #[test]
    fn compressor_width_uses_the_chain_remainder_with_readable_bounds() {
        assert_eq!(card_width(80.0), COMPACT_W);
        assert_eq!(card_width(710.0), 710.0);
        assert_eq!(card_width(1800.0), MAX_W);
    }
}
