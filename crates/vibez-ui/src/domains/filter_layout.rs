//! Measured regions for the native Filter body and its modulation disclosure.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    Compact,
    Medium,
    Expanded,
}

#[derive(Debug, Clone, Copy)]
pub struct Region {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Region {
    fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width: width.max(0.0),
            height: height.max(0.0),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Arrangement {
    pub tier: Tier,
    pub toolbar: Region,
    pub tabs: Option<Region>,
    pub response: Option<Region>,
    pub meter: Option<Region>,
    pub modes: Option<Region>,
    pub primary: Region,
    pub modulation: Option<Region>,
    pub disclosure: Option<Region>,
    pub vertical_modes: bool,
    pub knob_size: Option<f32>,
}

pub fn arrange(width: f32, height: f32, advanced: bool) -> Arrangement {
    let tier = if width >= 820.0 {
        Tier::Expanded
    } else if width >= 620.0 {
        Tier::Medium
    } else {
        Tier::Compact
    };
    let inner = (width - 16.0).max(0.0);
    let short = height < 114.0;
    let toolbar_h = if short { 22.0 } else { 26.0 };
    let primary_h = if short || (advanced && height < 180.0 && tier != Tier::Expanded) {
        28.0
    } else if height >= 260.0 {
        72.0
    } else if height >= 180.0 {
        60.0
    } else {
        46.0
    };
    let knob_size = if primary_h >= 70.0 {
        Some(40.0)
    } else if primary_h >= 60.0 {
        Some(28.0)
    } else if primary_h >= 44.0 {
        Some(16.0)
    } else {
        None
    };
    let primary_y = (height - primary_h).max(toolbar_h);
    let mut layout = Arrangement {
        tier,
        toolbar: Region::new(8.0, 0.0, inner, toolbar_h),
        tabs: None,
        response: None,
        meter: None,
        modes: None,
        primary: Region::new(8.0, primary_y, inner, primary_h),
        modulation: None,
        disclosure: None,
        vertical_modes: false,
        knob_size,
    };
    if short {
        let controls_w = if width >= 620.0 { 360.0 } else { inner };
        layout.primary.width = controls_w;
        let options = Region::new(8.0, toolbar_h, controls_w, (primary_y - toolbar_h).max(0.0));
        if advanced {
            layout.modulation = Some(options);
        } else {
            layout.modes = Some(options);
        }
        if width >= 620.0 {
            let x = 8.0 + controls_w + 12.0;
            layout.response = Some(Region::new(
                x,
                toolbar_h,
                width - x - 62.0,
                height - toolbar_h,
            ));
            layout.meter = Some(Region::new(
                width - 54.0,
                toolbar_h,
                46.0,
                height - toolbar_h,
            ));
        }
        return layout;
    }
    if tier == Tier::Expanded {
        let top = toolbar_h + 4.0;
        let available = primary_y - top - 4.0;
        if available >= 148.8 {
            layout.modes = Some(Region::new(8.0, top, 88.0, available));
            layout.vertical_modes = true;
            layout.response = Some(Region::new(108.0, top, width - 170.0, available));
            layout.meter = Some(Region::new(width - 54.0, top, 46.0, available));
        } else {
            let plot_h = (available - 28.0).max(0.0);
            if plot_h >= 16.0 {
                layout.response = Some(Region::new(8.0, top, width - 70.0, plot_h));
                layout.meter = Some(Region::new(width - 54.0, top, 46.0, plot_h));
            }
            layout.modes = Some(Region::new(8.0, top + plot_h + 4.0, inner, 24.0));
        }
        layout.primary.width = 328.0;
        if advanced {
            layout.modulation = Some(Region::new(352.0, primary_y, width - 360.0, primary_h));
        } else {
            layout.disclosure = Some(Region::new(352.0, primary_y + 20.0, 82.0, 20.0));
        }
        return layout;
    }
    let tabs_h = if tier == Tier::Medium && height >= 180.0 {
        20.0
    } else {
        0.0
    };
    if tabs_h > 0.0 {
        layout.tabs = Some(Region::new(8.0, toolbar_h, inner, tabs_h));
    } else {
        layout.disclosure = Some(Region::new(width - 90.0, height - 20.0, 82.0, 20.0));
    }
    let mut modulation_h = if !advanced {
        0.0
    } else if height >= 260.0 {
        70.0
    } else if height >= 180.0 {
        54.0
    } else {
        14.0
    };
    let mut y = toolbar_h + tabs_h + 4.0;
    let response_space = |modulation_h| {
        let available = primary_y - y - modulation_h - if advanced { 8.0 } else { 4.0 };
        let side_modes = tier == Tier::Medium && available >= 74.0;
        let plot_h = (available - if side_modes { 0.0 } else { 28.0 }).max(0.0);
        (plot_h, side_modes)
    };
    let (mut plot_h, mut side_modes) = response_space(modulation_h);
    if advanced && modulation_h > 14.0 && plot_h < 48.0 {
        modulation_h = 14.0;
        (plot_h, side_modes) = response_space(modulation_h);
    }
    if plot_h >= 16.0 {
        let right = if side_modes { width - 198.0 } else { width - 8.0 };
        layout.response = Some(Region::new(8.0, y, right - 66.0, plot_h));
        layout.meter = Some(Region::new(right - 46.0, y, 46.0, plot_h));
    }
    if side_modes {
        layout.modes = Some(Region::new(width - 186.0, y, 178.0, plot_h));
    }
    y += plot_h + 4.0;
    if !side_modes {
        layout.modes = Some(Region::new(8.0, y, inner, 24.0));
        y += 28.0;
    }
    if advanced {
        layout.modulation = Some(Region::new(8.0, y, inner, modulation_h));
    }
    layout.primary.width = inner.min(if layout.disclosure.is_some() {
        inner - 84.0
    } else {
        448.0
    });
    layout
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn advanced_retains_response_output_and_primary_controls_when_the_panel_has_room() {
        for (width, height) in [430.0, 660.0, 900.0]
            .into_iter()
            .flat_map(|width| [304.0, 318.0].map(|height| (width, height)))
        {
            let layout = arrange(width, height, true);
            let response = layout.response.expect("Advanced must retain the response");
            let meter = layout.meter.expect("Advanced must retain output metering");
            let modulation = layout.modulation.expect("Advanced must expose modulation");
            assert!(response.width >= 200.0, "response too narrow at {width}");
            assert!(response.height >= 80.0, "response too short at {width}");
            assert!(meter.height >= 80.0, "meter too short at {width}");
            assert!(
                layout.primary.height >= 60.0,
                "core controls lost at {width}"
            );
            for region in [response, meter, layout.primary, modulation] {
                assert!(region.x >= 0.0 && region.y >= 0.0);
                assert!(region.x + region.width <= width + 0.01);
                assert!(region.y + region.height <= height + 0.01);
            }
            for (left, right) in [
                (response, modulation),
                (meter, modulation),
                (layout.primary, modulation),
            ] {
                let overlap_x = left.x < right.x + right.width && right.x < left.x + left.width;
                let overlap_y = left.y < right.y + right.height && right.y < left.y + left.height;
                assert!(
                    !(overlap_x && overlap_y),
                    "disclosure overlaps content at {width}"
                );
            }
        }
    }

    #[test]
    fn every_measured_region_fits_without_overlap_at_supported_panel_sizes() {
        for width in [430.0, 464.0, 619.0, 620.0, 660.0, 819.0, 820.0, 900.0] {
            for height in (64..=400).step_by(2).map(|height| height as f32) {
                for advanced in [false, true] {
                    let layout = arrange(width, height, advanced);
                    let regions = [
                        Some(layout.toolbar),
                        layout.tabs,
                        layout.response,
                        layout.meter,
                        layout.modes,
                        Some(layout.primary),
                        layout.modulation,
                        layout.disclosure,
                    ]
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>();
                    for region in &regions {
                        assert!(region.x >= 0.0 && region.y >= 0.0);
                        assert!(region.width > 0.0 && region.height > 0.0);
                        assert!(
                            region.x + region.width <= width + 0.01,
                            "horizontal overflow {width}x{height} Advanced={advanced}: {region:?}"
                        );
                        assert!(
                            region.y + region.height <= height + 0.01,
                            "vertical overflow {width}x{height} Advanced={advanced}: {region:?}"
                        );
                    }
                    for (index, left) in regions.iter().enumerate() {
                        for right in &regions[index + 1..] {
                            let overlap_x =
                                left.x < right.x + right.width && right.x < left.x + left.width;
                            let overlap_y =
                                left.y < right.y + right.height && right.y < left.y + left.height;
                            assert!(
                                !(overlap_x && overlap_y),
                                "overlap {width}x{height} Advanced={advanced}: {left:?}, {right:?}"
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn tier_hierarchy_keeps_modulation_beside_expanded_controls_and_medium_tabs() {
        let expanded = arrange(900.0, 304.0, true);
        assert!(expanded.vertical_modes);
        assert!(expanded.response.unwrap().width > expanded.primary.width * 2.0);
        let modulation = expanded.modulation.unwrap();
        assert_eq!(modulation.y, expanded.primary.y);
        assert!(modulation.x >= expanded.primary.x + expanded.primary.width);
        for advanced in [false, true] {
            let medium = arrange(660.0, 304.0, advanced);
            assert!(medium.tabs.is_some());
            assert!(medium.response.is_some() && medium.meter.is_some());
            let compact = arrange(430.0, 304.0, advanced);
            let modes = compact.modes.unwrap();
            assert!(modes.width > modes.height * 8.0);
            assert!(compact.primary.height >= 60.0);
        }
        let minimum = arrange(430.0, 64.0, true);
        assert!(minimum.modulation.unwrap().height >= 14.0);
        assert!(minimum.primary.height >= 28.0);
    }

    #[test]
    fn filter_mode_and_slope_choices_survive_every_resize_transition() {
        for width in [430.0, 464.0, 619.0, 620.0, 660.0, 819.0, 820.0, 900.0] {
            for height in (64..=400).map(|height| height as f32) {
                for advanced in [false, true] {
                    let layout = arrange(width, height, advanced);
                    if advanced && height < 114.0 {
                        assert!(arrange(width, height, false).modes.is_some());
                    } else {
                        assert!(layout.modes.is_some(),
                            "mode and slope choices lost at {width}x{height}, Advanced={advanced}");
                    }
                }
            }
        }
    }

    #[test]
    fn the_vertical_type_bank_fits_its_native_children_or_gets_a_wide_fallback() {
        let native_bank_height = 5.0 * 20.0 + 2.0 * 10.4 + 7.0 * 4.0;
        for width in [820.0, 900.0] {
            for height in 114..=400 {
                for advanced in [false, true] {
                    let layout = arrange(width, height as f32, advanced);
                    if let Some(modes) = layout.modes {
                        if layout.vertical_modes {
                            assert!(modes.height >= native_bank_height,
                                "vertical mode bank clips at {width}x{height}: {} < {native_bank_height}",
                                modes.height);
                        } else {
                            assert!(modes.width >= 178.0,
                                "horizontal choices cannot fit an88px vertical-bank column");
                        }
                    }
                }
            }
        }
    }

}
