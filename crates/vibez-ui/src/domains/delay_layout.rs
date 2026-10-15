//! Fits Delay's reference response and musical controls to the measured device body.

use vibez_dsp::delay::param;

pub const MIN_WIDTH: f32 = 430.0;
pub const MEDIUM_WIDTH: f32 = 740.0;
pub const MAX_WIDTH: f32 = 900.0;
const FIELD_HEIGHT: f32 = 8.0 * 1.3 + 3.0 + 10.0 * 1.3 + 2.0;
const TOOLBAR_HEIGHT: f32 = 26.0;
const CONTENT_TOP: f32 = TOOLBAR_HEIGHT + 2.0;
const INSET: f32 = 4.0;
const GAP: f32 = 6.0;
const TITLE_HEIGHT: f32 = 10.4;
const RULER_HEIGHT: f32 = 13.0;
const GRAPH_MIN_HEIGHT: f32 = TITLE_HEIGHT + 3.0 + 30.0 + 3.0 + RULER_HEIGHT;
const METER_WIDTH: f32 = 52.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Region {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Page {
    #[default]
    Main,
    Left,
    Right,
    Stereo,
    HighPass,
    LowPass,
    Duck,
    Timing,
}

impl Page {
    pub const ALL: [Self; 8] = [
        Self::Main,
        Self::Left,
        Self::Right,
        Self::Stereo,
        Self::HighPass,
        Self::LowPass,
        Self::Duck,
        Self::Timing,
    ];

    fn parameters(self) -> &'static [&'static [usize]] {
        match self {
            Self::Main => MAIN,
            Self::Left => &[
                &[param::TIME_MS],
                &[param::LEFT_DIVISION],
                &[param::LEFT_MODIFIER],
            ],
            Self::Right => &[
                &[param::RIGHT_TIME_MS],
                &[param::RIGHT_DIVISION],
                &[param::RIGHT_MODIFIER],
            ],
            Self::Stereo => &[&[param::ROUTING], &[param::LINK]],
            Self::HighPass => &[&[param::FEEDBACK_HP_ENABLED], &[param::FEEDBACK_HP_HZ]],
            Self::LowPass => &[&[param::FEEDBACK_LP_ENABLED], &[param::FEEDBACK_LP_HZ]],
            Self::Duck => &[&[param::DUCK]],
            Self::Timing => &[&[param::TIME_CHANGE]],
        }
    }
}

impl std::fmt::Display for Page {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Main => "Main",
            Self::Left => "Left",
            Self::Right => "Right",
            Self::Stereo => "Stereo",
            Self::HighPass => "HP",
            Self::LowPass => "LP",
            Self::Duck => "Duck",
            Self::Timing => "Timing",
        })
    }
}

// One active time field edits Free milliseconds or the combined musical division.
const MAIN: &[&[usize]] = &[
    &[param::SYNC],
    &[param::TIME_MS, param::LEFT_DIVISION, param::LEFT_MODIFIER],
    &[param::FEEDBACK],
    &[param::MIX],
];
const FULL_MAIN: &[&[usize]] = &[
    &[param::SYNC],
    &[param::TIME_MS, param::LEFT_DIVISION, param::LEFT_MODIFIER],
    &[param::FEEDBACK],
    &[param::MIX],
    &[param::ROUTING],
    &[param::LINK],
];

#[derive(Clone, Debug)]
pub struct Control {
    pub parameters: &'static [usize],
    pub bounds: Region,
    pub knob_size: Option<f32>,
}

#[derive(Clone, Copy, Debug)]
pub struct Graph {
    pub bounds: Region,
    pub title: Region,
    pub plot: Region,
    pub ruler: Region,
}

#[derive(Clone, Debug)]
pub struct Arrangement {
    pub toolbar: Region,
    pub presets: Region,
    pub disclosure: Region,
    pub main: Vec<Control>,
    pub selector: Option<Region>,
    pub details: Vec<Control>,
    pub graph: Option<Graph>,
    pub output_meter: Option<Region>,
}

pub fn card_width(available: f32) -> f32 {
    available.clamp(MIN_WIDTH, MAX_WIDTH)
}

fn controls(
    parameters: &'static [&'static [usize]],
    widths: &[f32],
    x: f32,
    y: f32,
    knob_size: Option<f32>,
) -> Vec<Control> {
    let mut left = x;
    parameters
        .iter()
        .zip(widths)
        .map(|(parameters, &width)| {
            let bounds = Region {
                x: left,
                y,
                width,
                height: FIELD_HEIGHT + knob_size.map_or(0.0, |size| size + 3.0),
            };
            left += width + GAP;
            Control {
                parameters,
                bounds,
                knob_size,
            }
        })
        .collect()
}

fn graph(bounds: Region) -> Graph {
    let title = Region {
        height: TITLE_HEIGHT,
        ..bounds
    };
    let plot = Region {
        y: bounds.y + TITLE_HEIGHT + 3.0,
        height: bounds.height - TITLE_HEIGHT - RULER_HEIGHT - 6.0,
        ..bounds
    };
    let ruler = Region {
        y: bounds.y + bounds.height - RULER_HEIGHT,
        height: RULER_HEIGHT,
        ..bounds
    };
    Graph {
        bounds,
        title,
        plot,
        ruler,
    }
}

pub fn arrange(width: f32, height: f32, advanced: bool, page: Page) -> Arrangement {
    let toolbar = Region {
        x: 0.0,
        y: 0.0,
        width,
        height: TOOLBAR_HEIGHT,
    };
    let mut layout = Arrangement {
        toolbar,
        presets: Region {
            x: INSET,
            width: width - 104.0,
            ..toolbar
        },
        disclosure: Region {
            x: width - 92.0,
            width: 88.0,
            ..toolbar
        },
        main: Vec::new(),
        selector: None,
        details: Vec::new(),
        graph: None,
        output_meter: None,
    };
    // Short bodies exchange the control strip for a selected page rather than clip it.
    let main_visible = !advanced || height >= 180.0;
    let full_main = width >= MEDIUM_WIDTH && height >= 180.0;
    let detail_height = if advanced { FIELD_HEIGHT } else { 0.0 };
    let between_controls = if advanced && main_visible { GAP } else { 0.0 };
    let available = height - CONTENT_TOP;
    let minimum_controls =
        if main_visible { FIELD_HEIGHT } else { 0.0 } + detail_height + between_controls;
    let graph_visible = available >= minimum_controls + GAP + GRAPH_MIN_HEIGHT;
    let main_room = available
        - detail_height
        - between_controls
        - if graph_visible {
            GRAPH_MIN_HEIGHT + GAP
        } else {
            0.0
        };
    let knob_size = if main_visible {
        [34.0, 20.0]
            .into_iter()
            .find(|size| FIELD_HEIGHT + size + 3.0 <= main_room)
    } else {
        None
    };
    let main_height = if main_visible {
        FIELD_HEIGHT + knob_size.map_or(0.0, |size| size + 3.0)
    } else {
        0.0
    };
    let mut y = CONTENT_TOP;
    if graph_visible {
        let graph_height = available - main_height - detail_height - between_controls - GAP;
        let meter = width >= MEDIUM_WIDTH;
        let bounds = Region {
            x: INSET,
            y,
            width: width - 2.0 * INSET - if meter { METER_WIDTH + GAP } else { 0.0 },
            height: graph_height,
        };
        layout.graph = Some(graph(bounds));
        if meter {
            layout.output_meter = Some(Region {
                x: width - INSET - METER_WIDTH,
                width: METER_WIDTH,
                ..bounds
            });
        }
        y += graph_height + GAP;
    }
    if main_visible {
        layout.main = if full_main {
            let available = width - 2.0 * INSET - 5.0 * GAP;
            let widths = [0.11, 0.24, 0.13, 0.13, 0.22, 0.17].map(|share| available * share);
            controls(FULL_MAIN, &widths, INSET, y, knob_size)
        } else {
            controls(MAIN, &[58.0, 146.0, 76.0, 76.0], INSET, y, knob_size)
        };
        y += main_height + between_controls;
    }
    if advanced {
        layout.selector = Some(Region {
            x: INSET,
            y,
            width: 88.0,
            height: FIELD_HEIGHT,
        });
        let widths = if matches!(page, Page::Left | Page::Right) {
            [98.0, 98.0, 76.0, 76.0]
        } else {
            [58.0, 98.0, 76.0, 76.0]
        };
        layout.details = controls(page.parameters(), &widths, INSET + 96.0, y, None);
    }
    layout
}
