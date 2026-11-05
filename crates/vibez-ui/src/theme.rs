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
    /// Button fills. Older `.vzt` files predate it and fill their controls
    /// with `bg_elevated`, see [`ThemePalette::control`].
    #[serde(default, with = "hex_option", skip_serializing_if = "Option::is_none")]
    pub control: Option<Color>,

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

    /// Older `.vzt` files predate it and load with the dark style.
    #[serde(default)]
    pub clip_style: ClipStyle,

    /// Older `.vzt` files predate it and load with the dark style.
    #[serde(default)]
    pub lane_style: LaneStyle,
}

/// How a timeline clip is shaded from its track colour.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ClipStyle {
    /// Opacity of the clip body, so the lane shows through faintly while
    /// the title strip, notes and waveform stay solid.
    pub body_opacity: f32,
    /// How far the body is mixed from the track colour toward the lane
    /// background before `body_opacity` applies.
    pub body_toward_lane: f32,
    /// Notes, waveform and title text, drawn over the track colour.
    #[serde(with = "hex")]
    pub ink: Color,
}

impl ClipStyle {
    pub const DARK: Self = Self {
        body_opacity: 0.55,
        // Tuned by eye against the mockup's dark body shade.
        body_toward_lane: 0.28,
        ink: Color {
            r: 10.0 / 255.0,
            g: 10.0 / 255.0,
            b: 12.0 / 255.0,
            a: 0.86,
        },
    };

    pub const LIGHT: Self = Self {
        body_opacity: 0.55,
        body_toward_lane: 0.18,
        ink: Color {
            r: 16.0 / 255.0,
            g: 16.0 / 255.0,
            b: 18.0 / 255.0,
            a: 0.86,
        },
    };
}

impl Default for ClipStyle {
    fn default() -> Self {
        Self::DARK
    }
}

/// How the timeline lanes behind clips are shaded from the palette. Every
/// part is a mix toward the text colour, so the dark style still reads on
/// a light theme file that predates it.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LaneStyle {
    /// How far the lane is mixed from the background toward the text
    /// colour. Built-in themes author the lane as the background itself;
    /// older light theme files sink their lanes below it with this.
    pub recess: f32,
    /// How far every other bar band is mixed toward the text colour.
    pub band: f32,
    /// How far grid lines are mixed toward the text colour. Kept faint
    /// because clip bodies are see-through and the grid must read as
    /// texture behind them.
    pub lines: GridLineStrengths,
    /// How far the lanes, Section ruler and overview under a loop are mixed
    /// toward the accent.
    pub loop_tint: f32,
}

/// How strongly each rank of lane grid line is mixed toward the text colour.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct GridLineStrengths {
    /// Lines on the ruler's label step.
    pub label: f32,
    pub bar: f32,
    /// Lines on steps finer than a bar.
    pub fine: f32,
}

impl LaneStyle {
    pub const DARK: Self = Self {
        recess: 0.0,
        band: 0.025,
        lines: GridLineStrengths {
            label: 0.085,
            bar: 0.05,
            fine: 0.028,
        },
        loop_tint: 0.05,
    };

    pub const LIGHT: Self = Self {
        recess: 0.0,
        band: 0.05,
        lines: GridLineStrengths {
            label: 0.13,
            bar: 0.075,
            fine: 0.045,
        },
        loop_tint: 0.07,
    };
}

impl Default for LaneStyle {
    fn default() -> Self {
        Self::DARK
    }
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

mod hex_option {
    use iced::Color;
    use serde::{Deserializer, Serializer};

    pub fn serialize<S: Serializer>(c: &Option<Color>, s: S) -> Result<S::Ok, S::Error> {
        match c {
            Some(c) => super::hex::serialize(c, s),
            None => s.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Color>, D::Error> {
        super::hex::deserialize(d).map(Some)
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

impl ThemePalette {
    /// The fill of buttons and other controls.
    pub fn control(&self) -> Color {
        self.control.unwrap_or(self.bg_elevated)
    }

    /// Background of the timeline lanes behind clips.
    pub fn lane(&self) -> Color {
        blend(self.bg_dark, self.text, self.lane_style.recess)
    }

    /// Every background the text tiers are drawn on.
    pub(crate) fn text_surfaces(&self) -> [(&'static str, Color); 5] {
        [
            ("bg_dark", self.bg_dark),
            ("bg_surface", self.bg_surface),
            ("bg_elevated", self.bg_elevated),
            ("display_bg", self.display_bg),
            ("lane", self.lane()),
        ]
    }
}

impl Default for ThemePalette {
    fn default() -> Self {
        crate::themes::charcoal()
    }
}

static CURRENT: LazyLock<RwLock<ThemePalette>> =
    LazyLock::new(|| RwLock::new(crate::themes::charcoal()));

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

/// How strongly track colours show. A viewer preference saved in
/// `ui.json` and applied over whichever theme is active, so a `.vzt` file
/// never carries it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ColourStrength {
    #[default]
    Vivid,
    Medium,
    Soft,
}

impl ColourStrength {
    /// A theme's track colour at this strength. Scaling HSL saturation
    /// keeps each track's hue and lightness, so tracks stay tellable apart
    /// and the ink drawn over them keeps its contrast.
    pub fn track(self, authored: Color) -> Color {
        let keep = match self {
            Self::Vivid => return authored,
            Self::Medium => MEDIUM_SATURATION,
            Self::Soft => SOFT_SATURATION,
        };
        let hsl = Hsl::of(authored);
        Hsl {
            saturation: hsl.saturation * keep,
            ..hsl
        }
        .color(authored.a)
    }

    /// A theme's clip style at this strength. Themes author the calm Medium
    /// body, so Vivid and Soft are measured from it.
    pub fn clip_style(self, authored: ClipStyle) -> ClipStyle {
        match self {
            Self::Vivid => ClipStyle {
                body_opacity: authored.body_opacity
                    + (1.0 - authored.body_opacity) * VIVID_BODY_TOWARD_SOLID,
                body_toward_lane: authored.body_toward_lane * VIVID_BODY_LANE_MIX_KEPT,
                ..authored
            },
            Self::Medium => authored,
            Self::Soft => ClipStyle {
                body_opacity: authored.body_opacity * SOFT_BODY_OPACITY_KEPT,
                body_toward_lane: authored.body_toward_lane
                    + (1.0 - authored.body_toward_lane) * SOFT_BODY_TOWARD_LANE,
                ..authored
            },
        }
    }
}

// Tuned by eye in Charcoal and Porcelain. Medium's saturation lands near
// card 28b's muted palette. Soft's body stays close to Medium's because a
// darker body on a dark lane loses the dark notes drawn over it.
const MEDIUM_SATURATION: f32 = 0.8;
const SOFT_SATURATION: f32 = 0.55;
const VIVID_BODY_TOWARD_SOLID: f32 = 0.7;
const VIVID_BODY_LANE_MIX_KEPT: f32 = 0.25;
const SOFT_BODY_OPACITY_KEPT: f32 = 0.92;
const SOFT_BODY_TOWARD_LANE: f32 = 0.06;

/// A colour as hue, saturation and lightness, all in 0..=1.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Hsl {
    pub hue: f32,
    pub saturation: f32,
    pub lightness: f32,
}

impl Hsl {
    pub fn of(c: Color) -> Self {
        let max = c.r.max(c.g).max(c.b);
        let min = c.r.min(c.g).min(c.b);
        let lightness = (max + min) / 2.0;
        let chroma = max - min;
        if chroma == 0.0 {
            return Self {
                hue: 0.0,
                saturation: 0.0,
                lightness,
            };
        }
        let saturation = chroma / (1.0 - (2.0 * lightness - 1.0).abs());
        let sector = if max == c.r {
            ((c.g - c.b) / chroma).rem_euclid(6.0)
        } else if max == c.g {
            (c.b - c.r) / chroma + 2.0
        } else {
            (c.r - c.g) / chroma + 4.0
        };
        Self {
            hue: sector / 6.0,
            saturation,
            lightness,
        }
    }

    fn color(self, alpha: f32) -> Color {
        let chroma = (1.0 - (2.0 * self.lightness - 1.0).abs()) * self.saturation;
        let sector = self.hue * 6.0;
        let second = chroma * (1.0 - (sector.rem_euclid(2.0) - 1.0).abs());
        let (r, g, b) = match sector as u32 {
            0 => (chroma, second, 0.0),
            1 => (second, chroma, 0.0),
            2 => (0.0, chroma, second),
            3 => (0.0, second, chroma),
            4 => (second, 0.0, chroma),
            _ => (chroma, 0.0, second),
        };
        let base = self.lightness - chroma / 2.0;
        Color {
            r: r + base,
            g: g + base,
            b: b + base,
            a: alpha,
        }
    }
}

/// A colour as OKLCH: perceptual lightness in 0..=1, chroma, and hue in
/// degrees. Palettes are authored in it because equal steps of its
/// lightness look equal, which HSL's do not.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Oklch {
    pub lightness: f32,
    pub chroma: f32,
    pub hue: f32,
}

impl Oklch {
    pub const fn new(lightness: f32, chroma: f32, hue: f32) -> Self {
        Self {
            lightness,
            chroma,
            hue,
        }
    }

    #[cfg(test)]
    pub fn of(c: Color) -> Self {
        let (r, g, b) = (linear(c.r), linear(c.g), linear(c.b));
        let l = (0.412_221_46 * r + 0.536_332_55 * g + 0.051_445_995 * b).cbrt();
        let m = (0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b).cbrt();
        let s = (0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b).cbrt();
        let a = 1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s;
        let b = 0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s;
        Self {
            lightness: 0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s,
            chroma: a.hypot(b),
            hue: b.atan2(a).to_degrees().rem_euclid(360.0),
        }
    }

    /// The same lightness and hue with chroma reduced as far as sRGB needs.
    pub fn fitted(self) -> Self {
        let fits = |chroma: f32| {
            Self { chroma, ..self }
                .linear_rgb()
                .iter()
                .all(|v| (-1e-4..=1.0 + 1e-4).contains(v))
        };
        if fits(self.chroma) {
            return self;
        }
        let (mut inside, mut outside) = (0.0, self.chroma);
        for _ in 0..24 {
            let mid = (inside + outside) / 2.0;
            if fits(mid) {
                inside = mid;
            } else {
                outside = mid;
            }
        }
        Self {
            chroma: inside,
            ..self
        }
    }

    /// The sRGB colour, each channel clipped into gamut.
    pub fn color(self) -> Color {
        let encoded = |v: f32| {
            let v = v.clamp(0.0, 1.0);
            if v <= 0.003_130_8 {
                12.92 * v
            } else {
                1.055 * v.powf(1.0 / 2.4) - 0.055
            }
        };
        let [r, g, b] = self.linear_rgb();
        Color::from_rgb(encoded(r), encoded(g), encoded(b))
    }

    /// Linear-light sRGB channels, outside 0..=1 when out of gamut.
    fn linear_rgb(self) -> [f32; 3] {
        let (sin, cos) = self.hue.to_radians().sin_cos();
        let (a, b) = (self.chroma * cos, self.chroma * sin);
        let l = (self.lightness + 0.396_337_78 * a + 0.215_803_76 * b).powi(3);
        let m = (self.lightness - 0.105_561_35 * a - 0.063_854_17 * b).powi(3);
        let s = (self.lightness - 0.089_484_18 * a - 1.291_485_5 * b).powi(3);
        [
            4.076_741_7 * l - 3.307_711_6 * m + 0.230_969_94 * s,
            -1.268_438 * l + 2.609_757_4 * m - 0.341_319_38 * s,
            -0.004_196_086_3 * l - 0.703_418_6 * m + 1.707_614_7 * s,
        ]
    }
}

/// An sRGB channel in linear light.
fn linear(v: f32) -> f32 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

/// WCAG 2 relative luminance of an sRGB colour, ignoring alpha.
pub(crate) fn relative_luminance(c: Color) -> f32 {
    0.2126 * linear(c.r) + 0.7152 * linear(c.g) + 0.0722 * linear(c.b)
}

/// WCAG 2 contrast ratio between two opaque colours, from 1 to 21.
pub(crate) fn contrast_ratio(a: Color, b: Color) -> f32 {
    let (a, b) = (relative_luminance(a), relative_luminance(b));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

static STRENGTH: RwLock<ColourStrength> = RwLock::new(ColourStrength::Vivid);

/// Swap the colour strength; the next frame repaints everything.
pub fn set_colour_strength(strength: ColourStrength) {
    *STRENGTH.write().unwrap() = strength;
    EPOCH.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
}

pub fn colour_strength() -> ColourStrength {
    *STRENGTH.read().unwrap()
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

pub fn clip_style() -> ClipStyle {
    colour_strength().clip_style(CURRENT.read().unwrap().clip_style)
}

#[allow(dead_code)]
pub fn control() -> Color {
    CURRENT.read().unwrap().control()
}

pub fn lane_style() -> LaneStyle {
    CURRENT.read().unwrap().lane_style
}

// ── Derived roles (aliases of palette fields) ──

pub fn mute_active() -> Color {
    danger()
}

pub fn solo_active() -> Color {
    meter_yellow()
}

/// Washes out clip content the clip never reaches, so a loop that runs
/// past the clip's end reads as outside it.
pub fn past_clip_end_shade() -> Color {
    with_alpha(bg_dark(), 0.6)
}

pub fn knob_bg() -> Color {
    bg_elevated()
}

pub fn fader_track() -> Color {
    bg_elevated()
}

pub fn track_bg() -> Color {
    CURRENT.read().unwrap().lane()
}

pub fn track_bg_selected() -> Color {
    bg_elevated()
}

/// A hovered track header, halfway to its selected shade.
pub fn track_header_hovered() -> Color {
    blend(bg_surface(), track_bg_selected(), 0.5)
}

pub fn ruler_bg() -> Color {
    bg_surface()
}

pub fn ruler_tick() -> Color {
    blend(ruler_bg(), text(), 0.22)
}

/// The strip above the ruler numbers that holds the loop brace.
pub fn ruler_loop_strip() -> Color {
    blend(ruler_bg(), Color::BLACK, 0.08)
}

/// The loop brace while the loop is off.
pub fn ruler_brace_off() -> Color {
    blend(ruler_bg(), text(), 0.25)
}

/// Mix two theme colors without escaping the active palette. Perform uses
/// this for instrument-like depth that still follows light, dark, and custom
/// `.vzt` themes.
///
/// Faint tints and fades are mixed with this rather than painted
/// translucent: the renderer blends alpha in linear light, which makes a
/// faint tint several times stronger than its token and fades dark ink on a
/// light background far more than on a dark one.
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
    colour_strength().track(palette.track_colors[index as usize % palette.track_colors.len()])
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

#[cfg(test)]
mod tests {
    use super::*;

    const fn rgb(r: f32, g: f32, b: f32) -> Color {
        Color { r, g, b, a: 1.0 }
    }

    const fn rgba(r: f32, g: f32, b: f32, a: f32) -> Color {
        Color { r, g, b, a }
    }

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
        let p = crate::themes::charcoal();
        let accent = hex::to_hex(&p.accent);
        let json = serde_json::to_string_pretty(&p).unwrap();
        assert!(json.contains(&format!("\"accent\": \"{accent}\"")));
        let back: ThemePalette = serde_json::from_str(&json).unwrap();
        assert_eq!(back.name, "Charcoal");
        assert_eq!(hex::to_hex(&back.accent), accent);
        assert_eq!(back.track_colors.len(), 8);
    }

    #[test]
    fn a_theme_file_without_clip_style_loads_with_the_dark_style() {
        let mut json = serde_json::to_value(crate::themes::charcoal()).unwrap();
        json.as_object_mut().unwrap().remove("clip_style");

        let palette: ThemePalette = serde_json::from_value(json).unwrap();

        assert_eq!(palette.clip_style, ClipStyle::DARK);
    }

    #[test]
    fn a_theme_file_without_lane_style_loads_with_the_dark_style() {
        let mut json = serde_json::to_value(crate::themes::charcoal()).unwrap();
        json.as_object_mut().unwrap().remove("lane_style");

        let palette: ThemePalette = serde_json::from_value(json).unwrap();

        assert_eq!(palette.lane_style, LaneStyle::DARK);
    }

    #[test]
    fn a_theme_file_without_a_control_fill_fills_controls_with_its_raised_shade() {
        let mut json = serde_json::to_value(crate::themes::charcoal()).unwrap();
        json.as_object_mut().unwrap().remove("control");

        let palette: ThemePalette = serde_json::from_value(json).unwrap();

        assert_eq!(palette.control(), palette.bg_elevated);
    }

    #[test]
    fn a_theme_file_keeps_its_control_fill() {
        let mut authored = crate::themes::charcoal();
        authored.control = Some(Color::from_rgb8(0x32, 0x34, 0x37));
        let json = serde_json::to_string(&authored).unwrap();

        let palette: ThemePalette = serde_json::from_str(&json).unwrap();

        assert_eq!(hex::to_hex(&palette.control()), "#323437");
    }

    fn default_track_colours() -> Vec<Color> {
        ["Charcoal", "Porcelain"]
            .into_iter()
            .flat_map(|name| crate::themes::builtin_by_name(name).unwrap().track_colors)
            .collect()
    }

    #[test]
    fn vivid_shows_track_colours_as_the_theme_authored_them() {
        for color in default_track_colours() {
            assert_eq!(ColourStrength::Vivid.track(color), color);
        }
    }

    #[test]
    fn medium_and_soft_desaturate_track_colours_keeping_their_lightness() {
        for color in default_track_colours() {
            let [soft, medium, vivid] = [
                ColourStrength::Soft,
                ColourStrength::Medium,
                ColourStrength::Vivid,
            ]
            .map(|strength| Hsl::of(strength.track(color)));

            assert!(
                soft.saturation < medium.saturation && medium.saturation < vivid.saturation,
                "{color:?}: soft {soft:?} medium {medium:?} vivid {vivid:?}"
            );
            for hsl in [soft, medium] {
                assert!(hsl.saturation > 0.0, "{color:?} keeps some colour");
                assert!((hsl.lightness - vivid.lightness).abs() < 0.01, "{color:?}");
            }
        }
    }

    #[test]
    fn medium_shows_the_authored_clip_style_and_vivid_bodies_are_the_most_solid() {
        for authored in [ClipStyle::DARK, ClipStyle::LIGHT] {
            let [soft, medium, vivid] = [
                ColourStrength::Soft,
                ColourStrength::Medium,
                ColourStrength::Vivid,
            ]
            .map(|strength| strength.clip_style(authored));

            assert_eq!(medium, authored);
            assert!(soft.body_opacity < medium.body_opacity);
            assert!(medium.body_opacity < vivid.body_opacity);
            assert!(vivid.body_toward_lane < medium.body_toward_lane);
            assert!(medium.body_toward_lane < soft.body_toward_lane);
            assert_eq!((soft.ink, vivid.ink), (authored.ink, authored.ink));
        }
    }

    #[test]
    fn oklch_converts_to_srgb_as_the_mockup_generator_does() {
        for (oklch, expected) in [
            (Oklch::new(0.215, 0.006, 260.0), "#181a1c"),
            (Oklch::new(0.74, 0.175, 55.0), "#fc871e"),
            (Oklch::new(0.60, 0.19, 29.0), "#da4336"),
            (Oklch::new(0.965, 0.004, 85.0), "#f5f3f0"),
        ] {
            assert_eq!(hex::to_hex(&oklch.color()), expected, "{oklch:?}");
        }
    }

    #[test]
    fn oklch_reads_back_the_lightness_chroma_and_hue_it_was_built_from() {
        let authored = Oklch::new(0.72, 0.11, 165.0);
        let read = Oklch::of(authored.color());
        assert!((read.lightness - 0.72).abs() < 0.005, "{read:?}");
        assert!((read.chroma - 0.11).abs() < 0.005, "{read:?}");
        assert!((read.hue - 165.0).abs() < 1.0, "{read:?}");
    }

    #[test]
    fn fitting_oklch_into_srgb_lowers_only_the_chroma() {
        let wanted = Oklch::new(0.78, 0.25, 246.0);

        let fitted = Oklch::of(wanted.fitted().color());

        assert!(fitted.chroma < wanted.chroma, "{fitted:?}");
        assert!(
            fitted.chroma > 0.08,
            "keeps as much colour as fits: {fitted:?}"
        );
        assert!(
            (fitted.lightness - wanted.lightness).abs() < 0.005,
            "{fitted:?}"
        );
        assert!((fitted.hue - wanted.hue).abs() < 2.0, "{fitted:?}");
        let inside = Oklch::new(0.6, 0.1, 30.0);
        assert_eq!(inside.fitted(), inside);
    }

    #[test]
    fn missing_field_is_a_load_error_not_a_panic() {
        let result = serde_json::from_str::<ThemePalette>("{\"name\": \"Broken\"}");
        assert!(result.is_err());
    }
}
