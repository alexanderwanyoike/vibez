use iced::theme::Palette;
use iced::{Color, Theme};

// ── Main palette (dark charcoal) ──

/// Near-black main background: #1a1a1a
pub const BG_DARK: Color = Color {
    r: 0.102,
    g: 0.102,
    b: 0.102,
    a: 1.0,
};

/// Panels, headers, strips: #242424
pub const BG_SURFACE: Color = Color {
    r: 0.141,
    g: 0.141,
    b: 0.141,
    a: 1.0,
};

/// Cards, effect slots, raised elements: #2d2d2d
pub const BG_ELEVATED: Color = Color {
    r: 0.176,
    g: 0.176,
    b: 0.176,
    a: 1.0,
};

/// Hover state for interactive elements: #363636
#[allow(dead_code)]
pub const BG_HOVER: Color = Color {
    r: 0.212,
    g: 0.212,
    b: 0.212,
    a: 1.0,
};

// ── Text ──

/// Primary text: #e0e0e0
pub const TEXT: Color = Color {
    r: 0.878,
    g: 0.878,
    b: 0.878,
    a: 1.0,
};

/// Secondary text, labels: #808080
pub const TEXT_DIM: Color = Color {
    r: 0.502,
    g: 0.502,
    b: 0.502,
    a: 1.0,
};

/// Disabled, placeholder: #505050
pub const TEXT_MUTED: Color = Color {
    r: 0.314,
    g: 0.314,
    b: 0.314,
    a: 1.0,
};

// ── Accent ──

/// Orange accent — transport active, selected: #ff8c00
pub const ACCENT: Color = Color {
    r: 1.0,
    g: 0.549,
    b: 0.0,
    a: 1.0,
};

/// Accent at lower intensity: #995400
pub const ACCENT_DIM: Color = Color {
    r: 0.600,
    g: 0.329,
    b: 0.0,
    a: 1.0,
};

// ── Borders / dividers ──

/// Subtle panel borders: #3a3a3a
pub const BORDER: Color = Color {
    r: 0.227,
    g: 0.227,
    b: 0.227,
    a: 1.0,
};

/// Stronger borders for cards: #4a4a4a
#[allow(dead_code)]
pub const BORDER_LIGHT: Color = Color {
    r: 0.290,
    g: 0.290,
    b: 0.290,
    a: 1.0,
};

/// Section dividers: #2a2a2a
pub const DIVIDER: Color = Color {
    r: 0.165,
    g: 0.165,
    b: 0.165,
    a: 1.0,
};

// ── Semantic colors ──

/// Success/green for play: #4ade80
pub const SUCCESS: Color = Color {
    r: 0.290,
    g: 0.871,
    b: 0.502,
    a: 1.0,
};

/// Danger/red: #f87171
pub const DANGER: Color = Color {
    r: 0.973,
    g: 0.443,
    b: 0.443,
    a: 1.0,
};

/// Playhead: white at 80% opacity
pub const PLAYHEAD: Color = Color {
    r: 1.0,
    g: 1.0,
    b: 1.0,
    a: 0.8,
};

// ── Meter colors ──

/// VU meter green: #4ade80
pub const METER_GREEN: Color = Color {
    r: 0.290,
    g: 0.871,
    b: 0.502,
    a: 1.0,
};

/// VU meter yellow: #ffd700
pub const METER_YELLOW: Color = Color {
    r: 1.0,
    g: 0.843,
    b: 0.0,
    a: 1.0,
};

/// VU meter red: #f87171
pub const METER_RED: Color = Color {
    r: 0.973,
    g: 0.443,
    b: 0.443,
    a: 1.0,
};

// ── Button state colors ──

/// Mute active: #f87171 (red)
pub const MUTE_ACTIVE: Color = DANGER;

/// Solo active: #ffd700 (yellow)
pub const SOLO_ACTIVE: Color = METER_YELLOW;

// ── Knob / fader colors ──

/// Knob background: BG_ELEVATED
pub const KNOB_BG: Color = BG_ELEVATED;

/// Default knob arc color (overridden by track color): #888888
#[allow(dead_code)]
pub const KNOB_ARC: Color = Color {
    r: 0.533,
    g: 0.533,
    b: 0.533,
    a: 1.0,
};

/// Fader track color: BG_ELEVATED
pub const FADER_TRACK: Color = BG_ELEVATED;

/// Fader handle color: #888888
pub const FADER_HANDLE: Color = Color {
    r: 0.533,
    g: 0.533,
    b: 0.533,
    a: 1.0,
};

// ── Track colors (auto-assigned per track) ──

pub const TRACK_COLORS: [Color; 8] = [
    // Red: #e06060
    Color {
        r: 0.878,
        g: 0.376,
        b: 0.376,
        a: 1.0,
    },
    // Orange: #e09040
    Color {
        r: 0.878,
        g: 0.565,
        b: 0.251,
        a: 1.0,
    },
    // Yellow: #d0c040
    Color {
        r: 0.816,
        g: 0.753,
        b: 0.251,
        a: 1.0,
    },
    // Green: #50b060
    Color {
        r: 0.314,
        g: 0.690,
        b: 0.376,
        a: 1.0,
    },
    // Cyan: #40a0b0
    Color {
        r: 0.251,
        g: 0.627,
        b: 0.690,
        a: 1.0,
    },
    // Blue: #6080d0
    Color {
        r: 0.376,
        g: 0.502,
        b: 0.816,
        a: 1.0,
    },
    // Purple: #9060c0
    Color {
        r: 0.565,
        g: 0.376,
        b: 0.753,
        a: 1.0,
    },
    // Pink: #c060a0
    Color {
        r: 0.753,
        g: 0.376,
        b: 0.627,
        a: 1.0,
    },
];

/// Get a track color by index (wraps around).
pub fn track_color(index: u8) -> Color {
    TRACK_COLORS[index as usize % TRACK_COLORS.len()]
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
pub fn with_alpha(color: Color, alpha: f32) -> Color {
    Color { a: alpha, ..color }
}

// ── Aliases for backward compatibility in widgets ──

/// Track lane background
pub const TRACK_BG: Color = BG_DARK;

/// Selected track background
pub const TRACK_BG_SELECTED: Color = BG_ELEVATED;

/// Clip body color (default, overridden by track color)
#[allow(dead_code)]
pub const CLIP_BODY: Color = Color {
    r: 0.300,
    g: 0.480,
    b: 0.900,
    a: 0.6,
};

/// Clip border color (default, overridden by track color)
#[allow(dead_code)]
pub const CLIP_BORDER: Color = Color {
    r: 0.380,
    g: 0.565,
    b: 1.0,
    a: 0.9,
};

/// Waveform display color (default, overridden by track color)
pub const WAVEFORM: Color = Color {
    r: 0.380,
    g: 0.565,
    b: 1.0,
    a: 0.6,
};

/// Ruler background
pub const RULER_BG: Color = BG_SURFACE;

/// Ruler text
pub const RULER_TEXT: Color = TEXT_DIM;

/// Ruler line
#[allow(dead_code)]
pub const RULER_LINE: Color = Color {
    r: 0.227,
    g: 0.227,
    b: 0.227,
    a: 0.5,
};

pub fn vibez_theme() -> Theme {
    Theme::custom(
        "Vibez".to_string(),
        Palette {
            background: BG_DARK,
            text: TEXT,
            primary: ACCENT,
            success: SUCCESS,
            danger: DANGER,
        },
    )
}

/// Uniform device-card body height across the device chain: the
/// panel scrolls horizontally, so cards share one rack height.
pub const DEVICE_BODY_H: f32 = 184.0;
