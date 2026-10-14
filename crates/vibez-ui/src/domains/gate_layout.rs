//! Fits Gate history, core controls and progressive detail into measured device bounds.

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
