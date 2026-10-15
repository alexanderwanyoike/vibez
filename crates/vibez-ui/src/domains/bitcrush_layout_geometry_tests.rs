//! Verifies Bitcrush's concept composition against actual native child budgets across its full body range.

use super::{arrange, card_width, DetailCell, DetailPage, Region};
use vibez_dsp::bitcrush::param;

const FIELD_HEIGHT: f32 = 8.0 * 1.3 + 3.0 + 10.0 * 1.3 + 2.0;
const TOOLBAR_BOTTOM: f32 = 28.0;
const SCOPE_MIN_HEIGHT: f32 = 84.0;
const CELL_GAP: f32 = 6.0;
const WIDTHS: [f32; 13] = [
    430.0, 431.0, 464.0, 620.0, 739.0, 740.0, 741.0, 812.0, 871.0, 872.0, 873.0, 899.0, 900.0,
];

fn right(region: Region) -> f32 {
    region.x + region.width
}

fn bottom(region: Region) -> f32 {
    region.y + region.height
}

fn assert_inside(region: Region, parent: Region, context: &str) {
    assert!(
        [region.x, region.y, region.width, region.height]
            .into_iter()
            .all(f32::is_finite),
        "nonfinite child {region:?}: {context}"
    );
    assert!(
        region.width > 0.0 && region.height > 0.0,
        "empty child: {context}"
    );
    assert!(
        region.x >= parent.x - 0.001 && region.y >= parent.y - 0.001,
        "{region:?}: {context}"
    );
    assert!(
        right(region) <= right(parent) + 0.001,
        "horizontal overflow {region:?}: {context}"
    );
    assert!(
        bottom(region) <= bottom(parent) + 0.001,
        "vertical overflow {region:?}: {context}"
    );
}

fn assert_separate(a: Region, b: Region, context: &str) {
    assert!(
        right(a) <= b.x + 0.001
            || right(b) <= a.x + 0.001
            || bottom(a) <= b.y + 0.001
            || bottom(b) <= a.y + 0.001,
        "overlapping children {a:?} and {b:?}: {context}"
    );
}

fn required_cells(page: DetailPage, drive_in_core: bool) -> Vec<DetailCell> {
    use DetailCell::*;
    match page {
        DetailPage::Main => vec![
            Parameter(param::BITS),
            Parameter(param::SR_REDUCE),
            Parameter(param::MIX),
        ],
        DetailPage::Color => {
            let mut cells = vec![Parameter(param::ROUNDING), Parameter(param::DITHER)];
            if !drive_in_core {
                cells.insert(0, Parameter(param::DRIVE_DB));
            }
            cells
        }
        DetailPage::Clock => vec![
            Parameter(param::SR_REDUCE),
            Parameter(param::JITTER),
            ClockStatus,
        ],
        DetailPage::Filter => vec![
            Parameter(param::POST_LP_ENABLED),
            Parameter(param::POST_LP_HZ),
        ],
        DetailPage::Input => vec![InputPeakLeft, InputPeakRight, InputRmsLeft, InputRmsRight],
        DetailPage::Output => vec![
            OutputPeakLeft,
            OutputPeakRight,
            OutputRmsLeft,
            OutputRmsRight,
        ],
    }
}

fn control_height(knob: Option<f32>) -> f32 {
    FIELD_HEIGHT + knob.map_or(0.0, |size| size + 3.0)
}

fn scope_fits(width: f32, height: f32, advanced: bool, knob: Option<f32>) -> bool {
    let controls = control_height(knob);
    let needed = if width >= 872.0 {
        let side = controls
            + if advanced {
                CELL_GAP + 2.0 * FIELD_HEIGHT + CELL_GAP
            } else {
                0.0
            };
        TOOLBAR_BOTTOM + SCOPE_MIN_HEIGHT.max(side)
    } else {
        TOOLBAR_BOTTOM
            + SCOPE_MIN_HEIGHT
            + CELL_GAP
            + controls
            + if advanced {
                CELL_GAP + FIELD_HEIGHT
            } else {
                0.0
            }
    };
    height + 0.001 >= needed
}

#[test]
fn responsive_regions_preserve_real_children_and_concept_order_at_every_supported_height() {
    assert_eq!(
        DetailPage::ALL,
        [
            DetailPage::Main,
            DetailPage::Color,
            DetailPage::Clock,
            DetailPage::Filter,
            DetailPage::Input,
            DetailPage::Output,
        ]
    );
    for width in WIDTHS {
        for height in 64..=304 {
            let height = height as f32;
            for advanced in [false, true] {
                for page in DetailPage::ALL {
                    let context = format!("{width}x{height}, advanced={advanced}, {page:?}");
                    let body = Region {
                        x: 0.0,
                        y: 0.0,
                        width,
                        height,
                    };
                    let layout = arrange(width, height, advanced, page);
                    let mut regions = vec![layout.presets, layout.disclosure];
                    assert_inside(layout.presets, body, &context);
                    assert_inside(layout.disclosure, body, &context);
                    assert!(
                        layout.presets.width >= 140.0 + 72.0,
                        "preset actions/picker: {context}"
                    );
                    assert!(
                        layout.presets.height >= 26.0,
                        "preset row height: {context}"
                    );
                    assert!(
                        layout.disclosure.width >= 60.0,
                        "disclosure target: {context}"
                    );
                    assert!(
                        layout.disclosure.height >= 9.0 * 1.3 + 6.0,
                        "disclosure height: {context}"
                    );
                    assert!(
                        right(layout.presets) + 8.0 <= layout.disclosure.x + 0.001,
                        "preset/disclosure gap: {context}"
                    );
                    assert_eq!(
                        layout.details.is_some(),
                        advanced,
                        "Details availability: {context}"
                    );

                    let core_required: &[usize] = if width < 740.0
                        && advanced
                        && height + 0.001 < TOOLBAR_BOTTOM + 2.0 * FIELD_HEIGHT + CELL_GAP
                    {
                        &[]
                    } else if width < 740.0 {
                        &[param::BITS, param::SR_REDUCE, param::MIX]
                    } else {
                        &[param::BITS, param::SR_REDUCE, param::DRIVE_DB, param::MIX]
                    };
                    let actual: Vec<_> = layout
                        .core
                        .iter()
                        .map(|control| control.parameter)
                        .collect();
                    assert_eq!(actual, core_required, "core presence/order: {context}");
                    let drive_in_core = actual.contains(&param::DRIVE_DB);
                    for control in &layout.core {
                        assert_inside(control.bounds, body, &context);
                        assert!(
                            control.bounds.y >= TOOLBAR_BOTTOM,
                            "core overlaps toolbar: {context}"
                        );
                        let minimum_width = if width < 740.0 { 94.0 } else { 68.0 };
                        assert!(
                            control.bounds.width >= minimum_width,
                            "native field width: {context}"
                        );
                        assert!(
                            control.bounds.width <= 94.0,
                            "stretched control bank: {context}"
                        );
                        assert!(
                            matches!(control.knob_size, None | Some(20.0) | Some(34.0)),
                            "native knob ladder: {context}"
                        );
                        assert!(
                            control.bounds.height + 0.001 >= control_height(control.knob_size),
                            "native heading/knob/value clipped: {context}"
                        );
                        regions.push(control.bounds);
                    }
                    if let (Some(first), Some(last)) = (layout.core.first(), layout.core.last()) {
                        let maximum_bank =
                            layout.core.len() as f32 * 94.0 + (layout.core.len() - 1) as f32 * 8.0;
                        assert!(
                            right(last.bounds) - first.bounds.x <= maximum_bank + 0.001,
                            "unbounded primary bank: {context}"
                        );
                        for pair in layout.core.windows(2) {
                            assert!(
                                right(pair[0].bounds) + CELL_GAP <= pair[1].bounds.x + 0.001,
                                "core spacing: {context}"
                            );
                        }
                    }

                    if let Some(details) = &layout.details {
                        assert_eq!(details.page, page, "selected page: {context}");
                        assert_inside(details.bounds, body, &context);
                        assert!(
                            details.bounds.y >= TOOLBAR_BOTTOM,
                            "Details overlaps toolbar: {context}"
                        );
                        assert_inside(details.selector, details.bounds, &context);
                        assert_inside(details.content, details.bounds, &context);
                        assert_separate(details.selector, details.content, &context);
                        assert!(
                            details.selector.width >= 88.0,
                            "six-page selector: {context}"
                        );
                        assert!(
                            details.selector.height + 0.001 >= FIELD_HEIGHT,
                            "selector heading: {context}"
                        );
                        assert!(
                            details.content.height + 0.001 >= FIELD_HEIGHT,
                            "detail field height: {context}"
                        );
                        if width == 430.0 && height == 64.0 {
                            for (child, parent) in [
                                (layout.presets, body),
                                (layout.disclosure, body),
                                (details.bounds, body),
                                (details.selector, details.bounds),
                                (details.content, details.bounds),
                            ] {
                                assert!(child.x >= parent.x && child.y >= parent.y, "{context}");
                            }
                            assert!(right(layout.presets) + 8.0 <= layout.disclosure.x);
                            assert!(details.bounds.y >= bottom(layout.presets));
                            assert!(details.selector.height >= FIELD_HEIGHT);
                            assert!(details.content.height >= FIELD_HEIGHT);
                            assert!(details.content.width >= 4.0 * 68.0 + 3.0 * CELL_GAP);
                            assert!(right(details.selector) + 8.0 <= details.content.x);
                        }
                        let expected = required_cells(page, drive_in_core);
                        assert_eq!(
                            details.cells,
                            expected.as_slice(),
                            "detail children/Drive ownership: {context}"
                        );
                        assert_eq!(
                            details.cell_bounds.len(),
                            expected.len(),
                            "missing actual cell rectangles: {context}"
                        );
                        for cell in &details.cell_bounds {
                            assert_inside(*cell, details.content, &context);
                            assert!(
                                cell.width >= 68.0,
                                "six-character numeric/choice budget: {context}"
                            );
                            assert!(
                                cell.height + 0.001 >= FIELD_HEIGHT,
                                "detail child clipped: {context}"
                            );
                        }
                        for pair in details.cell_bounds.windows(2) {
                            assert!(
                                right(pair[0]) + CELL_GAP <= pair[1].x + 0.001,
                                "detail cell spacing: {context}"
                            );
                        }
                        if matches!(page, DetailPage::Input | DetailPage::Output) {
                            assert_eq!(
                                details.cells.len(),
                                4,
                                "all stereo peak/RMS readings: {context}"
                            );
                            assert!(
                                details.content.width + 0.001 >= 4.0 * 68.0 + 3.0 * CELL_GAP,
                                "last RMS field budget: {context}"
                            );
                            assert_inside(details.cell_bounds[3], details.content, &context);
                        }
                        regions.push(details.bounds);
                    }

                    let graph_possible = scope_fits(width, height, advanced, None);
                    assert_eq!(
                        layout.scope.is_some(),
                        graph_possible,
                        "useful waveform availability: {context}"
                    );
                    if let Some(scope) = layout.scope {
                        assert_inside(scope, body, &context);
                        assert!(
                            scope.y >= TOOLBAR_BOTTOM,
                            "scope overlaps toolbar: {context}"
                        );
                        assert!(
                            scope.height + 0.001 >= SCOPE_MIN_HEIGHT,
                            "legend/axes/signal budget: {context}"
                        );
                        assert!(scope.width >= 200.0, "paired waveform width: {context}");
                        regions.push(scope);
                        let expected_knob = [Some(34.0), Some(20.0), None]
                            .into_iter()
                            .find(|knob| scope_fits(width, height, advanced, *knob))
                            .unwrap();
                        assert!(
                            layout
                                .core
                                .iter()
                                .all(|control| control.knob_size == expected_knob),
                            "largest knob preserving a useful plot: {context}"
                        );
                        if width < 740.0 {
                            assert!(
                                layout.input_meter.is_none() && layout.output_meter.is_none(),
                                "compact full-width scope: {context}"
                            );
                            assert!(
                                (scope.x - 4.0).abs() < 0.001
                                    && (scope.width - (width - 8.0)).abs() < 0.001,
                                "compact scope spans body: {context}"
                            );
                        } else {
                            let input = layout.input_meter.expect("medium/expanded input meter");
                            let output = layout.output_meter.expect("medium/expanded output meter");
                            for meter in [input, output] {
                                assert_inside(meter, body, &context);
                                assert!(
                                    meter.width >= 52.0 && meter.width <= 56.0,
                                    "44px canvas plus native meter column budget: {context}"
                                );
                                assert!(
                                    meter.height + 0.001 >= SCOPE_MIN_HEIGHT,
                                    "meter heading/readouts/scale: {context}"
                                );
                                assert!(
                                    (meter.y - scope.y).abs() < 0.001
                                        && (meter.height - scope.height).abs() < 0.001,
                                    "plot/meter alignment: {context}"
                                );
                                regions.push(meter);
                            }
                            assert!(
                                right(input) + CELL_GAP <= scope.x + 0.001,
                                "input/scope order: {context}"
                            );
                            assert!(
                                right(scope) + CELL_GAP <= output.x + 0.001,
                                "scope/output order: {context}"
                            );
                        }
                        if width >= 872.0 {
                            let divider =
                                layout.divider.expect("expanded scope/control separation");
                            assert!(
                                divider.width >= 1.0 && divider.width <= 2.0,
                                "native divider: {context}"
                            );
                            let output = layout.output_meter.unwrap();
                            assert!(
                                right(output) + 8.0 <= divider.x + 0.001,
                                "scope bank before divider: {context}"
                            );
                            assert!(
                                layout.core.iter().all(|control| control.bounds.x + 0.001
                                    >= right(divider) + 8.0
                                    && control.bounds.width <= 68.0),
                                "bounded right core bank: {context}"
                            );
                            if let Some(details) = &layout.details {
                                assert!(
                                    details.bounds.x + 0.001 >= right(divider) + 8.0,
                                    "expanded Details stays right: {context}"
                                );
                                assert!(
                                    details.selector.y + FIELD_HEIGHT + CELL_GAP
                                        <= details.content.y + 0.001,
                                    "expanded selector above fields: {context}"
                                );
                                assert!(
                                    layout
                                        .core
                                        .iter()
                                        .all(|control| bottom(control.bounds) + CELL_GAP
                                            <= details.bounds.y + 0.001),
                                    "advanced below main: {context}"
                                );
                            }
                        } else {
                            assert!(
                                layout
                                    .core
                                    .iter()
                                    .all(|control| bottom(scope) + CELL_GAP
                                        <= control.bounds.y + 0.001),
                                "scope above footer: {context}"
                            );
                            assert!(
                                (layout.core[0].bounds.x - 4.0).abs() < 0.001,
                                "bounded footer anchored at inset: {context}"
                            );
                            if let Some(details) = &layout.details {
                                assert!(
                                    layout
                                        .core
                                        .iter()
                                        .all(|control| bottom(control.bounds) + CELL_GAP
                                            <= details.bounds.y + 0.001),
                                    "Details below core: {context}"
                                );
                            }
                        }
                    } else {
                        assert!(
                            layout.input_meter.is_none() && layout.output_meter.is_none(),
                            "no unreadable meter slivers: {context}"
                        );
                        if width >= 740.0 && advanced {
                            let details = layout.details.as_ref().unwrap();
                            let divider = layout
                                .divider
                                .expect("wide minimum core/details separation");
                            assert!(
                                right(layout.core.last().unwrap().bounds) + 8.0
                                    <= divider.x + 0.001,
                                "minimum retains four core fields: {context}"
                            );
                            assert!(
                                right(divider) + 8.0 <= details.bounds.x + 0.001,
                                "wide details beside core: {context}"
                            );
                            assert!(
                                details.content.width + 0.001 >= 4.0 * 68.0 + 3.0 * CELL_GAP,
                                "wide minimum page budget: {context}"
                            );
                        }
                    }
                    if let Some(divider) = layout.divider {
                        assert_inside(divider, body, &context);
                        regions.push(divider);
                    }
                    for (index, region) in regions.iter().copied().enumerate() {
                        for other in regions.iter().copied().skip(index + 1) {
                            assert_separate(region, other, &context);
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn card_width_uses_the_chain_remainder_with_native_minimum_and_maximum() {
    for (available, expected) in [
        (0.0, 430.0),
        (80.0, 430.0),
        (429.0, 430.0),
        (430.0, 430.0),
        (431.0, 431.0),
        (739.0, 739.0),
        (740.0, 740.0),
        (871.0, 871.0),
        (872.0, 872.0),
        (899.0, 899.0),
        (900.0, 900.0),
        (1800.0, 900.0),
    ] {
        assert_eq!(
            card_width(available),
            expected,
            "chain remainder {available}"
        );
    }
}
