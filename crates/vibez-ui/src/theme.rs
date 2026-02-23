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
