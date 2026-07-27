//! Runtime theme system.
//!
//! Every color the UI paints comes from the current [`ThemePalette`],
//! swapped at runtime by the Appearance settings. Accessors mirror
//! the old constant names in snake_case (`th::accent()` was
//! `th::ACCENT`), so call sites read the same and canvases pick up a
//! theme switch on the next frame. A palette serializes to JSON with
//! hex colors — that is exactly the `.vzt` theme file format.

use std::sync::{LazyLock, RwLock};

use iced::theme::Palette;
use iced::{Color, Theme};
use serde::{Deserialize, Serialize};

/// A complete vibez color scheme. Serialized as the `.vzt` file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThemePalette {
    pub name: String,

    // ── Backgrounds ──
    #[serde(with = "hex")]
    pub bg_dark: Color,
    #[serde(with = "hex")]
    pub bg_surface: Color,
    #[serde(with = "hex")]
    pub bg_elevated: Color,
    #[serde(with = "hex")]
    pub bg_hover: Color,

    // ── Text ──
    #[serde(with = "hex")]
    pub text: Color,
    #[serde(with = "hex")]
    pub text_dim: Color,
    #[serde(with = "hex")]
    pub text_muted: Color,

    // ── Accent ──
    #[serde(with = "hex")]
    pub accent: Color,
    #[serde(with = "hex")]
    pub accent_dim: Color,

    // ── Borders ──
    #[serde(with = "hex")]
    pub border: Color,
    #[serde(with = "hex")]
    pub border_light: Color,
    #[serde(with = "hex")]
    pub divider: Color,

    // ── Semantic ──
    #[serde(with = "hex")]
    pub success: Color,
    #[serde(with = "hex")]
    pub danger: Color,
    #[serde(with = "hex")]
    pub playhead: Color,

    // ── Meters ──
    #[serde(with = "hex")]
    pub meter_green: Color,
    #[serde(with = "hex")]
    pub meter_yellow: Color,
    #[serde(with = "hex")]
    pub meter_red: Color,

    /// Recessed display wells (mini waveforms, sample views).
    #[serde(with = "hex")]
    pub display_bg: Color,

    // ── Knobs / faders ──
    #[serde(with = "hex")]
    pub knob_arc: Color,
    #[serde(with = "hex")]
    pub knob_body: Color,
    #[serde(with = "hex")]
    pub knob_body_engaged: Color,
    #[serde(with = "hex")]
    pub knob_track: Color,
    #[serde(with = "hex")]
    pub fader_handle: Color,

    // ── Track palette (auto-assigned per track, index wraps) ──
    #[serde(with = "hex_array")]
    pub track_colors: [Color; 8],

    // ── Channel EQ bands ──
    #[serde(with = "hex")]
    pub eq_lf: Color,
    #[serde(with = "hex")]
    pub eq_lmf: Color,
    #[serde(with = "hex")]
    pub eq_hmf: Color,
    #[serde(with = "hex")]
    pub eq_hf: Color,

    // ── Clips / waveforms ──
    #[serde(with = "hex")]
    pub clip_body: Color,
    #[serde(with = "hex")]
    pub clip_border: Color,
    #[serde(with = "hex")]
    pub waveform: Color,
    #[serde(with = "hex")]
    pub ruler_line: Color,

    // ── Editor grid lines (bar / beat / subdivision) ──
    #[serde(with = "hex")]
    pub grid_bar: Color,
    #[serde(with = "hex")]
    pub grid_beat: Color,
    #[serde(with = "hex")]
    pub grid_sub: Color,

    // ── Piano roll ──
    #[serde(with = "hex")]
    pub piano_white_row: Color,
    #[serde(with = "hex")]
    pub piano_black_row: Color,
    #[serde(with = "hex")]
    pub piano_octave_line: Color,
    #[serde(with = "hex")]
    pub piano_grid: Color,
    #[serde(with = "hex")]
    pub piano_white_key: Color,
    #[serde(with = "hex")]
    pub piano_black_key: Color,
    #[serde(with = "hex")]
    pub piano_key_label: Color,
}

/// Hex color (de)serialization: `#rrggbb` or `#rrggbbaa`.
mod hex {
    use iced::Color;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn to_hex(c: &Color) -> String {
        let [r, g, b, a] = c.into_rgba8();
        if a == 255 {
            format!("#{r:02x}{g:02x}{b:02x}")
        } else {
            format!("#{r:02x}{g:02x}{b:02x}{a:02x}")
        }
    }

    pub fn parse(s: &str) -> Option<Color> {
        let s = s.strip_prefix('#')?;
        let byte = |i: usize| u8::from_str_radix(s.get(i..i + 2)?, 16).ok();
        match s.len() {
            6 => Some(Color::from_rgb8(byte(0)?, byte(2)?, byte(4)?)),
            8 => Some(Color::from_rgba8(
                byte(0)?,
                byte(2)?,
                byte(4)?,
                byte(6)? as f32 / 255.0,
            )),
            _ => None,
        }
    }

    pub fn serialize<S: Serializer>(c: &Color, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&to_hex(c))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Color, D::Error> {
        let s = String::deserialize(d)?;
        parse(&s).ok_or_else(|| serde::de::Error::custom(format!("invalid hex color {s:?}")))
    }
}

mod hex_array {
    use iced::Color;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(colors: &[Color; 8], s: S) -> Result<S::Ok, S::Error> {
        s.collect_seq(colors.iter().map(super::hex::to_hex))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<[Color; 8], D::Error> {
        let raw = Vec::<String>::deserialize(d)?;
        if raw.len() != 8 {
            return Err(serde::de::Error::custom(format!(
                "track_colors needs exactly 8 entries, got {}",
                raw.len()
            )));
        }
        let mut out = [Color::BLACK; 8];
        for (slot, s) in out.iter_mut().zip(&raw) {
            *slot = super::hex::parse(s)
                .ok_or_else(|| serde::de::Error::custom(format!("invalid hex color {s:?}")))?;
        }
        Ok(out)
    }
}

const fn rgb(r: f32, g: f32, b: f32) -> Color {
    Color { r, g, b, a: 1.0 }
}

const fn rgba(r: f32, g: f32, b: f32, a: f32) -> Color {
    Color { r, g, b, a }
}

impl ThemePalette {
    /// The founding vibez look: dark charcoal, orange accent.
    pub fn charcoal() -> Self {
        Self {
            name: "Charcoal".to_string(),
            bg_dark: rgb(0.102, 0.102, 0.102),
            bg_surface: rgb(0.141, 0.141, 0.141),
            bg_elevated: rgb(0.176, 0.176, 0.176),
            bg_hover: rgb(0.212, 0.212, 0.212),
            text: rgb(0.878, 0.878, 0.878),
            text_dim: rgb(0.502, 0.502, 0.502),
            text_muted: rgb(0.314, 0.314, 0.314),
            accent: rgb(1.0, 0.549, 0.0),
            accent_dim: rgb(0.600, 0.329, 0.0),
            border: rgb(0.227, 0.227, 0.227),
            border_light: rgb(0.290, 0.290, 0.290),
            divider: rgb(0.165, 0.165, 0.165),
            success: rgb(0.290, 0.871, 0.502),
            danger: rgb(0.973, 0.443, 0.443),
            playhead: rgba(1.0, 1.0, 1.0, 0.8),
            meter_green: rgb(0.290, 0.871, 0.502),
            meter_yellow: rgb(1.0, 0.843, 0.0),
            meter_red: rgb(0.973, 0.443, 0.443),
            display_bg: rgb(0.07, 0.07, 0.07),
            knob_arc: rgb(0.533, 0.533, 0.533),
            knob_body: rgb(0.12, 0.12, 0.12),
            knob_body_engaged: rgb(0.16, 0.16, 0.16),
            knob_track: rgb(0.22, 0.22, 0.22),
            fader_handle: rgb(0.533, 0.533, 0.533),
            track_colors: [
                rgb(0.878, 0.376, 0.376), // red
                rgb(0.878, 0.565, 0.251), // orange
                rgb(0.816, 0.753, 0.251), // yellow
                rgb(0.314, 0.690, 0.376), // green
                rgb(0.251, 0.627, 0.690), // cyan
                rgb(0.376, 0.502, 0.816), // blue
                rgb(0.565, 0.376, 0.753), // purple
                rgb(0.753, 0.376, 0.627), // pink
            ],
            eq_lf: rgb(0.65, 0.46, 0.28),
            eq_lmf: rgb(0.36, 0.48, 0.72),
            eq_hmf: rgb(0.33, 0.62, 0.38),
            eq_hf: rgb(0.76, 0.33, 0.30),
            clip_body: rgba(0.300, 0.480, 0.900, 0.6),
            clip_border: rgba(0.380, 0.565, 1.0, 0.9),
            waveform: rgba(0.380, 0.565, 1.0, 0.6),
            ruler_line: rgba(0.227, 0.227, 0.227, 0.5),
            grid_bar: rgb(0.376, 0.376, 0.376),
            grid_beat: rgb(0.251, 0.251, 0.251),
            grid_sub: rgb(0.216, 0.216, 0.216),
            piano_white_row: rgb(0.145, 0.145, 0.145),
            piano_black_row: rgb(0.110, 0.110, 0.110),
            piano_octave_line: rgb(0.227, 0.227, 0.227),
            piano_grid: rgb(0.165, 0.165, 0.165),
            piano_white_key: rgb(0.784, 0.784, 0.784),
            piano_black_key: rgb(0.102, 0.102, 0.102),
            piano_key_label: rgb(0.35, 0.35, 0.35),
        }
    }
}

impl Default for ThemePalette {
    fn default() -> Self {
        Self::charcoal()
    }
}

static CURRENT: LazyLock<RwLock<ThemePalette>> =
    LazyLock::new(|| RwLock::new(ThemePalette::charcoal()));

static EPOCH: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Swap the active theme; the next frame repaints everything.
#[allow(dead_code)] // wired up by the Appearance settings
pub fn set_theme(palette: ThemePalette) {
    *CURRENT.write().unwrap() = palette;
    EPOCH.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
}

/// Monotonic palette generation. Canvases that cache geometry fold
/// this into their fingerprints so a theme swap invalidates colors
/// baked into the cache.
pub fn epoch() -> u64 {
    EPOCH.load(std::sync::atomic::Ordering::Relaxed)
}

/// Snapshot of the whole current palette (theme save, swatches).
#[allow(dead_code)] // wired up by the Appearance settings
pub fn current() -> ThemePalette {
    CURRENT.read().unwrap().clone()
}

macro_rules! accessors {
    ($($fn_name:ident => $field:ident),* $(,)?) => {
        $(
            #[allow(dead_code)]
            pub fn $fn_name() -> Color {
                CURRENT.read().unwrap().$field
            }
        )*
    };
}

accessors! {
    bg_dark => bg_dark,
    bg_surface => bg_surface,
    bg_elevated => bg_elevated,
    bg_hover => bg_hover,
    text => text,
    text_dim => text_dim,
    text_muted => text_muted,
    accent => accent,
    accent_dim => accent_dim,
    border => border,
    border_light => border_light,
    divider => divider,
    success => success,
    danger => danger,
    playhead => playhead,
    meter_green => meter_green,
    meter_yellow => meter_yellow,
    meter_red => meter_red,
    display_bg => display_bg,
    knob_arc => knob_arc,
    knob_body => knob_body,
    knob_body_engaged => knob_body_engaged,
    knob_track => knob_track,
    fader_handle => fader_handle,
    eq_lf => eq_lf,
    eq_lmf => eq_lmf,
    eq_hmf => eq_hmf,
    eq_hf => eq_hf,
    clip_body => clip_body,
    clip_border => clip_border,
    waveform => waveform,
    ruler_line => ruler_line,
    grid_bar => grid_bar,
    grid_beat => grid_beat,
    grid_sub => grid_sub,
    piano_white_row => piano_white_row,
    piano_black_row => piano_black_row,
    piano_octave_line => piano_octave_line,
    piano_grid => piano_grid,
    piano_white_key => piano_white_key,
    piano_black_key => piano_black_key,
    piano_key_label => piano_key_label,
}

// ── Derived roles (aliases of palette fields) ──

pub fn mute_active() -> Color {
    danger()
}

pub fn solo_active() -> Color {
    meter_yellow()
}

pub fn knob_bg() -> Color {
    bg_elevated()
}

pub fn fader_track() -> Color {
    bg_elevated()
}

pub fn track_bg() -> Color {
    bg_dark()
}

pub fn track_bg_selected() -> Color {
    bg_elevated()
}

pub fn ruler_bg() -> Color {
    bg_surface()
}

pub fn ruler_text() -> Color {
    text_dim()
}

/// Mix two theme colors without escaping the active palette. Perform uses
/// this for instrument-like depth that still follows light, dark, and custom
/// `.vzt` themes.
pub fn blend(from: Color, to: Color, amount: f32) -> Color {
    let amount = amount.clamp(0.0, 1.0);
    let keep = 1.0 - amount;
    Color {
        r: from.r * keep + to.r * amount,
        g: from.g * keep + to.g * amount,
        b: from.b * keep + to.b * amount,
        a: from.a * keep + to.a * amount,
    }
}

/// Raised face and recessed edge roles for the Perform workspace. These are
/// derived roles rather than serialized palette fields, so every existing and
/// user-authored theme remains compatible.
pub fn perform_active_surface() -> Color {
    blend(bg_surface(), accent(), 0.1)
}

pub fn perform_pad_highlight() -> Color {
    blend(bg_elevated(), text(), 0.045)
}

pub fn perform_pad_lowlight() -> Color {
    blend(bg_elevated(), bg_dark(), 0.42)
}

pub fn perform_inset() -> Color {
    blend(bg_dark(), border(), 0.16)
}

pub fn perform_shadow() -> Color {
    with_alpha(darken(bg_dark(), 0.42), 0.72)
}

pub fn perform_grid_line() -> Color {
    with_alpha(grid_beat(), 0.52)
}

/// Get a track color by index (wraps around).
pub fn track_color(index: u8) -> Color {
    let palette = CURRENT.read().unwrap();
    palette.track_colors[index as usize % palette.track_colors.len()]
}

/// Darken a color by a factor (for borders, etc.)
pub fn darken(color: Color, factor: f32) -> Color {
    Color {
        r: color.r * factor,
        g: color.g * factor,
        b: color.b * factor,
        a: color.a,
    }
}

/// Apply alpha to a color.
#[allow(dead_code)]
pub fn with_alpha(color: Color, alpha: f32) -> Color {
    Color { a: alpha, ..color }
}

/// iced theme derived from the current palette (built-in widget
/// chrome: scrollbars, text inputs, pick lists).
pub fn vibez_theme() -> Theme {
    let p = CURRENT.read().unwrap();
    Theme::custom(
        p.name.clone(),
        Palette {
            background: p.bg_dark,
            text: p.text,
            primary: p.accent,
            success: p.success,
            danger: p.danger,
        },
    )
}

/// Uniform device-card body height across the device chain: the
/// panel scrolls horizontally, so cards share one rack height.
pub const DEVICE_BODY_H: f32 = 184.0;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_roundtrip() {
        for c in [
            rgb(0.102, 0.102, 0.102),
            rgb(1.0, 0.549, 0.0),
            rgba(1.0, 1.0, 1.0, 0.8),
        ] {
            let s = hex::to_hex(&c);
            let back = hex::parse(&s).unwrap();
            let [r1, g1, b1, a1] = c.into_rgba8();
            let [r2, g2, b2, a2] = back.into_rgba8();
            assert_eq!((r1, g1, b1, a1), (r2, g2, b2, a2), "{s}");
        }
        assert!(hex::parse("#12345").is_none());
        assert!(hex::parse("nope").is_none());
    }

    #[test]
    fn palette_serializes_as_vzt_json_and_loads_back() {
        let p = ThemePalette::charcoal();
        let json = serde_json::to_string_pretty(&p).unwrap();
        assert!(json.contains("\"accent\": \"#ff8c00\""));
        let back: ThemePalette = serde_json::from_str(&json).unwrap();
        assert_eq!(back.name, "Charcoal");
        assert_eq!(hex::to_hex(&back.accent), "#ff8c00");
        assert_eq!(back.track_colors.len(), 8);
    }

    #[test]
    fn missing_field_is_a_load_error_not_a_panic() {
        let result = serde_json::from_str::<ThemePalette>("{\"name\": \"Broken\"}");
        assert!(result.is_err());
    }
}
