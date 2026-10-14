//! Fits Gate history, core controls and progressive detail into measured device bounds.

pub const COMPACT_WIDTH: f32 = 430.0;
pub const WIDE_WIDTH: f32 = 740.0;
pub const MAX_WIDTH: f32 = 900.0;
pub const SOURCE_WIDTH: f32 = 120.0;
pub const OUTPUT_WIDTH: f32 = 64.0;
pub const ASIDE_WIDTH: f32 = 300.0;
pub const INLINE_CORE_WIDTH: f32 = 224.0;

#[derive(Clone, Copy, Debug)]
pub struct Arrangement {
    pub toolbar: f32,
    pub plot: f32,
    pub controls: f32,
    pub details: f32,
    pub knob_size: Option<f32>,
    pub compact: bool,
    pub source_column: bool,
    pub output_column: bool,
    pub advanced_aside: bool,
    pub inline_details: bool,
    pub primary_visible: bool,
    pub primary_count: usize,
}

pub fn arrange(width: f32, height: f32, advanced: bool) -> Arrangement {
    let compact = width < WIDE_WIDTH;
    let advanced_aside = width >= 872.0 && height >= 280.0;
    let inline_details = advanced && !compact && height < 112.0;
    let (controls, knob_size) = if height >= 280.0 {
        (if advanced_aside { 88.0 } else { 84.0 }, Some(34.0))
    } else if height >= 160.0 {
        (57.0, Some(20.0))
    } else {
        (36.0, None)
    };
    let details = if advanced && !advanced_aside && height >= 112.0 {
        if height >= 280.0 { 44.0 } else { 36.0 }
    } else {
        0.0
    };
    let toolbar = 26.0;
    let available = (height - toolbar - controls - details).max(0.0);
    let plot = if available >= 84.0 { available } else { 0.0 };
    Arrangement {
        toolbar,
        plot,
        controls,
        details,
        knob_size,
        compact,
        source_column: !compact && height >= 240.0 && plot > 0.0,
        output_column: compact && height >= 240.0 && plot > 0.0,
        advanced_aside,
        inline_details,
        primary_visible: !advanced || details > 0.0 || advanced_aside || inline_details,
        primary_count: if compact || inline_details { 3 } else { 5 },
    }
}

pub fn card_width(available: f32) -> f32 {
    available.clamp(COMPACT_WIDTH, MAX_WIDTH)
}

pub fn preset_bar_width(width: f32, height: f32) -> f32 {
    width - if height < 160.0 { 180.0 } else { 8.0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measured_gate_layout_preserves_readable_core_and_usable_displays_without_scrolling() {
        for width in [430.0, 620.0, 740.0, 812.0, 900.0] {
            for height in [64.0, 80.0, 112.0, 146.0, 160.0, 240.0, 304.0] {
                for advanced in [false, true] {
                    let layout = arrange(width, height, advanced);
                    assert!(layout.toolbar + layout.plot + layout.controls + layout.details <= height + 0.001);
                    let primary_child = 8.0 * 1.3 + 3.0 + 10.0 * 1.3 + 2.0
                        + layout.knob_size.map_or(0.0, |size| size + 3.0);
                    assert!(layout.controls - 1.0 >= primary_child);
                    assert!(layout.plot == 0.0 || layout.plot >= 84.0,
                        "a named detector graph needs a usable canvas at {width}x{height}");
                    assert!(!advanced || width < 740.0 || layout.primary_visible,
                        "wide details replaced the essential gate fields at {width}x{height}");
                    assert!(!layout.source_column || width >= 740.0 && height >= 240.0);
                }
            }
        }
        let expanded = arrange(900.0, 304.0, true);
        assert!(expanded.advanced_aside && expanded.source_column);
        assert!(expanded.plot >= 180.0);
        assert_eq!(expanded.primary_count, 5);
        let compact = arrange(430.0, 304.0, false);
        assert_eq!(compact.primary_count, 3);
        assert!(compact.output_column && compact.plot >= 120.0);
        let minimum = arrange(740.0, 64.0, true);
        assert!(minimum.inline_details && minimum.primary_visible);
        assert_eq!(minimum.primary_count, 3);
    }
}
