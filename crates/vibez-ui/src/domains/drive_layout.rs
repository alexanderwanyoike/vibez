//! Fits Drive's split compact bank, paired plots and progressive details to measured bounds.

pub const COMPACT_WIDTH: f32 = 430.0;
pub const MEDIUM_WIDTH: f32 = 740.0;
pub const EXPANDED_WIDTH: f32 = 872.0;
pub const MAX_WIDTH: f32 = 900.0;
pub const METER_WIDTH: f32 = 52.0;
pub const ASIDE_WIDTH: f32 = 176.0;
pub const COMPACT_BANK_WIDTH: f32 = 184.0;
pub const INLINE_CORE_WIDTH: f32 = 296.0;

#[derive(Clone, Copy, Debug)]
pub struct Arrangement {
    pub toolbar: f32,
    pub body: f32,
    pub footer: f32,
    pub details: f32,
    pub transfer_width: f32,
    pub waveform_width: f32,
    pub primary_width: f32,
    pub primary_height: f32,
    pub details_width: f32,
    pub knob_size: Option<f32>,
    pub primary_rows: usize,
    pub compact_split: bool,
    pub both_plots: bool,
    pub input_meter: bool,
    pub output_meter: bool,
    pub advanced_aside: bool,
    pub inline_details: bool,
    pub primary_visible: bool,
    pub character_visible: bool,
}

pub fn arrange(width: f32, height: f32, advanced: bool) -> Arrangement {
    let compact = width < MEDIUM_WIDTH;
    let expanded = width >= EXPANDED_WIDTH;
    let toolbar = 26.0;
    let advanced_aside = advanced && expanded && height >= 280.0;
    let inline_details = advanced && !compact && height < 112.0;
    let details = if advanced && !advanced_aside && height >= 112.0 {
        if height >= 280.0 {
            44.0
        } else {
            36.0
        }
    } else {
        0.0
    };
    let compact_body = (height - toolbar - 36.0 - details).max(0.0);
    let compact_split = compact && compact_body >= 84.0;
    let (footer, knob_size) = if compact_split {
        let knob = [34.0, 20.0].into_iter().find(|knob| {
            let field = 8.0 * 1.3 + 3.0 + 10.0 * 1.3 + 2.0 + knob + 3.0;
            2.0 * field + 8.0 <= compact_body - 4.0
        });
        (36.0, knob)
    } else if height >= 280.0 {
        (84.0, Some(34.0))
    } else if height >= 160.0 {
        (57.0, Some(20.0))
    } else {
        (36.0, None)
    };
    let available = (height - toolbar - footer - details).max(0.0);
    let body = if available >= 84.0 { available } else { 0.0 };
    let both_plots = !compact && body > 0.0;
    let input_meter = expanded && body > 0.0;
    let output_meter = !compact && body > 0.0;
    let (transfer_width, waveform_width) = if compact_split {
        (width - 8.0 - 12.0 - COMPACT_BANK_WIDTH, 0.0)
    } else if both_plots {
        let input = if input_meter { METER_WIDTH + 8.0 } else { 0.0 };
        let aside = if advanced_aside {
            ASIDE_WIDTH + 8.0
        } else {
            0.0
        };
        let plots = width - 8.0 - input - METER_WIDTH - 2.0 * 8.0 - aside;
        (plots * 9.0 / 16.0, plots * 7.0 / 16.0)
    } else {
        (0.0, 0.0)
    };
    let primary_width = if compact_split {
        COMPACT_BANK_WIDTH
    } else if inline_details {
        INLINE_CORE_WIDTH
    } else if compact {
        (width - 8.0 - 60.0 - 8.0).min(4.0 * 94.0 + 3.0 * 6.0)
    } else {
        4.0 * 94.0 + 3.0 * 6.0
    };
    Arrangement {
        toolbar,
        body,
        footer,
        details,
        transfer_width,
        waveform_width,
        primary_width,
        primary_height: if compact_split {
            body - 4.0
        } else {
            footer - 5.0
        },
        details_width: if inline_details {
            width - 8.0 - INLINE_CORE_WIDTH - 1.0 - 2.0 * 8.0
        } else if advanced_aside {
            ASIDE_WIDTH
        } else {
            width - 8.0
        },
        knob_size,
        primary_rows: if compact_split { 2 } else { 1 },
        compact_split,
        both_plots,
        input_meter,
        output_meter,
        advanced_aside,
        inline_details,
        primary_visible: !advanced || !compact || height >= 112.0,
        character_visible: compact_split || !compact && !inline_details,
    }
}

pub fn card_width(available: f32) -> f32 {
    available.clamp(COMPACT_WIDTH, MAX_WIDTH)
}

pub fn preset_bar_width(width: f32, height: f32) -> f32 {
    width - if height < 160.0 { 140.0 } else { 8.0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIELD_HEIGHT: f32 = 8.0 * 1.3 + 3.0 + 10.0 * 1.3 + 2.0;
    const FIELD_WIDTH: f32 = 68.0;
    const FIELD_GAP: f32 = 6.0;
    const BANK_ROW_GAP: f32 = 8.0;
    const CHARACTER_WIDTH: f32 = 112.0;
    const DISCLOSURE_WIDTH: f32 = 60.0;

    #[test]
    fn every_measured_size_fits_real_children_without_internal_scrolling() {
        for width in [
            430.0, 464.0, 620.0, 739.0, 740.0, 812.0, 871.0, 872.0, 900.0,
        ] {
            for height in 64..=304 {
                for advanced in [false, true] {
                    let layout = arrange(width, height as f32, advanced);
                    assert!(
                        layout.toolbar + layout.body + layout.footer + layout.details
                            <= height as f32 + 0.001,
                        "{width}x{height}, Advanced={advanced}: {layout:?}"
                    );
                    assert!(layout.toolbar >= 26.0);
                    assert!(layout.body == 0.0 || layout.body >= 84.0);
                    assert!(layout.footer >= 36.0);
                    assert!(layout.details == 0.0 || layout.details - 6.0 >= FIELD_HEIGHT);
                    if layout.primary_visible {
                        assert!(matches!(layout.primary_rows, 1 | 2));
                        let columns = 4 / layout.primary_rows;
                        let child = FIELD_HEIGHT + layout.knob_size.map_or(0.0, |knob| knob + 3.0);
                        let needed = layout.primary_rows as f32 * child
                            + (layout.primary_rows - 1) as f32 * BANK_ROW_GAP;
                        assert!(
                            layout.primary_height >= needed,
                            "primary children clip at {width}x{height}: {layout:?}"
                        );
                        assert!(
                            layout.primary_width
                                >= columns as f32 * FIELD_WIDTH + (columns - 1) as f32 * FIELD_GAP,
                            "primary fields squeeze at {width}x{height}: {layout:?}"
                        );
                    }
                    if layout.body > 0.0 {
                        let real_plot = layout.body - 4.0 - (8.0 * 1.3 + 3.0) - 32.0;
                        assert!(
                            real_plot >= 32.0,
                            "axes displace the real plot at {width}x{height}"
                        );
                    }
                    assert!(!layout.both_plots || layout.body > 0.0 && width >= MEDIUM_WIDTH);
                    assert!(!layout.input_meter || width >= EXPANDED_WIDTH && layout.body > 0.0);
                    assert!(!layout.output_meter || width >= MEDIUM_WIDTH && layout.body > 0.0);
                    assert!(
                        !layout.advanced_aside
                            || advanced && width >= EXPANDED_WIDTH && layout.body > 0.0
                    );
                    assert!(
                        !advanced || width < MEDIUM_WIDTH || layout.primary_visible,
                        "wide Advanced replaces core fields at {width}x{height}"
                    );
                }
            }
        }
    }

    #[test]
    fn compact_keeps_transfer_left_of_the_four_control_two_by_two_bank() {
        for width in [430.0, 464.0, 620.0, 739.0] {
            for advanced in [false, true] {
                let layout = arrange(width, 304.0, advanced);
                assert!(layout.compact_split && layout.primary_visible && layout.character_visible);
                assert_eq!(layout.primary_rows, 2);
                assert_eq!(layout.primary_width, COMPACT_BANK_WIDTH);
                assert_eq!(layout.knob_size, Some(34.0));
                assert!(
                    !layout.both_plots
                        && !layout.input_meter
                        && !layout.output_meter
                        && !layout.advanced_aside
                );
                assert_eq!(layout.waveform_width, 0.0);
                assert!(layout.transfer_width >= 220.0);
                assert!(8.0 + layout.transfer_width + 12.0 + layout.primary_width <= width + 0.001);
                assert!(layout.primary_height <= layout.body - 4.0);
                assert!(layout.primary_height >= 2.0 * (FIELD_HEIGHT + 34.0 + 3.0) + BANK_ROW_GAP);
                assert!(layout.footer - 4.0 >= FIELD_HEIGHT);
                assert!(width - 8.0 >= CHARACTER_WIDTH + DISCLOSURE_WIDTH + 8.0);
                if advanced {
                    assert!(layout.details > 0.0);
                }
            }
        }
    }

    #[test]
    fn compact_reduces_knobs_before_losing_a_usable_split_plot() {
        let numeric = arrange(430.0, 146.0, false);
        assert!(numeric.compact_split && numeric.primary_visible);
        assert_eq!(numeric.primary_rows, 2);
        assert_eq!(numeric.knob_size, None);
        assert!(numeric.body >= 84.0);
        let intermediate = arrange(430.0, 240.0, true);
        assert!(intermediate.compact_split && intermediate.primary_visible);
        assert_eq!(intermediate.knob_size, Some(20.0));
        assert!(intermediate.primary_height >= 2.0 * (FIELD_HEIGHT + 20.0 + 3.0) + BANK_ROW_GAP);
        let minimum = arrange(430.0, 64.0, false);
        assert!(!minimum.compact_split);
        assert_eq!(minimum.body, 0.0);
        assert!(minimum.primary_visible);
        assert_eq!(minimum.primary_rows, 1);
        assert_eq!(minimum.knob_size, None);
        let details = arrange(430.0, 64.0, true);
        assert!(!details.primary_visible && !details.inline_details);
        assert_eq!(details.body, 0.0);
    }

    #[test]
    fn medium_keeps_both_plots_output_meter_and_bounded_primary_footer() {
        for width in [740.0, 812.0, 871.0] {
            for advanced in [false, true] {
                let layout = arrange(width, 304.0, advanced);
                assert!(
                    layout.both_plots
                        && layout.output_meter
                        && layout.primary_visible
                        && layout.character_visible
                );
                assert!(!layout.input_meter && !layout.advanced_aside && !layout.compact_split);
                assert_eq!(layout.primary_rows, 1);
                assert!(layout.transfer_width >= 240.0 && layout.waveform_width >= 200.0);
                assert!(layout.transfer_width >= layout.waveform_width);
                assert!(
                    8.0 + layout.transfer_width + layout.waveform_width + METER_WIDTH + 2.0 * 8.0
                        <= width + 0.001
                );
                assert!(layout.primary_width <= 4.0 * 94.0 + 3.0 * FIELD_GAP);
                assert!(
                    layout.primary_width
                        + CHARACTER_WIDTH
                        + DISCLOSURE_WIDTH
                        + 3.0 * 12.0
                        + 1.0
                        + 8.0
                        <= width
                );
                assert!(layout.primary_height <= layout.footer - 5.0);
                assert_eq!(layout.details > 0.0, advanced);
            }
        }
    }

    #[test]
    fn expanded_places_advanced_beside_paired_plots_and_keeps_character_beside_core() {
        for width in [872.0, 900.0] {
            for advanced in [false, true] {
                let layout = arrange(width, 304.0, advanced);
                assert!(layout.both_plots && layout.input_meter && layout.output_meter);
                assert!(layout.primary_visible && layout.character_visible);
                assert_eq!(layout.primary_rows, 1);
                assert_eq!(layout.advanced_aside, advanced);
                assert_eq!(layout.details, 0.0);
                assert!(layout.transfer_width >= 300.0 && layout.waveform_width >= 230.0);
                let aside = if advanced { ASIDE_WIDTH + 8.0 } else { 0.0 };
                assert!(
                    8.0 + 2.0 * METER_WIDTH
                        + 3.0 * 8.0
                        + layout.transfer_width
                        + layout.waveform_width
                        + aside
                        <= width + 0.001
                );
                if advanced {
                    let heading = 9.0 * 1.3 + 6.0;
                    let filters_and_quality = 3.0 * FIELD_HEIGHT + 2.0 * 8.0;
                    assert!(layout.body - 8.0 >= heading + 6.0 + filters_and_quality);
                    assert!(ASIDE_WIDTH - 8.0 >= 22.0 + 54.0 + 12.0 + 2.0 * 4.0);
                }
            }
        }
    }

    #[test]
    fn wide_minimum_advanced_keeps_four_core_fields_beside_explicit_pages() {
        for width in [740.0, 812.0, 872.0, 900.0] {
            for height in 64..112 {
                let layout = arrange(width, height as f32, true);
                assert!(layout.primary_visible && layout.inline_details);
                assert_eq!(layout.primary_rows, 1);
                assert_eq!(layout.knob_size, None);
                assert_eq!(layout.primary_width, INLINE_CORE_WIDTH);
                assert_eq!(layout.details, 0.0);
                assert_eq!(layout.body, 0.0);
                assert!(
                    8.0 + INLINE_CORE_WIDTH + 1.0 + 2.0 * 8.0 + layout.details_width
                        <= width + 0.001
                );
                let selector = 88.0 + 8.0;
                let filters = 2.0 * (22.0 + 62.0) + 3.0 * FIELD_GAP;
                let metering = 8.0 * 36.0 + 7.0 * 4.0;
                assert!(layout.details_width >= selector + filters);
                assert!(layout.details_width >= selector + metering);
                assert!(layout.footer - 5.0 >= FIELD_HEIGHT);
            }
        }
    }

    #[test]
    fn card_width_uses_chain_remainder_and_short_toolbar_reserves_disclosure_room() {
        assert_eq!(card_width(100.0), COMPACT_WIDTH);
        assert_eq!(card_width(812.0), 812.0);
        assert_eq!(card_width(1800.0), MAX_WIDTH);
        for width in [430.0, 740.0, 900.0] {
            assert!(preset_bar_width(width, 64.0) + 120.0 + 8.0 <= width);
            assert!(preset_bar_width(width, 304.0) + 8.0 <= width);
        }
    }
}
