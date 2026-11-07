//! The built-in theme collection and the `.vzt` user-theme service.
//!
//! Built-ins are authored in OKLCH through the dark/light builders below,
//! so every theme fills the complete [`ThemePalette`] from one tinted
//! surface ramp. User themes are plain `.vzt` JSON files scanned from the
//! config themes directory, plugin-cache style.

use std::path::PathBuf;

use iced::Color;

use crate::theme::{contrast_ratio, ClipStyle, LaneStyle, Oklch, ThemePalette};

const fn rgb8(r: u8, g: u8, b: u8) -> Color {
    Color {
        r: r as f32 / 255.0,
        g: g as f32 / 255.0,
        b: b as f32 / 255.0,
        a: 1.0,
    }
}

const fn lch(lightness: f32, chroma: f32, hue: f32) -> Oklch {
    Oklch::new(lightness, chroma, hue)
}

/// Move `c` toward white by `f` (0..1).
fn lighten(c: Color, f: f32) -> Color {
    Color {
        r: c.r + (1.0 - c.r) * f,
        g: c.g + (1.0 - c.g) * f,
        b: c.b + (1.0 - c.b) * f,
        a: c.a,
    }
}

/// Move `c` toward black by `f` (0..1).
fn darken(c: Color, f: f32) -> Color {
    Color {
        r: c.r * (1.0 - f),
        g: c.g * (1.0 - f),
        b: c.b * (1.0 - f),
        a: c.a,
    }
}

fn alpha(c: Color, a: f32) -> Color {
    Color { a, ..c }
}

/// Track hues in rainbow order (red, orange, yellow, green, cyan, blue,
/// purple, magenta), taken from the palettes the themes shipped with.
const HUES_CLASSIC: [f32; 8] = [23.0, 62.0, 102.0, 147.0, 210.0, 266.0, 305.0, 342.0];
const HUES_MUTED: [f32; 8] = [21.0, 62.0, 96.0, 147.0, 211.0, 268.0, 314.0, 352.0];
const HUES_NEON: [f32; 8] = [17.0, 60.0, 117.0, 142.0, 175.0, 246.0, 311.0, 340.0];
const HUES_PASTEL: [f32; 8] = [3.0, 53.0, 87.0, 143.0, 183.0, 260.0, 305.0, 336.0];
const HUES_WARM: [f32; 8] = [31.0, 59.0, 96.0, 120.0, 171.0, 267.0, 325.0, 6.0];
const HUES_SOLARIZED: [f32; 8] = [27.0, 40.0, 86.0, 128.0, 187.0, 245.0, 279.0, 356.0];

/// Yellow looks olive at the lightness the other hues share, so it sits
/// this much higher.
const YELLOW_LIFT: f32 = 0.1;

/// Eight track colours at one perceived lightness and chroma, keeping
/// `hues`, with as much chroma as sRGB allows each one.
fn matched(hues: [f32; 8], lightness: f32, chroma: f32) -> [Oklch; 8] {
    hues.map(|hue| {
        let lift = if (70.0..=120.0).contains(&hue) {
            YELLOW_LIFT
        } else {
            0.0
        };
        lch(lightness + lift, chroma, hue).fitted()
    })
}

/// EQ bands take the theme's own orange, blue, green and red tracks (LF,
/// LMF, HMF, HF), so they belong to its palette.
fn eq_bands(tracks: [Oklch; 8]) -> [Oklch; 4] {
    [tracks[1], tracks[5], tracks[3], tracks[0]]
}

/// Meter green, yellow and red, shared by every theme so levels read the
/// same everywhere.
const DARK_METER: [Oklch; 3] = [
    lch(0.80, 0.17, 150.0),
    lch(0.86, 0.16, 95.0),
    lch(0.68, 0.19, 25.0),
];
const LIGHT_METER: [Oklch; 3] = [
    lch(0.66, 0.16, 150.0),
    lch(0.76, 0.15, 85.0),
    lch(0.58, 0.19, 25.0),
];

/// Contrast floors for the dimmer text tiers on every surface they are
/// drawn on. Dim text carries 8 to 10 px captions and values, so it keeps
/// WCAG AA's 4.5:1. Muted text only carries hints, placeholders and
/// disabled states, so it keeps the 3:1 AA allows for large text.
const TEXT_DIM_CONTRAST: f32 = 4.5;
const TEXT_MUTED_CONTRAST: f32 = 3.0;

/// WCAG's 3:1 for graphics cannot hold for a knob ring, which sits between
/// the body inside it, the strip round it and the band colour drawn over
/// it: a ring 3:1 off a dark body is brighter than the dimmer band colours.
/// 1.4:1 is the smallest step that reads at the ring's 2.5 px on every
/// built-in theme.
const KNOB_RING_STEP: f32 = 1.4;

/// The dark text tiers' lightness: body, dim and muted.
const DARK_TEXT: [f32; 3] = [0.94, 0.76, 0.62];
const LIGHT_TEXT: [f32; 3] = [0.25, 0.45, 0.56];

/// A theme authored in OKLCH, as the approved theme mockup builds it.
struct Seed {
    name: &'static str,
    /// The lane shade of a dark theme, the panel shade of a light one.
    /// Every surface keeps its hue and chroma and steps only in lightness.
    base: Oklch,
    /// Lightness of the body, dim and muted text, tinted with `base`.
    text: [f32; 3],
    accent: Color,
    tracks: [Oklch; 8],
    /// LF, LMF, HMF and HF, or [`eq_bands`] of `tracks` when `None`.
    eq: Option<[Oklch; 4]>,
}

impl Seed {
    fn eq(&self) -> [Color; 4] {
        self.eq
            .unwrap_or_else(|| eq_bands(self.tracks))
            .map(Oklch::color)
    }
}

/// The first colour `nearer` returns, stepping 0.01 at a time from its
/// nominal shade, that keeps `floor` against every one of `behind`.
fn holding(floor: f32, behind: &[Color], nearer: impl Fn(f32) -> Color) -> Color {
    (0..=60)
        .map(|step| nearer(step as f32 * 0.01))
        .find(|&colour| {
            behind
                .iter()
                .all(|&background| contrast_ratio(colour, background) >= floor)
        })
        .unwrap_or_else(|| nearer(0.6))
}

/// Fill the dim and muted text tiers at their nominal lightness, or as far
/// nearer the body text as each needs to hold its floor on every surface
/// it is drawn on.
fn with_text_tiers(
    mut palette: ThemePalette,
    dim: impl Fn(f32) -> Color,
    muted: impl Fn(f32) -> Color,
) -> ThemePalette {
    let surfaces = palette.text_surfaces().map(|(_, surface)| surface);
    palette.text_dim = holding(TEXT_DIM_CONTRAST, &surfaces, dim);
    palette.text_muted = holding(TEXT_MUTED_CONTRAST, &surfaces, muted);
    palette
}

/// Fill a complete palette from a dark seed: surfaces step up in lightness
/// from the lane, text is light, editor grids sit between.
fn dark(seed: Seed) -> ThemePalette {
    let Oklch {
        lightness: b,
        chroma: c,
        hue: h,
    } = seed.base;
    let shade = |lightness: f32| lch(lightness, c, h).color();
    let tint = |lightness: f32, chroma: f32| lch(lightness, chroma, h).color();
    let [text, dim, muted] = seed.text;
    let [meter_green, meter_yellow, meter_red] = DARK_METER.map(Oklch::color);
    let [eq_lf, eq_lmf, eq_hmf, eq_hf] = seed.eq();
    let accent = seed.accent;
    let raised = shade(b + 0.07);
    let palette = ThemePalette {
        name: seed.name.to_string(),
        bg_dark: shade(b),
        bg_surface: shade(b + 0.035),
        bg_elevated: raised,
        bg_hover: shade(b + 0.145),
        control: Some(shade(b + 0.11)),
        text: tint(text, c * 0.6),
        text_dim: tint(dim, c * 0.8),
        text_muted: tint(muted, c),
        accent,
        accent_dim: darken(accent, 0.4),
        border: shade(b + 0.09),
        border_light: shade(b + 0.16),
        divider: shade(b + 0.09),
        success: meter_green,
        danger: meter_red,
        playhead: alpha(Color::WHITE, 0.8),
        meter_green,
        meter_yellow,
        meter_red,
        display_bg: shade(b - 0.035),
        knob_arc: tint(muted, c),
        knob_body: shade(b),
        knob_body_engaged: shade(b + 0.035),
        knob_track: holding(
            KNOB_RING_STEP,
            &[shade(b), shade(b + 0.035), raised],
            |step| shade(b + 0.17 + step),
        ),
        fader_handle: tint(0.86, c * 0.6),
        track_colors: seed.tracks.map(Oklch::color),
        eq_lf,
        eq_lmf,
        eq_hmf,
        eq_hf,
        clip_body: alpha(accent, 0.35),
        clip_border: alpha(lighten(accent, 0.2), 0.9),
        waveform: alpha(lighten(accent, 0.2), 0.6),
        ruler_line: alpha(shade(b + 0.09), 0.5),
        grid_bar: shade(b + 0.24),
        grid_beat: shade(b + 0.13),
        grid_sub: shade(b + 0.09),
        piano_white_row: shade(b + 0.035),
        piano_black_row: shade(b),
        piano_octave_line: shade(b + 0.11),
        piano_grid: shade(b + 0.05),
        piano_white_key: tint(0.84, c * 0.6),
        piano_black_key: shade(b - 0.05),
        piano_key_label: tint(0.45, c),
        clip_style: ClipStyle {
            ink: alpha(tint(0.17, c * 0.5), ClipStyle::DARK.ink.a),
            ..ClipStyle::DARK
        },
        lane_style: LaneStyle::DARK,
    };
    with_text_tiers(
        palette,
        |step| tint(dim + step, c * 0.8),
        |step| tint(muted + step, c),
    )
}

/// Fill a complete palette from a light seed: the panel is paper, lanes
/// and wells sink below it, raised cards lift above it, text is ink.
fn light(seed: Seed) -> ThemePalette {
    let Oklch {
        lightness: b,
        chroma: c,
        hue: h,
    } = seed.base;
    let shade = |lightness: f32| lch(lightness, c, h).color();
    let tint = |lightness: f32, chroma: f32| lch(lightness, chroma, h).color();
    let [text, dim, muted] = seed.text;
    let [meter_green, meter_yellow, meter_red] = LIGHT_METER.map(Oklch::color);
    let [eq_lf, eq_lmf, eq_hmf, eq_hf] = seed.eq();
    let accent = seed.accent;
    let raised = shade((b + 0.025).min(0.995));
    let palette = ThemePalette {
        name: seed.name.to_string(),
        bg_dark: shade(b - 0.035),
        bg_surface: shade(b),
        bg_elevated: raised,
        bg_hover: shade(b - 0.075),
        control: Some(shade(b - 0.05)),
        text: tint(text, c * 2.0),
        text_dim: tint(dim, c * 2.0),
        text_muted: tint(muted, c * 2.0),
        accent,
        accent_dim: lighten(accent, 0.3),
        border: shade(b - 0.08),
        border_light: shade(b - 0.15),
        divider: shade(b - 0.08),
        success: meter_green,
        danger: meter_red,
        playhead: alpha(Color::BLACK, 0.7),
        meter_green,
        meter_yellow,
        meter_red,
        display_bg: shade(b - 0.055),
        knob_arc: tint(muted, c * 2.0),
        knob_body: shade(b - 0.05),
        knob_body_engaged: shade(b - 0.075),
        knob_track: holding(
            KNOB_RING_STEP,
            &[shade(b - 0.05), shade(b), raised],
            |step| shade(b - 0.16 - step),
        ),
        fader_handle: tint(0.42, c * 2.0),
        track_colors: seed.tracks.map(Oklch::color),
        eq_lf,
        eq_lmf,
        eq_hmf,
        eq_hf,
        clip_body: alpha(accent, 0.35),
        clip_border: alpha(darken(accent, 0.2), 0.9),
        waveform: alpha(darken(accent, 0.25), 0.65),
        ruler_line: alpha(shade(b - 0.08), 0.5),
        grid_bar: shade(b - 0.2),
        grid_beat: shade(b - 0.12),
        grid_sub: shade(b - 0.08),
        piano_white_row: shade(b),
        piano_black_row: shade(b - 0.05),
        piano_octave_line: shade(b - 0.15),
        piano_grid: shade(b - 0.08),
        piano_white_key: raised,
        piano_black_key: tint(0.3, c),
        piano_key_label: tint(muted, c * 2.0),
        clip_style: ClipStyle {
            ink: alpha(tint(0.2, c), ClipStyle::LIGHT.ink.a),
            ..ClipStyle::LIGHT
        },
        lane_style: LaneStyle::LIGHT,
    };
    with_text_tiers(
        palette,
        |step| tint(dim - step, c * 2.0),
        |step| tint(muted - step, c * 2.0),
    )
}

/// The founding vibez look: near-neutral charcoal, orange accent. The
/// approved theme mockup's values, as are Gruvbox's, Nord's and Porcelain's.
pub(crate) fn charcoal() -> ThemePalette {
    dark(Seed {
        name: "Charcoal",
        base: lch(0.215, 0.006, 260.0),
        text: DARK_TEXT,
        accent: lch(0.74, 0.175, 55.0).color(),
        tracks: [
            lch(0.70, 0.15, 25.0),
            lch(0.74, 0.14, 58.0),
            lch(0.82, 0.135, 95.0),
            lch(0.73, 0.14, 150.0),
            lch(0.73, 0.11, 200.0),
            lch(0.69, 0.13, 255.0),
            lch(0.67, 0.14, 300.0),
            lch(0.70, 0.15, 350.0),
        ],
        eq: Some([
            lch(0.72, 0.12, 60.0),
            lch(0.70, 0.11, 250.0),
            lch(0.74, 0.12, 155.0),
            lch(0.69, 0.14, 25.0),
        ]),
    })
}

/// All built-in themes, default first.
pub fn builtins() -> Vec<ThemePalette> {
    vec![
        charcoal(),
        dark(Seed {
            name: "Obsidian",
            base: lch(0.175, 0.006, 270.0),
            text: DARK_TEXT,
            accent: rgb8(125, 200, 255),
            tracks: matched(HUES_CLASSIC, 0.70, 0.15),
            eq: None,
        }),
        dark(Seed {
            name: "Berlin",
            base: lch(0.16, 0.003, 30.0),
            text: [0.88, 0.72, 0.6],
            accent: rgb8(192, 57, 43),
            tracks: matched(HUES_MUTED, 0.64, 0.105),
            eq: None,
        }),
        dark(Seed {
            name: "Acid",
            base: lch(0.17, 0.014, 128.0),
            text: DARK_TEXT,
            accent: rgb8(170, 255, 0),
            tracks: matched(HUES_NEON, 0.78, 0.2),
            eq: None,
        }),
        dark(Seed {
            name: "Vaporwave",
            base: lch(0.215, 0.05, 298.0),
            text: DARK_TEXT,
            accent: rgb8(255, 113, 206),
            tracks: matched(HUES_NEON, 0.74, 0.19),
            eq: Some([
                lch(0.74, 0.18, 343.0),
                lch(0.76, 0.13, 240.0),
                lch(0.84, 0.17, 165.0),
                lch(0.66, 0.2, 300.0),
            ]),
        }),
        dark(Seed {
            name: "Nord",
            base: lch(0.30, 0.032, 262.0),
            text: [0.95, 0.80, 0.68],
            accent: lch(0.78, 0.10, 215.0).color(),
            tracks: [
                lch(0.65, 0.16, 18.0),
                lch(0.72, 0.15, 50.0),
                lch(0.83, 0.145, 90.0),
                lch(0.74, 0.15, 140.0),
                lch(0.75, 0.115, 190.0),
                lch(0.69, 0.135, 245.0),
                lch(0.66, 0.14, 320.0),
                lch(0.62, 0.15, 270.0),
            ],
            eq: Some([
                lch(0.73, 0.13, 55.0),
                lch(0.69, 0.13, 245.0),
                lch(0.75, 0.14, 140.0),
                lch(0.66, 0.15, 20.0),
            ]),
        }),
        dark(Seed {
            name: "Dracula",
            base: lch(0.285, 0.025, 278.0),
            text: DARK_TEXT,
            accent: rgb8(189, 147, 249),
            tracks: matched(
                [24.0, 67.0, 113.0, 148.0, 213.0, 270.0, 302.0, 347.0],
                0.74,
                0.16,
            ),
            eq: None,
        }),
        dark(Seed {
            name: "Gruvbox",
            base: lch(0.255, 0.012, 70.0),
            text: DARK_TEXT,
            accent: lch(0.73, 0.165, 55.0).color(),
            tracks: [
                lch(0.60, 0.19, 29.0),
                lch(0.69, 0.17, 52.0),
                lch(0.79, 0.155, 85.0),
                lch(0.72, 0.15, 120.0),
                lch(0.72, 0.11, 165.0),
                lch(0.66, 0.10, 215.0),
                lch(0.63, 0.13, 335.0),
                lch(0.65, 0.15, 5.0),
            ],
            eq: Some([
                lch(0.70, 0.15, 60.0),
                lch(0.67, 0.10, 215.0),
                lch(0.72, 0.14, 120.0),
                lch(0.62, 0.18, 29.0),
            ]),
        }),
        dark(Seed {
            name: "Solarized Dark",
            base: lch(0.255, 0.045, 220.0),
            text: DARK_TEXT,
            accent: rgb8(38, 139, 210),
            tracks: matched(HUES_SOLARIZED, 0.66, 0.15),
            eq: None,
        }),
        dark(Seed {
            name: "Monokai",
            base: lch(0.27, 0.012, 115.0),
            text: DARK_TEXT,
            accent: rgb8(166, 226, 46),
            tracks: matched(
                [7.0, 62.0, 103.0, 127.0, 212.0, 265.0, 298.0, 340.0],
                0.74,
                0.17,
            ),
            eq: None,
        }),
        dark(Seed {
            name: "Tokyo Night",
            base: lch(0.225, 0.025, 278.0),
            text: DARK_TEXT,
            accent: rgb8(122, 162, 247),
            tracks: matched(
                [10.0, 51.0, 75.0, 130.0, 182.0, 264.0, 299.0, 350.0],
                0.74,
                0.15,
            ),
            eq: None,
        }),
        dark(Seed {
            name: "Catppuccin",
            base: lch(0.245, 0.03, 284.0),
            text: DARK_TEXT,
            accent: rgb8(203, 166, 247),
            tracks: matched(HUES_PASTEL, 0.80, 0.11),
            eq: None,
        }),
        dark(Seed {
            name: "One Dark",
            base: lch(0.28, 0.018, 264.0),
            text: DARK_TEXT,
            accent: rgb8(97, 175, 239),
            tracks: matched(
                [17.0, 64.0, 82.0, 133.0, 206.0, 245.0, 318.0, 350.0],
                0.72,
                0.14,
            ),
            eq: None,
        }),
        dark(Seed {
            name: "Deep Sea",
            base: lch(0.215, 0.03, 236.0),
            text: DARK_TEXT,
            accent: rgb8(46, 196, 182),
            tracks: matched(
                [35.0, 58.0, 87.0, 138.0, 183.0, 238.0, 300.0, 5.0],
                0.70,
                0.13,
            ),
            eq: None,
        }),
        dark(Seed {
            name: "Espresso",
            base: lch(0.235, 0.022, 55.0),
            text: DARK_TEXT,
            accent: rgb8(212, 163, 115),
            tracks: matched(HUES_WARM, 0.68, 0.11),
            eq: None,
        }),
        dark(Seed {
            name: "Slate",
            base: lch(0.245, 0.018, 255.0),
            text: DARK_TEXT,
            accent: rgb8(83, 155, 245),
            tracks: matched(HUES_CLASSIC, 0.70, 0.14),
            eq: None,
        }),
        dark(Seed {
            name: "Rose Pine",
            base: lch(0.22, 0.028, 291.0),
            text: DARK_TEXT,
            accent: rgb8(235, 188, 186),
            tracks: matched(
                [4.0, 30.0, 75.0, 228.0, 195.0, 305.0, 270.0, 345.0],
                0.74,
                0.12,
            ),
            eq: None,
        }),
        dark(Seed {
            name: "High Contrast",
            base: lch(0.10, 0.0, 0.0),
            text: [1.0, 0.86, 0.74],
            accent: rgb8(255, 215, 0),
            tracks: matched(HUES_NEON, 0.80, 0.2),
            eq: None,
        }),
        light(Seed {
            name: "Solarized Light",
            base: lch(0.965, 0.022, 88.0),
            text: LIGHT_TEXT,
            accent: rgb8(38, 139, 210),
            tracks: matched(HUES_SOLARIZED, 0.62, 0.15),
            eq: None,
        }),
        light(Seed {
            name: "Porcelain",
            base: lch(0.965, 0.004, 85.0),
            text: LIGHT_TEXT,
            accent: lch(0.66, 0.17, 50.0).color(),
            tracks: [
                lch(0.63, 0.17, 27.0),
                lch(0.68, 0.15, 55.0),
                lch(0.75, 0.14, 88.0),
                lch(0.64, 0.14, 150.0),
                lch(0.64, 0.10, 205.0),
                lch(0.61, 0.15, 260.0),
                lch(0.61, 0.15, 300.0),
                lch(0.62, 0.17, 350.0),
            ],
            eq: Some([
                lch(0.62, 0.12, 60.0),
                lch(0.56, 0.12, 255.0),
                lch(0.60, 0.12, 150.0),
                lch(0.57, 0.15, 25.0),
            ]),
        }),
    ]
}

/// Look a built-in up by name.
pub fn builtin_by_name(name: &str) -> Option<ThemePalette> {
    builtins().into_iter().find(|t| t.name == name)
}

// ── User theme (.vzt) service ──────────────────────────────────────

/// A user theme scanned from the themes directory.
#[derive(Debug, Clone)]
pub struct UserTheme {
    /// Source file; kept for future delete/reveal actions.
    #[allow(dead_code)]
    pub path: PathBuf,
    pub palette: ThemePalette,
}

/// `~/.config/vibez/themes`, created on first use.
pub fn themes_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("vibez")
        .join("themes")
}

/// Scan the themes directory for `.vzt` files, plugin-cache style.
/// Unreadable or malformed files are skipped with a warning list so
/// one broken theme never hides the rest.
pub fn scan_user_themes() -> (Vec<UserTheme>, Vec<String>) {
    let mut themes = Vec::new();
    let mut warnings = Vec::new();
    let dir = themes_dir();
    let entries = match std::fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(_) => return (themes, warnings), // no dir yet: no themes
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("vzt") {
            continue;
        }
        match std::fs::read_to_string(&path)
            .map_err(|e| e.to_string())
            .and_then(|s| serde_json::from_str::<ThemePalette>(&s).map_err(|e| e.to_string()))
        {
            Ok(palette) => themes.push(UserTheme {
                path: path.clone(),
                palette,
            }),
            Err(e) => warnings.push(format!("{}: {e}", path.display())),
        }
    }
    themes.sort_by(|a, b| {
        a.palette
            .name
            .to_lowercase()
            .cmp(&b.palette.name.to_lowercase())
    });
    (themes, warnings)
}

/// Save a palette into the themes directory as `<slug>.vzt`.
pub fn save_user_theme(palette: &ThemePalette) -> Result<PathBuf, String> {
    let dir = themes_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let slug: String = palette
        .name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string();
    let slug = if slug.is_empty() {
        "custom".to_string()
    } else {
        slug
    };
    let path = dir.join(format!("{slug}.vzt"));
    let json = serde_json::to_string_pretty(palette).map_err(|e| e.to_string())?;
    std::fs::write(&path, json).map_err(|e| e.to_string())?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn twenty_builtins_with_unique_names() {
        let themes = builtins();
        assert_eq!(themes.len(), 20, "the collection ships 20 themes");
        let mut names: Vec<&str> = themes.iter().map(|t| t.name.as_str()).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), themes.len(), "names must be unique");
        assert_eq!(themes[0].name, "Charcoal", "default first");
    }

    /// Body text meets WCAG AAA.
    const TEXT_CONTRAST: f32 = 7.0;
    /// A fader handle is the control itself, so it keeps WCAG's 3:1 for
    /// graphics.
    const FADER_HANDLE_CONTRAST: f32 = 3.0;
    /// Floors for each rank of lane grid line; why they sit so low is on
    /// [`crate::theme::LaneStyle`].
    const GRID_LABEL_LINE_CONTRAST: f32 = 1.5;
    const GRID_BAR_LINE_CONTRAST: f32 = 1.2;
    const GRID_FINE_LINE_CONTRAST: f32 = 1.12;

    fn shortfalls(
        theme: &ThemePalette,
        role: &str,
        colour: Color,
        backgrounds: &[(&str, Color)],
        floor: f32,
    ) -> Vec<String> {
        backgrounds
            .iter()
            .map(|&(name, background)| (name, contrast_ratio(colour, background)))
            .filter(|&(_, ratio)| ratio < floor)
            .map(|(name, ratio)| format!("{}: {role} on {name} {ratio:.2} < {floor}", theme.name))
            .collect()
    }

    #[test]
    fn every_builtin_theme_keeps_its_text_tiers_readable_on_every_surface() {
        let failures: Vec<String> = builtins()
            .iter()
            .flat_map(|theme| {
                let surfaces = theme.text_surfaces();
                [
                    ("text", theme.text, TEXT_CONTRAST),
                    ("text_dim", theme.text_dim, TEXT_DIM_CONTRAST),
                    ("text_muted", theme.text_muted, TEXT_MUTED_CONTRAST),
                ]
                .into_iter()
                .flat_map(move |(role, colour, floor)| {
                    shortfalls(theme, role, colour, &surfaces, floor)
                })
            })
            .collect();
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    #[test]
    fn every_builtin_theme_draws_lane_grid_lines_that_read_against_the_lane() {
        let failures: Vec<String> = builtins()
            .iter()
            .flat_map(|theme| {
                let lane = [("lane", theme.lane())];
                let lines = theme.lane_style.lines;
                [
                    ("label line", lines.label, GRID_LABEL_LINE_CONTRAST),
                    ("bar line", lines.bar, GRID_BAR_LINE_CONTRAST),
                    ("fine line", lines.fine, GRID_FINE_LINE_CONTRAST),
                ]
                .into_iter()
                .flat_map(move |(role, strength, floor)| {
                    let colour = crate::theme::blend(theme.lane(), theme.text, strength);
                    shortfalls(theme, role, colour, &lane, floor)
                })
            })
            .collect();
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    #[test]
    fn every_builtin_theme_steps_its_knob_rings_clear_of_body_strip_and_arc() {
        let failures: Vec<String> = builtins()
            .iter()
            .flat_map(|theme| {
                let around = [
                    ("knob_body", theme.knob_body),
                    ("bg_surface", theme.bg_surface),
                    ("bg_elevated", theme.bg_elevated),
                ];
                let ring = [("knob_track", theme.knob_track)];
                let mut failures = shortfalls(
                    theme,
                    "knob_track",
                    theme.knob_track,
                    &around,
                    KNOB_RING_STEP,
                );
                for (role, arc) in [
                    ("accent", theme.accent),
                    ("eq_lf", theme.eq_lf),
                    ("eq_lmf", theme.eq_lmf),
                    ("eq_hmf", theme.eq_hmf),
                    ("eq_hf", theme.eq_hf),
                ] {
                    failures.extend(shortfalls(theme, role, arc, &ring, KNOB_RING_STEP));
                }
                failures
            })
            .collect();
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    #[test]
    fn every_builtin_theme_sets_its_fader_handles_clear_of_their_track_and_strip() {
        let failures: Vec<String> = builtins()
            .iter()
            .flat_map(|theme| {
                let behind = [
                    ("fader_track", theme.display_bg),
                    ("bg_elevated", theme.bg_elevated),
                    ("bg_surface", theme.bg_surface),
                ];
                shortfalls(
                    theme,
                    "fader_handle",
                    theme.fader_handle,
                    &behind,
                    FADER_HANDLE_CONTRAST,
                )
            })
            .collect();
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    #[test]
    fn the_default_dark_and_light_themes_have_vivid_track_palettes() {
        // Dev's classic rainbow averages 0.53. The muted 0.42 palette took
        // the pop out of the app; calmer looks are the Colour strength
        // preference's job, not the theme's.
        for name in ["Charcoal", "Porcelain"] {
            let theme = builtin_by_name(name).unwrap();
            let mean = theme
                .track_colors
                .iter()
                .map(|&color| crate::theme::Hsl::of(color).saturation)
                .sum::<f32>()
                / theme.track_colors.len() as f32;
            assert!(mean >= 0.5, "{name} mean track saturation is {mean}");
        }
    }

    /// The approved mockup steps dark surfaces about 0.035 OKLCH lightness
    /// apart. Light themes step 0.02 to 0.035, keeping their raised shade
    /// under white.
    const SURFACE_STEP: f32 = 0.018;
    /// How far a control fill stands off the raised cards it sits on.
    const CONTROL_STEP: f32 = 0.03;
    /// Below this chroma a surface is grey and its hue is noise.
    const TINTED: f32 = 0.01;

    fn hue_gap(a: f32, b: f32) -> f32 {
        ((a - b).rem_euclid(360.0)).min((b - a).rem_euclid(360.0))
    }

    #[test]
    fn every_builtin_theme_steps_its_surfaces_in_lightness_keeping_one_hue() {
        let mut failures = Vec::new();
        for theme in builtins() {
            let ramp = [
                ("display_bg", theme.display_bg),
                ("lane", theme.lane()),
                ("bg_surface", theme.bg_surface),
                ("bg_elevated", theme.bg_elevated),
            ]
            .map(|(role, colour)| (role, Oklch::of(colour)));
            for pair in ramp.windows(2) {
                let [(low, below), (high, above)] = [pair[0], pair[1]];
                if above.lightness - below.lightness < SURFACE_STEP {
                    failures.push(format!("{}: {high} not a step above {low}", theme.name));
                }
            }
            let raised = ramp[3].1;
            let control = Oklch::of(theme.control());
            if (control.lightness - raised.lightness).abs() < CONTROL_STEP {
                failures.push(format!("{}: control too close to bg_elevated", theme.name));
            }
            let lane = ramp[1].1;
            if lane.chroma < TINTED {
                continue;
            }
            for (role, surface) in ramp.into_iter().chain([("control", control)]) {
                if surface.chroma < lane.chroma * 0.6 || hue_gap(surface.hue, lane.hue) > 15.0 {
                    failures.push(format!("{}: {role} loses the lane's tint", theme.name));
                }
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    /// Tracks this close to their palette's median lightness read as one
    /// family. Yellow sits highest, as it must to look yellow at all.
    const TRACK_LIGHTNESS_SPREAD: f32 = 0.13;
    const TRACK_CHROMA: f32 = 0.095;

    #[test]
    fn every_builtin_theme_has_track_colours_of_one_lightness_and_strong_colour() {
        let mut failures = Vec::new();
        for theme in builtins() {
            let tracks = theme.track_colors.map(Oklch::of);
            let mut lightness = tracks.map(|track| track.lightness);
            lightness.sort_by(f32::total_cmp);
            let median = (lightness[3] + lightness[4]) / 2.0;
            for (index, track) in tracks.iter().enumerate() {
                if (track.lightness - median).abs() > TRACK_LIGHTNESS_SPREAD {
                    failures.push(format!(
                        "{}: track {index} {track:?} off {median}",
                        theme.name
                    ));
                }
                if track.chroma < TRACK_CHROMA {
                    failures.push(format!("{}: track {index} {track:?} too grey", theme.name));
                }
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    #[test]
    fn every_builtin_theme_draws_its_eq_bands_from_its_own_track_colours() {
        let mut failures = Vec::new();
        for theme in builtins() {
            let tracks = theme.track_colors.map(Oklch::of);
            for (band, colour) in [
                ("eq_lf", theme.eq_lf),
                ("eq_lmf", theme.eq_lmf),
                ("eq_hmf", theme.eq_hmf),
                ("eq_hf", theme.eq_hf),
            ] {
                let eq = Oklch::of(colour);
                let near_a_track = tracks
                    .iter()
                    .any(|track| hue_gap(track.hue, eq.hue) <= 15.0);
                if !near_a_track || eq.chroma < TRACK_CHROMA {
                    failures.push(format!("{}: {band} {eq:?}", theme.name));
                }
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    #[test]
    fn vzt_roundtrip_through_the_service() {
        let dir = tempfile::tempdir().unwrap();
        // Redirect the service to a temp dir via direct file ops:
        // save_user_theme always writes to the real config dir, so
        // exercise the same serialize/parse path manually here.
        let mut palette = charcoal();
        palette.name = "Test Custom".to_string();
        let json = serde_json::to_string_pretty(&palette).unwrap();
        let path = dir.path().join("test-custom.vzt");
        std::fs::write(&path, &json).unwrap();
        let back: ThemePalette =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(back.name, "Test Custom");
    }
}
