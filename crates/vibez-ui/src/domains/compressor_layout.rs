//! Device height is a hard budget: Advanced must preserve core controls and routing access.

use super::compressor::CompressorPage;
use vibez_dsp::compressor::param;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrimaryControls {
    Core,
    Full,
    Expanded,
}

impl PrimaryControls {
    pub fn indices(self) -> &'static [usize] {
        match self {
            Self::Core => &[
                param::THRESHOLD,
                param::RATIO,
                param::ATTACK,
                param::RELEASE,
            ],
            Self::Full | Self::Expanded => &[
                param::THRESHOLD,
                param::RATIO,
                param::ATTACK,
                param::RELEASE,
                param::KNEE,
                param::MAKEUP,
                param::MIX,
            ],
        }
    }
}

pub const COMPACT_W: f32 = 430.0;
pub const MEDIUM_W: f32 = 740.0;
pub const MAX_W: f32 = 900.0;
pub const TOOLBAR_H: f32 = 26.0;
pub const SIDECHAIN_W: f32 = 120.0;
pub const GAUGE_W: f32 = 76.0;
pub const ASIDE_W: f32 = 260.0;
pub const INLINE_CORE_W: f32 = 276.0;

#[derive(Debug, Clone, Copy)]
pub struct Arrangement {
    pub plot: f32,
    pub controls: f32,
    pub knobs: Option<f32>,
    pub primary_visible_with_details: bool,
    pub sidechain_column: bool,
    pub both_plots: bool,
    pub compact: bool,
    pub expanded: bool,
    pub tabs: f32,
    pub details: f32,
    pub routing: f32,
}

pub fn arrange(width: f32, height: f32, advanced: bool) -> Arrangement {
    let compact = width < MEDIUM_W;
    let expanded = width >= 872.0 && height >= 280.0;
    let (controls, knobs) = if height >= 280.0 {
        (if expanded { 88.0 } else { 84.0 }, Some(34.0))
    } else if height >= 160.0 {
        (57.0, Some(20.0))
    } else {
        (36.0, None)
    };
    let details = if advanced && !expanded && height >= 112.0 {
        if height >= 280.0 {
            44.0
        } else {
            36.0
        }
    } else {
        0.0
    };
    let available = (height - TOOLBAR_H - controls - details).max(0.0);
    let tabs = if compact && available >= 84.0 {
        24.0
    } else {
        0.0
    };
    let sidechain_column = !compact && available >= 150.0;
    let routing = if !compact && !sidechain_column && available >= 120.0 {
        36.0
    } else {
        0.0
    };
    let available = available - tabs - routing;
    let plot = if compact {
        if tabs > 0.0 && available >= 60.0 {
            available
        } else {
            0.0
        }
    } else if available >= 84.0 {
        available
    } else {
        0.0
    };
    Arrangement {
        plot,
        controls,
        knobs,
        primary_visible_with_details: advanced && (!compact || details > 0.0),
        sidechain_column,
        both_plots: !compact && plot > 0.0,
        compact,
        expanded,
        tabs,
        details,
        routing,
    }
}

pub fn card_width(available: f32) -> f32 {
    available.clamp(COMPACT_W, MAX_W)
}

pub fn preset_bar_width(width: f32, height: f32) -> f32 {
    width - if height < 160.0 { 190.0 } else { 8.0 }
}

pub fn plot_widths(width: f32, layout: Arrangement) -> (f32, f32) {
    let source = if layout.sidechain_column {
        SIDECHAIN_W + 8.0
    } else {
        0.0
    };
    let available = (width - 8.0 - source - GAUGE_W - 8.0).max(32.0);
    if layout.both_plots {
        let available = available - 8.0;
        (available * 2.0 / 3.0, available / 3.0)
    } else {
        (available, 0.0)
    }
}

impl Arrangement {
    pub fn inline_details(self) -> bool {
        self.primary_visible_with_details && !self.expanded && self.details == 0.0
    }

    pub fn primary_controls(self) -> PrimaryControls {
        if self.compact || self.inline_details() {
            PrimaryControls::Core
        } else if self.expanded {
            PrimaryControls::Expanded
        } else {
            PrimaryControls::Full
        }
    }
}

pub const ASIDE_PAGES: [CompressorPage; 4] = [
    CompressorPage::Dynamics,
    CompressorPage::Detector,
    CompressorPage::Range,
    CompressorPage::Metering,
];

pub fn aside_page(selected: CompressorPage) -> CompressorPage {
    if ASIDE_PAGES.contains(&selected) {
        selected
    } else {
        CompressorPage::Dynamics
    }
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
                    let layout = arrange(width, height, advanced);
                    assert!(
                        TOOLBAR_H
                            + layout.plot
                            + layout.controls
                            + layout.tabs
                            + layout.details
                            + layout.routing
                            <= height + 0.001,
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
                    assert_eq!(
                        layout.primary_visible_with_details,
                        advanced && (height >= 112.0 || width >= MEDIUM_W)
                    );
                    assert!(!layout.sidechain_column || width >= MEDIUM_W && layout.plot >= 150.0);
                    assert!(!layout.both_plots || width >= MEDIUM_W);
                    if layout.plot > 0.0 {
                        assert!(
                            layout.plot - 4.0 - (8.0 * 1.3 + 9.0 * 1.3 + 6.0) >= 18.0,
                            "GR gauge loses its usable height"
                        );
                    }
                    if layout.plot >= 100.0 {
                        assert!(
                            layout.plot - 4.0 - (3.0 * 8.0 * 1.3 + 6.0) >= 32.0,
                            "signal meter loses its usable height"
                        );
                    }
                    if layout.sidechain_column {
                        assert!(
                            layout.plot - 4.0
                                >= 2.0 * (8.0 * 1.3 + 3.0 + 10.0 * 1.3 + 2.0)
                                    + 9.0 * 1.3
                                    + 4.0
                                    + 2.0 * 8.0 * 1.3
                                    + 20.0
                        );
                    }
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

    #[test]
    fn sidechain_column_reserves_source_tap_input_disconnect_warning_and_raw_peak() {
        let field = 8.0 * 1.3 + 3.0 + 10.0 * 1.3 + 2.0;
        let disconnect = 9.0 * 1.3 + 4.0;
        let warnings_and_raw_peak = 2.0 * 8.0 * 1.3;
        let four_gaps = 4.0 * 5.0;
        let child_content = 2.0 * field + disconnect + warnings_and_raw_peak + four_gaps;
        for height in [251.0, 252.0, 253.0, 254.0, 260.0, 262.0] {
            let layout = arrange(900.0, height, true);
            assert!(
                !layout.sidechain_column || layout.plot - 4.0 >= child_content,
                "sidechain fields clip at body height {height}: {} available for {child_content}",
                layout.plot - 4.0
            );
        }
    }

    #[test]
    fn advanced_preserves_primary_controls_at_the_actual_maximum_body_in_every_tier() {
        for width in [430.0, 472.0, 740.0, 804.0, 872.0, 892.0] {
            let layout = arrange(width, 304.0, true);
            assert!(
                layout.primary_visible_with_details,
                "Advanced replaces the core controls at width {width}"
            );
            let child_height = 8.0 * 1.3 + 3.0 + 34.0 + 3.0 + 10.0 * 1.3 + 2.0;
            assert!(
                layout.controls >= child_height + 4.0,
                "core knobs and values clip at width {width}"
            );
            assert!(
                layout.plot >= 100.0,
                "the defining visual disappears at {width}"
            );
        }
    }

    #[test]
    fn medium_keeps_sidechain_source_visible_before_opening_details() {
        for width in [740.0, 804.0, 820.0, 871.0] {
            let layout = arrange(width, 304.0, false);
            assert!(
                layout.sidechain_column,
                "sidechain hidden at medium width {width}"
            );
            assert!(
                layout.plot >= 150.0,
                "sidechain has no real vertical budget"
            );
        }
    }

    #[test]
    fn composition_boundaries_keep_a_landscape_transfer_and_a_supporting_history() {
        for width in [430.0, 472.0, 739.0, 740.0, 804.0, 871.0, 872.0, 892.0] {
            for advanced in [false, true] {
                let layout = arrange(width, 304.0, advanced);
                let (transfer, history) = plot_widths(width, layout);
                if width < MEDIUM_W {
                    assert_eq!(layout.tabs, 24.0);
                    assert_eq!(history, 0.0);
                    assert!(transfer >= 320.0);
                } else {
                    assert!(layout.sidechain_column);
                    assert!(transfer >= 320.0, "transfer too narrow at {width}");
                    assert!(history >= 150.0, "history too narrow at {width}");
                    assert!((transfer - history * 2.0).abs() < 0.001);
                }
                assert_eq!(layout.expanded, width >= 872.0);
                let child = 8.0 * 1.3 + 3.0 + 10.0 * 1.3 + 2.0;
                if advanced && !layout.expanded {
                    assert!(layout.details - 6.0 >= child);
                }
            }
        }
    }

    #[test]
    fn primary_children_fit_after_separator_and_container_padding_at_intermediate_heights() {
        for height in [160.0, 180.0, 219.0, 240.0, 279.0, 280.0, 304.0] {
            for width in [430.0, 740.0, 872.0] {
                for advanced in [false, true] {
                    let layout = arrange(width, height, advanced);
                    let child = 8.0 * 1.3
                        + 3.0
                        + 10.0 * 1.3
                        + 2.0
                        + layout.knobs.map_or(0.0, |knob| knob + 3.0);
                    let available = layout.controls - 1.0 - 2.0 * 2.0;
                    assert!(available >= child,
                        "primary child needs {child}px but only {available}px remains at {width}x{height}");
                }
            }
        }
    }

    #[test]
    fn every_visible_compact_plot_keeps_transfer_and_history_selectable() {
        for width in [430.0, 472.0, 739.0] {
            for height in 64..305 {
                for advanced in [false, true] {
                    let layout = arrange(width, height as f32, advanced);
                    if layout.plot > 0.0 {
                        assert!(layout.tabs >= 24.0,
                            "Transfer/History inaccessible at {width}x{height}, Advanced={advanced}");
                    }
                }
            }
        }
    }

    #[test]
    fn expanded_details_keep_a_sidechain_route_or_the_page_selector_at_every_height() {
        for width in [872.0, 892.0, 900.0] {
            for height in 112..305 {
                let layout = arrange(width, height as f32, true);
                let sidechain_visible = layout.sidechain_column || layout.routing > 0.0;
                let details_pages = !layout.expanded;
                assert!(
                    sidechain_visible || details_pages,
                    "source/tap/disconnect unreachable at {width}x{height}"
                );
            }
        }
    }

    #[test]
    fn expanded_advanced_shares_the_primary_strip_instead_of_adding_a_generic_row() {
        for width in [872.0, 892.0, 900.0] {
            let layout = arrange(width, 304.0, true);
            assert_eq!(
                layout.details, 0.0,
                "Advanced became a generic row at {width}"
            );
            assert!(layout.primary_visible_with_details);
            assert!(
                layout.plot >= 180.0,
                "the aside displaced the dominant graph"
            );
            let main_width = width - 8.0 - ASIDE_W - 1.0 - 2.0 * 12.0;
            let parameter_width = (main_width - 44.0 - 7.0 * 6.0) / 7.0;
            assert!(
                parameter_width >= 68.0,
                "the aside squeezed core values at {width}"
            );
            let header = 9.0 * 1.3 + 6.0;
            let reading = 8.0 * 1.3 + 3.0 + 10.0 * 1.3;
            let two_meter_rows = 2.0 * reading + 4.0;
            assert!(
                layout.controls - 5.0 >= header + 5.0 + two_meter_rows,
                "the aside clips its real header and metering rows"
            );
            let detector_frequency_width = 9.0 * 1.3 + 8.0 + 54.0 + 8.0 * 1.3 + 8.0;
            assert!(106.0 >= detector_frequency_width);
            const { assert!((ASIDE_W - 106.0 - 16.0) / 2.0 >= 68.0) };
        }
    }

    #[test]
    fn wide_minimum_advanced_keeps_all_four_core_parameters_beside_every_detail_page() {
        for width in [740.0, 804.0, 871.0, 872.0, 892.0, 900.0] {
            for height in 64..112 {
                let layout = arrange(width, height as f32, true);
                assert!(
                    layout.primary_visible_with_details,
                    "Advanced replaced Threshold/Ratio/Attack/Release at {width}x{height}"
                );
                assert_eq!(layout.primary_controls(), PrimaryControls::Core);
                assert_eq!(
                    layout.primary_controls().indices(),
                    &[
                        param::THRESHOLD,
                        param::RATIO,
                        param::ATTACK,
                        param::RELEASE
                    ]
                );
                assert_eq!(layout.knobs, None);
                let field_height = 8.0 * 1.3 + 3.0 + 10.0 * 1.3 + 2.0;
                let content_height = layout.controls - 5.0;
                assert!(content_height >= field_height);
                let core_width = INLINE_CORE_W;
                assert!((core_width - 3.0 * 6.0) / 4.0 >= 62.0);
                let detail_controls_width = width - 8.0 - core_width - 17.0 - 88.0 - 8.0;
                for page in CompressorPage::ALL {
                    let minimum_width = match page {
                        CompressorPage::Sound => 3.0 * 62.0 + 2.0 * 8.0,
                        CompressorPage::Dynamics => 3.0 * 92.0 + 2.0 * 6.0,
                        CompressorPage::Detector => 2.0 * 62.0 + 22.0 + 22.0 + 47.0 + 4.0 * 6.0,
                        CompressorPage::Range => 46.0 + 62.0 + 8.0,
                        CompressorPage::Sidechain => 96.0 + 76.0 + 62.0 + 82.0 + 3.0 * 6.0,
                        CompressorPage::Metering => 8.0 * 36.0 + 7.0 * 4.0,
                    };
                    assert!(
                        detail_controls_width >= minimum_width,
                        "{page} clips beside the core bank at {width}x{height}"
                    );
                    assert!(
                        content_height >= field_height,
                        "{page} clips its label and selector at {width}x{height}"
                    );
                }
            }
        }
        let compact = arrange(430.0, 64.0, true);
        assert!(!compact.primary_visible_with_details);
    }
}
