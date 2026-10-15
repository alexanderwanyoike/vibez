//! Verifies Delay's measured native children fit without losing musical controls.

use super::delay_layout::{arrange, card_width, Page, Region};

const FIELD: f32 = 8.0 * 1.3 + 3.0 + 10.0 * 1.3 + 2.0;
const PAGE_PARAMS: [&[usize]; 8] = [
    &[3, 0, 4, 5, 1, 2],
    &[0, 4, 5],
    &[8, 9, 10],
    &[6, 7],
    &[11, 12],
    &[13, 14],
    &[15],
    &[16],
];

fn right(r: Region) -> f32 {
    r.x + r.width
}
fn bottom(r: Region) -> f32 {
    r.y + r.height
}
fn inside(r: Region, parent: Region) {
    assert!(
        r.x >= parent.x && r.y >= parent.y,
        "{r:?} outside {parent:?}"
    );
    assert!(r.width > 0.0 && r.height > 0.0);
    assert!(right(r) <= right(parent) + 0.001 && bottom(r) <= bottom(parent) + 0.001);
}
fn separate(a: Region, b: Region) {
    assert!(
        right(a) <= b.x + 0.001
            || right(b) <= a.x + 0.001
            || bottom(a) <= b.y + 0.001
            || bottom(b) <= a.y + 0.001,
        "{a:?} overlaps {b:?}"
    );
}

#[test]
fn native_delay_geometry_preserves_pages_and_the_reference_control_strip() {
    assert_eq!(
        Page::ALL,
        [
            Page::Main,
            Page::Left,
            Page::Right,
            Page::Stereo,
            Page::HighPass,
            Page::LowPass,
            Page::Duck,
            Page::Timing
        ]
    );
    for (available, expected) in [
        (0.0, 430.0),
        (429.0, 430.0),
        (430.0, 430.0),
        (739.0, 739.0),
        (740.0, 740.0),
        (899.0, 899.0),
        (900.0, 900.0),
        (1800.0, 900.0),
    ] {
        assert_eq!(card_width(available), expected);
    }
    for width in [430.0, 431.0, 739.0, 740.0, 741.0, 899.0, 900.0] {
        for height in 64..=304 {
            for advanced in [false, true] {
                let mut reachable = [false; 17];
                for (page_index, page) in Page::ALL.into_iter().enumerate() {
                    let layout = arrange(width, height as f32, advanced, page);
                    let body = Region {
                        x: 0.0,
                        y: 0.0,
                        width,
                        height: height as f32,
                    };
                    assert_eq!(layout.toolbar.height, 26.0);
                    inside(layout.toolbar, body);
                    inside(layout.presets, layout.toolbar);
                    inside(layout.disclosure, layout.toolbar);
                    separate(layout.presets, layout.disclosure);
                    let mut regions = vec![layout.toolbar];
                    let mut shown = Vec::new();
                    for cell in layout.main.iter().chain(&layout.details) {
                        inside(cell.bounds, body);
                        assert!(cell.bounds.y >= 28.0);
                        let needed = FIELD + cell.knob_size.map_or(0.0, |size| size + 3.0);
                        assert!(cell.bounds.height + 0.001 >= needed);
                        assert!(cell.bounds.width >= 58.0 && cell.bounds.width <= 220.0);
                        shown.extend_from_slice(cell.parameters);
                        for &index in cell.parameters {
                            reachable[index] = true;
                        }
                        regions.push(cell.bounds);
                    }
                    if advanced {
                        let selector = layout
                            .selector
                            .expect("all eight Advanced pages remain reachable");
                        inside(selector, body);
                        assert!(selector.width >= 88.0 && selector.height + 0.001 >= FIELD);
                        regions.push(selector);
                        let page_values: Vec<_> = layout
                            .details
                            .iter()
                            .flat_map(|cell| cell.parameters.iter().copied())
                            .collect();
                        assert_eq!(page_values, PAGE_PARAMS[page_index]);
                    } else {
                        assert!(layout.selector.is_none() && layout.details.is_empty());
                    }
                    if !advanced || height >= 180 || page == Page::Main {
                        for index in [3, 0, 4, 5, 1, 2] {
                            assert!(shown.contains(&index), "Main lost at {width}x{height}");
                        }
                    }
                    if height == 64 {
                        assert_eq!(64.0 - bottom(layout.toolbar) - 2.0, 36.0);
                        assert!(layout.graph.is_none() && layout.output_meter.is_none());
                        assert!(layout
                            .main
                            .iter()
                            .chain(&layout.details)
                            .all(|c| c.knob_size.is_none()));
                        if advanced {
                            let selector = layout.selector.unwrap();
                            assert_eq!(selector.x, 4.0);
                            assert!(layout.details[0].bounds.x >= right(selector) + 8.0);
                            for (cell, minimum) in
                                layout.details.iter().zip([58.0, 98.0, 76.0, 76.0])
                            {
                                assert!(cell.bounds.width + 0.001 >= minimum);
                            }
                        }
                    }
                    if let Some(graph) = layout.graph {
                        inside(graph.bounds, body);
                        for part in [graph.title, graph.plot, graph.ruler] {
                            inside(part, graph.bounds);
                        }
                        assert!(
                            graph.title.height >= 10.4
                                && graph.plot.height >= 30.0
                                && graph.ruler.height >= 13.0
                        );
                        separate(graph.title, graph.plot);
                        separate(graph.plot, graph.ruler);
                        assert!(graph.bounds.width >= 300.0);
                        regions.push(graph.bounds);
                        if width >= 740.0 {
                            let meter = layout
                                .output_meter
                                .expect("slim real output beside response");
                            assert_eq!(meter.width, 52.0);
                            assert_eq!(meter.y, graph.bounds.y);
                            assert_eq!(meter.height, graph.bounds.height);
                            assert!(meter.x >= right(graph.bounds) + 6.0);
                            inside(meter, body);
                            regions.push(meter);
                        } else {
                            assert!(layout.output_meter.is_none());
                        }
                    } else {
                        assert!(layout.output_meter.is_none());
                    }
                    if height >= 180 {
                        assert!(layout.graph.is_some(), "readable reference omitted");
                    }
                    if width == 900.0 && height == 304 {
                        let graph = layout.graph.unwrap();
                        assert!(graph.bounds.width >= 800.0 && graph.plot.height >= 100.0);
                        assert_eq!(layout.main.len(), 6);
                        assert_eq!(layout.main[0].bounds.x, 4.0);
                        for pair in layout.main.windows(2) {
                            assert_eq!(pair[0].bounds.y, pair[1].bounds.y);
                            assert!(pair[1].bounds.x >= right(pair[0].bounds) + 6.0);
                        }
                        assert!(layout
                            .main
                            .iter()
                            .all(|c| c.bounds.y >= bottom(graph.bounds) + 6.0));
                        for index in [6, 7] {
                            assert!(shown.contains(&index));
                        }
                    }
                    for (i, a) in regions.iter().enumerate() {
                        for b in regions.iter().skip(i + 1) {
                            separate(*a, *b);
                        }
                    }
                }
                if advanced {
                    assert!(reachable.into_iter().all(|covered| covered));
                }
            }
        }
    }
}
