//! The built-in theme collection and the `.vzt` user-theme service.
//!
//! Built-ins are authored from a handful of seeds through the
//! dark/light builders below, so every theme fills the complete
//! [`ThemePalette`]. User themes are plain `.vzt` JSON files scanned
//! from the config themes directory, plugin-cache style.

use std::path::PathBuf;

use iced::Color;

use crate::theme::ThemePalette;

const fn rgb8(r: u8, g: u8, b: u8) -> Color {
    Color {
        r: r as f32 / 255.0,
        g: g as f32 / 255.0,
        b: b as f32 / 255.0,
        a: 1.0,
    }
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

/// Console-convention EQ band colors (LF brown, LMF blue, HMF green,
/// HF red), shared by most themes.
const EQ_CONSOLE: [Color; 4] = [
    Color {
        r: 0.65,
        g: 0.46,
        b: 0.28,
        a: 1.0,
    },
    Color {
        r: 0.36,
        g: 0.48,
        b: 0.72,
        a: 1.0,
    },
    Color {
        r: 0.33,
        g: 0.62,
        b: 0.38,
        a: 1.0,
    },
    Color {
        r: 0.76,
        g: 0.33,
        b: 0.30,
        a: 1.0,
    },
];

/// The default track rainbow (charcoal's).
const TRACKS_CLASSIC: [Color; 8] = [
    rgb8(224, 96, 96),
    rgb8(224, 144, 64),
    rgb8(208, 192, 64),
    rgb8(80, 176, 96),
    rgb8(64, 160, 176),
    rgb8(96, 128, 208),
    rgb8(144, 96, 192),
    rgb8(192, 96, 160),
];

const TRACKS_NEON: [Color; 8] = [
    rgb8(255, 64, 96),
    rgb8(255, 144, 0),
    rgb8(230, 255, 0),
    rgb8(57, 255, 20),
    rgb8(0, 255, 213),
    rgb8(0, 160, 255),
    rgb8(191, 64, 255),
    rgb8(255, 64, 208),
];

const TRACKS_PASTEL: [Color; 8] = [
    rgb8(243, 139, 168),
    rgb8(250, 179, 135),
    rgb8(249, 226, 175),
    rgb8(166, 227, 161),
    rgb8(148, 226, 213),
    rgb8(137, 180, 250),
    rgb8(203, 166, 247),
    rgb8(245, 194, 231),
];

const TRACKS_WARM: [Color; 8] = [
    rgb8(204, 102, 84),
    rgb8(214, 143, 84),
    rgb8(199, 178, 90),
    rgb8(143, 158, 96),
    rgb8(114, 150, 137),
    rgb8(122, 138, 176),
    rgb8(158, 118, 160),
    rgb8(186, 118, 132),
];

/// Desaturated club palette: distinct hues, no neon. Tracks stay
/// tellable apart without breaking a monochrome-leaning theme.
const TRACKS_MUTED: [Color; 8] = [
    rgb8(178, 96, 96),
    rgb8(178, 130, 88),
    rgb8(168, 154, 96),
    rgb8(106, 146, 110),
    rgb8(96, 138, 146),
    rgb8(106, 122, 162),
    rgb8(138, 108, 154),
    rgb8(162, 106, 130),
];

struct Seed {
    name: &'static str,
    bg: Color,
    accent: Color,
    text: Color,
    tracks: [Color; 8],
    eq: [Color; 4],
}

/// Fill a complete palette from a dark seed: surfaces step up from
/// the background, text tiers step down, grids sit between.
fn dark(seed: Seed) -> ThemePalette {
    let bg = seed.bg;
    ThemePalette {
        name: seed.name.to_string(),
        bg_dark: bg,
        bg_surface: lighten(bg, 0.045),
        bg_elevated: lighten(bg, 0.085),
        bg_hover: lighten(bg, 0.125),
        text: seed.text,
        text_dim: darken(seed.text, 0.43),
        text_muted: darken(seed.text, 0.64),
        accent: seed.accent,
        accent_dim: darken(seed.accent, 0.4),
        border: lighten(bg, 0.14),
        border_light: lighten(bg, 0.21),
        divider: lighten(bg, 0.07),
        success: rgb8(74, 222, 128),
        danger: rgb8(248, 113, 113),
        playhead: alpha(Color::WHITE, 0.8),
        meter_green: rgb8(74, 222, 128),
        meter_yellow: rgb8(255, 215, 0),
        meter_red: rgb8(248, 113, 113),
        display_bg: darken(bg, 0.32),
        knob_arc: lighten(bg, 0.48),
        knob_body: lighten(bg, 0.02),
        knob_body_engaged: lighten(bg, 0.065),
        knob_track: lighten(bg, 0.13),
        fader_handle: lighten(bg, 0.48),
        track_colors: seed.tracks,
        eq_lf: seed.eq[0],
        eq_lmf: seed.eq[1],
        eq_hmf: seed.eq[2],
        eq_hf: seed.eq[3],
        clip_body: alpha(seed.accent, 0.35),
        clip_border: alpha(lighten(seed.accent, 0.2), 0.9),
        waveform: alpha(lighten(seed.accent, 0.2), 0.6),
        ruler_line: alpha(lighten(bg, 0.14), 0.5),
        grid_bar: lighten(bg, 0.30),
        grid_beat: lighten(bg, 0.17),
        grid_sub: lighten(bg, 0.13),
        piano_white_row: lighten(bg, 0.05),
        piano_black_row: lighten(bg, 0.01),
        piano_octave_line: lighten(bg, 0.14),
        piano_grid: lighten(bg, 0.07),
        piano_white_key: rgb8(200, 200, 200),
        piano_black_key: darken(bg, 0.15),
        piano_key_label: lighten(bg, 0.32),
    }
}

/// Fill a complete palette from a light seed: surfaces step down
/// from the background, text is ink, grids darker than paper.
fn light(seed: Seed) -> ThemePalette {
    let bg = seed.bg;
    ThemePalette {
        name: seed.name.to_string(),
        bg_dark: bg,
        bg_surface: darken(bg, 0.035),
        bg_elevated: darken(bg, 0.065),
        bg_hover: darken(bg, 0.10),
        text: seed.text,
        text_dim: lighten(seed.text, 0.38),
        text_muted: lighten(seed.text, 0.58),
        accent: seed.accent,
        accent_dim: lighten(seed.accent, 0.3),
        border: darken(bg, 0.14),
        border_light: darken(bg, 0.22),
        divider: darken(bg, 0.07),
        success: rgb8(34, 154, 82),
        danger: rgb8(205, 60, 60),
        playhead: alpha(Color::BLACK, 0.7),
        meter_green: rgb8(46, 178, 99),
        meter_yellow: rgb8(212, 165, 8),
        meter_red: rgb8(205, 60, 60),
        display_bg: darken(bg, 0.05),
        knob_arc: darken(bg, 0.45),
        knob_body: darken(bg, 0.05),
        knob_body_engaged: darken(bg, 0.10),
        knob_track: darken(bg, 0.18),
        fader_handle: darken(bg, 0.45),
        track_colors: seed.tracks,
        eq_lf: darken(seed.eq[0], 0.12),
        eq_lmf: darken(seed.eq[1], 0.12),
        eq_hmf: darken(seed.eq[2], 0.12),
        eq_hf: darken(seed.eq[3], 0.12),
        clip_body: alpha(seed.accent, 0.35),
        clip_border: alpha(darken(seed.accent, 0.2), 0.9),
        waveform: alpha(darken(seed.accent, 0.25), 0.65),
        ruler_line: alpha(darken(bg, 0.14), 0.5),
        grid_bar: darken(bg, 0.32),
        grid_beat: darken(bg, 0.18),
        grid_sub: darken(bg, 0.12),
        piano_white_row: darken(bg, 0.02),
        piano_black_row: darken(bg, 0.075),
        piano_octave_line: darken(bg, 0.20),
        piano_grid: darken(bg, 0.10),
        piano_white_key: rgb8(250, 250, 250),
        piano_black_key: rgb8(40, 40, 40),
        piano_key_label: darken(bg, 0.45),
    }
}

/// All built-in themes, default first.
pub fn builtins() -> Vec<ThemePalette> {
    let mut themes = vec![ThemePalette::charcoal()];
    themes.extend([
        dark(Seed {
            name: "Obsidian",
            bg: rgb8(10, 10, 12),
            accent: rgb8(125, 200, 255),
            text: rgb8(222, 226, 230),
            tracks: TRACKS_CLASSIC,
            eq: EQ_CONSOLE,
        }),
        dark(Seed {
            name: "Berlin",
            bg: rgb8(6, 6, 6),
            accent: rgb8(192, 57, 43),
            text: rgb8(200, 200, 200),
            tracks: TRACKS_MUTED,
            eq: EQ_CONSOLE,
        }),
        dark(Seed {
            name: "Acid",
            bg: rgb8(8, 10, 6),
            accent: rgb8(170, 255, 0),
            text: rgb8(214, 230, 202),
            tracks: TRACKS_NEON,
            eq: EQ_CONSOLE,
        }),
        dark(Seed {
            name: "Vaporwave",
            bg: rgb8(24, 14, 42),
            accent: rgb8(255, 113, 206),
            text: rgb8(230, 220, 245),
            tracks: TRACKS_NEON,
            eq: [
                rgb8(255, 113, 206),
                rgb8(1, 205, 254),
                rgb8(5, 255, 161),
                rgb8(185, 103, 255),
            ],
        }),
        dark(Seed {
            name: "Nord",
            bg: rgb8(46, 52, 64),
            accent: rgb8(136, 192, 208),
            text: rgb8(216, 222, 233),
            tracks: [
                rgb8(191, 97, 106),
                rgb8(208, 135, 112),
                rgb8(235, 203, 139),
                rgb8(163, 190, 140),
                rgb8(143, 188, 187),
                rgb8(129, 161, 193),
                rgb8(180, 142, 173),
                rgb8(94, 129, 172),
            ],
            eq: EQ_CONSOLE,
        }),
        dark(Seed {
            name: "Dracula",
            bg: rgb8(40, 42, 54),
            accent: rgb8(189, 147, 249),
            text: rgb8(248, 248, 242),
            tracks: [
                rgb8(255, 85, 85),
                rgb8(255, 184, 108),
                rgb8(241, 250, 140),
                rgb8(80, 250, 123),
                rgb8(139, 233, 253),
                rgb8(98, 114, 164),
                rgb8(189, 147, 249),
                rgb8(255, 121, 198),
            ],
            eq: EQ_CONSOLE,
        }),
        dark(Seed {
            name: "Gruvbox",
            bg: rgb8(40, 40, 40),
            accent: rgb8(254, 128, 25),
            text: rgb8(235, 219, 178),
            tracks: [
                rgb8(204, 36, 29),
                rgb8(214, 93, 14),
                rgb8(215, 153, 33),
                rgb8(152, 151, 26),
                rgb8(104, 157, 106),
                rgb8(69, 133, 136),
                rgb8(177, 98, 134),
                rgb8(254, 128, 25),
            ],
            eq: EQ_CONSOLE,
        }),
        dark(Seed {
            name: "Solarized Dark",
            bg: rgb8(0, 43, 54),
            accent: rgb8(38, 139, 210),
            text: rgb8(147, 161, 161),
            tracks: [
                rgb8(220, 50, 47),
                rgb8(203, 75, 22),
                rgb8(181, 137, 0),
                rgb8(133, 153, 0),
                rgb8(42, 161, 152),
                rgb8(38, 139, 210),
                rgb8(108, 113, 196),
                rgb8(211, 54, 130),
            ],
            eq: EQ_CONSOLE,
        }),
        dark(Seed {
            name: "Monokai",
            bg: rgb8(39, 40, 34),
            accent: rgb8(166, 226, 46),
            text: rgb8(248, 248, 242),
            tracks: [
                rgb8(249, 38, 114),
                rgb8(253, 151, 31),
                rgb8(230, 219, 116),
                rgb8(166, 226, 46),
                rgb8(102, 217, 239),
                rgb8(118, 149, 216),
                rgb8(174, 129, 255),
                rgb8(249, 38, 114),
            ],
            eq: EQ_CONSOLE,
        }),
        dark(Seed {
            name: "Tokyo Night",
            bg: rgb8(26, 27, 38),
            accent: rgb8(122, 162, 247),
            text: rgb8(192, 202, 245),
            tracks: [
                rgb8(247, 118, 142),
                rgb8(255, 158, 100),
                rgb8(224, 175, 104),
                rgb8(158, 206, 106),
                rgb8(115, 218, 202),
                rgb8(122, 162, 247),
                rgb8(187, 154, 247),
                rgb8(255, 117, 127),
            ],
            eq: EQ_CONSOLE,
        }),
        dark(Seed {
            name: "Catppuccin",
            bg: rgb8(30, 30, 46),
            accent: rgb8(203, 166, 247),
            text: rgb8(205, 214, 244),
            tracks: TRACKS_PASTEL,
            eq: EQ_CONSOLE,
        }),
        dark(Seed {
            name: "One Dark",
            bg: rgb8(40, 44, 52),
            accent: rgb8(97, 175, 239),
            text: rgb8(171, 178, 191),
            tracks: [
                rgb8(224, 108, 117),
                rgb8(209, 154, 102),
                rgb8(229, 192, 123),
                rgb8(152, 195, 121),
                rgb8(86, 182, 194),
                rgb8(97, 175, 239),
                rgb8(198, 120, 221),
                rgb8(190, 80, 70),
            ],
            eq: EQ_CONSOLE,
        }),
        dark(Seed {
            name: "Deep Sea",
            bg: rgb8(11, 26, 36),
            accent: rgb8(46, 196, 182),
            text: rgb8(202, 220, 228),
            tracks: [
                rgb8(231, 111, 81),
                rgb8(244, 162, 97),
                rgb8(233, 196, 106),
                rgb8(138, 177, 125),
                rgb8(42, 157, 143),
                rgb8(69, 123, 157),
                rgb8(131, 111, 168),
                rgb8(190, 111, 130),
            ],
            eq: EQ_CONSOLE,
        }),
        dark(Seed {
            name: "Espresso",
            bg: rgb8(36, 26, 20),
            accent: rgb8(212, 163, 115),
            text: rgb8(230, 218, 205),
            tracks: TRACKS_WARM,
            eq: EQ_CONSOLE,
        }),
        dark(Seed {
            name: "Slate",
            bg: rgb8(28, 33, 40),
            accent: rgb8(83, 155, 245),
            text: rgb8(205, 217, 229),
            tracks: TRACKS_CLASSIC,
            eq: EQ_CONSOLE,
        }),
        dark(Seed {
            name: "Rose Pine",
            bg: rgb8(25, 23, 36),
            accent: rgb8(235, 188, 186),
            text: rgb8(224, 222, 244),
            tracks: [
                rgb8(235, 111, 146),
                rgb8(246, 193, 119),
                rgb8(234, 154, 151),
                rgb8(49, 116, 143),
                rgb8(156, 207, 216),
                rgb8(196, 167, 231),
                rgb8(144, 122, 169),
                rgb8(235, 188, 186),
            ],
            eq: EQ_CONSOLE,
        }),
        dark(Seed {
            name: "High Contrast",
            bg: rgb8(0, 0, 0),
            accent: rgb8(255, 215, 0),
            text: rgb8(255, 255, 255),
            tracks: TRACKS_NEON,
            eq: EQ_CONSOLE,
        }),
        light(Seed {
            name: "Solarized Light",
            bg: rgb8(253, 246, 227),
            accent: rgb8(38, 139, 210),
            text: rgb8(88, 110, 117),
            tracks: [
                rgb8(220, 50, 47),
                rgb8(203, 75, 22),
                rgb8(181, 137, 0),
                rgb8(133, 153, 0),
                rgb8(42, 161, 152),
                rgb8(38, 139, 210),
                rgb8(108, 113, 196),
                rgb8(211, 54, 130),
            ],
            eq: EQ_CONSOLE,
        }),
        light(Seed {
            name: "Porcelain",
            bg: rgb8(242, 242, 240),
            accent: rgb8(235, 120, 15),
            text: rgb8(40, 42, 46),
            tracks: TRACKS_CLASSIC,
            eq: EQ_CONSOLE,
        }),
    ]);
    themes
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

    #[test]
    fn builtin_text_contrasts_with_background() {
        // Cheap luminance gate: every theme's primary text must sit
        // far from its background, dark or light.
        fn luma(c: Color) -> f32 {
            0.2126 * c.r + 0.7152 * c.g + 0.0722 * c.b
        }
        for t in builtins() {
            let d = (luma(t.text) - luma(t.bg_dark)).abs();
            assert!(d > 0.45, "theme {} text/bg contrast too low: {d}", t.name);
        }
    }

    #[test]
    fn vzt_roundtrip_through_the_service() {
        let dir = tempfile::tempdir().unwrap();
        // Redirect the service to a temp dir via direct file ops:
        // save_user_theme always writes to the real config dir, so
        // exercise the same serialize/parse path manually here.
        let mut palette = ThemePalette::charcoal();
        palette.name = "Test Custom".to_string();
        let json = serde_json::to_string_pretty(&palette).unwrap();
        let path = dir.path().join("test-custom.vzt");
        std::fs::write(&path, &json).unwrap();
        let back: ThemePalette =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(back.name, "Test Custom");
    }
}
