//! Fits Bitcrush's genuine waveform, bounded core controls and accessible detail pages.

use vibez_dsp::bitcrush::param;

pub const MIN_WIDTH: f32 = 430.0;
pub const MEDIUM_WIDTH: f32 = 740.0;
pub const EXPANDED_WIDTH: f32 = 872.0;
pub const MAX_WIDTH: f32 = 900.0;
const FIELD_HEIGHT: f32 = 8.0 * 1.3 + 3.0 + 10.0 * 1.3 + 2.0;
const TOOLBAR_BOTTOM: f32 = 28.0;
const GAP: f32 = 6.0;
const SCOPE_MIN_HEIGHT: f32 = 84.0;
const METER_WIDTH: f32 = 52.0;
const ASIDE_WIDTH: f32 = 296.0;
const STACKED_DETAILS_HEIGHT: f32 = 2.0 * FIELD_HEIGHT + GAP;
const COMPACT_CORE: &[usize] = &[param::BITS, param::SR_REDUCE, param::MIX];
const FULL_CORE: &[usize] = &[param::BITS, param::SR_REDUCE, param::DRIVE_DB, param::MIX];

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Region {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DetailPage {
    Main,
    #[default]
    Color,
    Clock,
    Filter,
    Input,
    Output,
}

impl DetailPage {
    pub const ALL: [Self; 6] = [
        Self::Main,
        Self::Color,
        Self::Clock,
        Self::Filter,
        Self::Input,
        Self::Output,
    ];

    fn cells(self, drive_in_core: bool) -> &'static [DetailCell] {
        use DetailCell::*;
        match self {
            Self::Main => &[
                Parameter(param::BITS),
                Parameter(param::SR_REDUCE),
                Parameter(param::MIX),
            ],
            Self::Color if drive_in_core => &[
                Parameter(param::ROUNDING),
                Parameter(param::DITHER),
            ],
            Self::Color => &[
                Parameter(param::DRIVE_DB),
                Parameter(param::ROUNDING),
                Parameter(param::DITHER),
            ],
            Self::Clock => &[
                Parameter(param::SR_REDUCE),
                Parameter(param::JITTER),
                ClockStatus,
            ],
            Self::Filter => &[
                Parameter(param::POST_LP_ENABLED),
                Parameter(param::POST_LP_HZ),
            ],
            Self::Input => &[InputPeakLeft, InputPeakRight, InputRmsLeft, InputRmsRight],
            Self::Output => &[
                OutputPeakLeft,
                OutputPeakRight,
                OutputRmsLeft,
                OutputRmsRight,
            ],
        }
    }
}

impl std::fmt::Display for DetailPage {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Main => "Main",
            Self::Color => "Color",
            Self::Clock => "Clock",
            Self::Filter => "Filter",
            Self::Input => "Input",
            Self::Output => "Output",
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DetailCell {
    Parameter(usize),
    ClockStatus,
    InputPeakLeft,
    InputPeakRight,
    InputRmsLeft,
    InputRmsRight,
    OutputPeakLeft,
    OutputPeakRight,
    OutputRmsLeft,
    OutputRmsRight,
}

#[derive(Clone, Debug)]
pub struct Details {
    pub bounds: Region,
    pub selector: Region,
    pub content: Region,
    pub page: DetailPage,
    pub cells: &'static [DetailCell],
    pub cell_bounds: Vec<Region>,
}

#[derive(Clone, Copy, Debug)]
pub struct CoreControl {
    pub parameter: usize,
    pub bounds: Region,
    pub knob_size: Option<f32>,
}

#[derive(Clone, Debug)]
pub struct Arrangement {
    pub presets: Region,
    pub disclosure: Region,
    pub core: Vec<CoreControl>,
    pub scope: Option<Region>,
    pub input_meter: Option<Region>,
    pub output_meter: Option<Region>,
    pub divider: Option<Region>,
    pub details: Option<Details>,
}

pub fn preset_bar_width(width: f32) -> f32 {
    width - 76.0
}

pub fn card_width(available: f32) -> f32 {
    available.clamp(MIN_WIDTH, MAX_WIDTH)
}

fn control_height(knob_size: Option<f32>) -> f32 {
    FIELD_HEIGHT + knob_size.map_or(0.0, |size| size + 3.0)
}

fn fit_knob(available: f32) -> Option<f32> {
    [34.0, 20.0]
        .into_iter()
        .find(|size| control_height(Some(*size)) <= available)
}

fn core_bank(
    parameters: &[usize],
    x: f32,
    y: f32,
    cell_width: f32,
    gap: f32,
    knob_size: Option<f32>,
) -> Vec<CoreControl> {
    parameters
        .iter()
        .enumerate()
        .map(|(index, parameter)| CoreControl {
            parameter: *parameter,
            bounds: Region {
                x: x + index as f32 * (cell_width + gap),
                y,
                width: cell_width,
                height: control_height(knob_size),
            },
            knob_size,
        })
        .collect()
}

fn details(bounds: Region, page: DetailPage, drive_in_core: bool, stacked: bool) -> Details {
    let field_y = bounds.y + (bounds.height - FIELD_HEIGHT) * 0.5;
    let selector = Region {
        x: bounds.x,
        y: if stacked { bounds.y } else { field_y },
        width: 88.0,
        height: FIELD_HEIGHT,
    };
    let content = Region {
        x: bounds.x + if stacked { 0.0 } else { 96.0 },
        y: if stacked { bounds.y + FIELD_HEIGHT + GAP } else { field_y },
        width: bounds.width - if stacked { 0.0 } else { 96.0 },
        height: FIELD_HEIGHT,
    };
    let cells = page.cells(drive_in_core);
    let cell_width = ((content.width - (cells.len() - 1) as f32 * GAP)
        / cells.len() as f32)
        .min(94.0);
    let cell_bounds = (0..cells.len())
        .map(|index| Region {
            x: content.x + index as f32 * (cell_width + GAP),
            y: content.y,
            width: cell_width,
            height: FIELD_HEIGHT,
        })
        .collect();
    Details { bounds, selector, content, page, cells, cell_bounds }
}

fn plots(layout: &mut Arrangement, bounds: Region, meters: bool) {
    if meters {
        layout.input_meter = Some(Region { width: METER_WIDTH, ..bounds });
        layout.output_meter = Some(Region {
            x: bounds.x + bounds.width - METER_WIDTH,
            width: METER_WIDTH,
            ..bounds
        });
        layout.scope = Some(Region {
            x: bounds.x + METER_WIDTH + GAP,
            width: bounds.width - 2.0 * (METER_WIDTH + GAP),
            ..bounds
        });
    } else {
        layout.scope = Some(bounds);
    }
}

pub fn arrange(width: f32, height: f32, advanced: bool, page: DetailPage) -> Arrangement {
    let compact = width < MEDIUM_WIDTH;
    let expanded = width >= EXPANDED_WIDTH;
    let body = Region {
        x: 4.0,
        y: TOOLBAR_BOTTOM,
        width: width - 8.0,
        height: height - TOOLBAR_BOTTOM,
    };
    let mut layout = Arrangement {
        presets: Region { x: 4.0, y: 0.0, width: preset_bar_width(width), height: 26.0 },
        disclosure: Region { x: width - 64.0, y: 0.0, width: 60.0, height: 26.0 },
        core: Vec::new(),
        scope: None,
        input_meter: None,
        output_meter: None,
        divider: None,
        details: None,
    };
    let detail_reserve = if advanced { FIELD_HEIGHT + GAP } else { 0.0 };
    let side_reserve = if advanced { STACKED_DETAILS_HEIGHT + GAP } else { 0.0 };
    let plotted_core_room = if expanded {
        body.height - side_reserve
    } else {
        body.height - SCOPE_MIN_HEIGHT - GAP - detail_reserve
    };
    let plot_fits = if expanded {
        body.height >= SCOPE_MIN_HEIGHT && plotted_core_room >= FIELD_HEIGHT
    } else {
        plotted_core_room >= FIELD_HEIGHT
    };
    if plot_fits && expanded {
        let knob = fit_knob(plotted_core_room);
        let side_height = control_height(knob) + side_reserve;
        let side_x = width - 4.0 - ASIDE_WIDTH;
        let core_y = body.y + (body.height - side_height) * 0.5;
        layout.core = core_bank(FULL_CORE, side_x, core_y, 68.0, 8.0, knob);
        let divider = Region { x: side_x - 9.0, width: 1.0, ..body };
        layout.divider = Some(divider);
        plots(&mut layout, Region { width: divider.x - 8.0 - body.x, ..body }, true);
        if advanced {
            layout.details = Some(details(
                Region {
                    x: side_x,
                    y: core_y + control_height(knob) + GAP,
                    width: ASIDE_WIDTH,
                    height: STACKED_DETAILS_HEIGHT,
                },
                page,
                true,
                true,
            ));
        }
    } else if plot_fits {
        let knob = fit_knob(plotted_core_room);
        let scope_height = body.height - control_height(knob) - GAP - detail_reserve;
        plots(&mut layout, Region { height: scope_height, ..body }, !compact);
        let core_y = body.y + scope_height + GAP;
        layout.core = core_bank(
            if compact { COMPACT_CORE } else { FULL_CORE },
            body.x,
            core_y,
            94.0,
            GAP,
            knob,
        );
        if advanced {
            layout.details = Some(details(
                Region {
                    y: core_y + control_height(knob) + GAP,
                    height: FIELD_HEIGHT,
                    ..body
                },
                page,
                !compact,
                false,
            ));
        }
    } else if !compact && advanced {
        let knob = fit_knob(body.height);
        let core_y = body.y + (body.height - control_height(knob)) * 0.5;
        layout.core = core_bank(FULL_CORE, body.x, core_y, 68.0, GAP, knob);
        let divider = Region { x: body.x + 290.0 + 8.0, width: 1.0, ..body };
        layout.divider = Some(divider);
        layout.details = Some(details(
            Region {
                x: divider.x + 9.0,
                width: body.x + body.width - divider.x - 9.0,
                ..body
            },
            page,
            true,
            false,
        ));
    } else if compact && advanced && body.height < 2.0 * FIELD_HEIGHT + GAP {
        layout.details = Some(details(body, page, false, false));
    } else {
        let knob = fit_knob(body.height - detail_reserve);
        let bank_height = control_height(knob) + detail_reserve;
        let core_y = body.y + (body.height - bank_height) * 0.5;
        layout.core = core_bank(
            if compact { COMPACT_CORE } else { FULL_CORE },
            body.x,
            core_y,
            94.0,
            GAP,
            knob,
        );
        if advanced {
            layout.details = Some(details(
                Region {
                    y: core_y + control_height(knob) + GAP,
                    height: FIELD_HEIGHT,
                    ..body
                },
                page,
                false,
                false,
            ));
        }
    }
    layout
}

#[cfg(test)]
mod tests {
    use super::{arrange, DetailCell, DetailPage, Region};
    use vibez_dsp::bitcrush::param;

    fn assert_inside(region: Region, parent: Region) {
        assert!(region.width > 0.0 && region.height > 0.0);
        assert!(region.x >= parent.x && region.y >= parent.y);
        assert!(region.x + region.width <= parent.x + parent.width + 0.001);
        assert!(region.y + region.height <= parent.y + parent.height + 0.001);
    }

    #[test]
    fn compact_minimum_keeps_all_six_pages_and_their_real_native_children_accessible() {
        use DetailCell::{
            ClockStatus, InputPeakLeft, InputPeakRight, InputRmsLeft, InputRmsRight,
            OutputPeakLeft, OutputPeakRight, OutputRmsLeft, OutputRmsRight, Parameter,
        };
        use DetailPage::{Clock, Color, Filter, Input, Main, Output};

        let cases: [(DetailPage, &[DetailCell]); 6] = [
            (
                Main,
                &[
                    Parameter(param::BITS),
                    Parameter(param::SR_REDUCE),
                    Parameter(param::MIX),
                ],
            ),
            (
                Color,
                &[
                    Parameter(param::DRIVE_DB),
                    Parameter(param::ROUNDING),
                    Parameter(param::DITHER),
                ],
            ),
            (
                Clock,
                &[
                    Parameter(param::SR_REDUCE),
                    Parameter(param::JITTER),
                    ClockStatus,
                ],
            ),
            (
                Filter,
                &[
                    Parameter(param::POST_LP_ENABLED),
                    Parameter(param::POST_LP_HZ),
                ],
            ),
            (
                Input,
                &[InputPeakLeft, InputPeakRight, InputRmsLeft, InputRmsRight],
            ),
            (
                Output,
                &[
                    OutputPeakLeft,
                    OutputPeakRight,
                    OutputRmsLeft,
                    OutputRmsRight,
                ],
            ),
        ];
        assert_eq!(DetailPage::ALL, [Main, Color, Clock, Filter, Input, Output]);

        let bounds = Region {
            x: 0.0,
            y: 0.0,
            width: 430.0,
            height: 64.0,
        };
        let native_field_height = 8.0 * 1.3 + 3.0 + 10.0 * 1.3 + 2.0;
        let four_readings_width = 4.0 * 68.0 + 3.0 * 6.0;
        for (page, required) in cases {
            let layout = arrange(bounds.width, bounds.height, true, page);
            assert_inside(layout.presets, bounds);
            assert_inside(layout.disclosure, bounds);
            assert!(layout.presets.width >= 140.0 + 72.0);
            assert!(layout.presets.height >= 26.0);
            assert!(layout.disclosure.width >= 60.0);
            assert!(layout.disclosure.height >= 9.0 * 1.3 + 6.0);
            assert!(
                layout.presets.x + layout.presets.width + 8.0 <= layout.disclosure.x,
                "preset actions overlap the disclosure on {page:?}"
            );
            assert!(layout.scope.is_none());

            let details = layout.details.expect("every minimum page needs a region");
            assert_eq!(details.page, page);
            assert_inside(details.bounds, bounds);
            assert_inside(details.selector, details.bounds);
            assert_inside(details.content, details.bounds);
            assert!(details.bounds.y >= layout.presets.y + layout.presets.height);
            assert!(details.selector.width >= 88.0);
            assert!(details.selector.height >= native_field_height);
            assert!(details.content.height >= native_field_height);
            assert!(details.content.width >= four_readings_width);
            assert!(
                details.selector.x + details.selector.width + 8.0 <= details.content.x,
                "page selection overlaps native fields on {page:?}"
            );
            assert_eq!(details.cells.len(), required.len());
            for cell in required {
                assert!(
                    details.cells.contains(cell),
                    "{page:?} lost an accessible child: {cell:?}"
                );
            }
        }
    }
}

#[cfg(test)]
#[path = "bitcrush_layout_geometry_tests.rs"]
mod geometry_tests;
