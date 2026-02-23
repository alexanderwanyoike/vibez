use iced::theme::Palette;
use iced::{Color, Theme};

/// Vibez accent blue: #6190FF
pub const ACCENT: Color = Color {
    r: 0.380,
    g: 0.565,
    b: 1.0,
    a: 1.0,
};

/// Dark background: #1A1A2E
pub const BG_DARK: Color = Color {
    r: 0.102,
    g: 0.102,
    b: 0.180,
    a: 1.0,
};

/// Surface/panel background: #22223A
pub const BG_SURFACE: Color = Color {
    r: 0.133,
    g: 0.133,
    b: 0.227,
    a: 1.0,
};

/// Text color: #E0E0F0
pub const TEXT: Color = Color {
    r: 0.878,
    g: 0.878,
    b: 0.941,
    a: 1.0,
};

/// Dimmed text: #8888AA
pub const TEXT_DIM: Color = Color {
    r: 0.533,
    g: 0.533,
    b: 0.667,
    a: 1.0,
};

/// Success/green for play: #4ADE80
pub const SUCCESS: Color = Color {
    r: 0.290,
    g: 0.871,
    b: 0.502,
    a: 1.0,
};

/// Danger/red for stop: #F87171
pub const DANGER: Color = Color {
    r: 0.973,
    g: 0.443,
    b: 0.443,
    a: 1.0,
};

/// Waveform color
pub const WAVEFORM: Color = Color {
    r: 0.380,
    g: 0.565,
    b: 1.0,
    a: 0.8,
};

/// Playhead color
pub const PLAYHEAD: Color = Color {
    r: 1.0,
    g: 1.0,
    b: 1.0,
    a: 0.9,
};

/// VU meter green
pub const METER_GREEN: Color = Color {
    r: 0.290,
    g: 0.871,
    b: 0.502,
    a: 1.0,
};

/// VU meter yellow
pub const METER_YELLOW: Color = Color {
    r: 1.0,
    g: 0.843,
    b: 0.0,
    a: 1.0,
};

/// VU meter red
pub const METER_RED: Color = Color {
    r: 0.973,
    g: 0.443,
    b: 0.443,
    a: 1.0,
};

// -- Phase 2 colors --

/// Track lane background
pub const TRACK_BG: Color = Color {
    r: 0.118,
    g: 0.118,
    b: 0.200,
    a: 1.0,
};

/// Selected track background
pub const TRACK_BG_SELECTED: Color = Color {
    r: 0.160,
    g: 0.160,
    b: 0.260,
    a: 1.0,
};

/// Clip body color
pub const CLIP_BODY: Color = Color {
    r: 0.300,
    g: 0.480,
    b: 0.900,
    a: 0.6,
};

/// Clip border color
pub const CLIP_BORDER: Color = Color {
    r: 0.380,
    g: 0.565,
    b: 1.0,
    a: 0.9,
};

/// Fader track color
pub const FADER_TRACK: Color = Color {
    r: 0.180,
    g: 0.180,
    b: 0.280,
    a: 1.0,
};

/// Fader handle color
pub const FADER_HANDLE: Color = Color {
    r: 0.700,
    g: 0.700,
    b: 0.800,
    a: 1.0,
};

/// Knob background
pub const KNOB_BG: Color = Color {
    r: 0.160,
    g: 0.160,
    b: 0.250,
    a: 1.0,
};

/// Knob arc color
pub const KNOB_ARC: Color = Color {
    r: 0.380,
    g: 0.565,
    b: 1.0,
    a: 1.0,
};

/// Mute button active color
pub const MUTE_ACTIVE: Color = Color {
    r: 0.973,
    g: 0.443,
    b: 0.443,
    a: 1.0,
};

/// Solo button active color
pub const SOLO_ACTIVE: Color = Color {
    r: 1.0,
    g: 0.843,
    b: 0.0,
    a: 1.0,
};

/// Time ruler background
pub const RULER_BG: Color = Color {
    r: 0.110,
    g: 0.110,
    b: 0.190,
    a: 1.0,
};

/// Time ruler text
pub const RULER_TEXT: Color = Color {
    r: 0.533,
    g: 0.533,
    b: 0.667,
    a: 1.0,
};

/// Time ruler line
pub const RULER_LINE: Color = Color {
    r: 0.300,
    g: 0.300,
    b: 0.420,
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
