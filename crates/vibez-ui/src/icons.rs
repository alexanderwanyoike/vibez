use iced::widget::text;
use iced::Font;

/// Lucide icon font loaded from assets/fonts/Lucide.ttf
pub const ICON_FONT: Font = Font::with_name("lucide");

/// Icon font bytes for loading at startup.
pub const ICON_FONT_BYTES: &[u8] = include_bytes!("../../../assets/fonts/Lucide.ttf");

// ── Icon codepoints (from lucide-static) ──

pub const PLAY: char = '\u{E13C}';
pub const PAUSE: char = '\u{E12E}';
pub const STOP: char = '\u{E167}'; // square
pub const SKIP_BACK: char = '\u{E15F}';
pub const SKIP_FORWARD: char = '\u{E160}';
pub const VOLUME_2: char = '\u{E1AB}';
pub const MUSIC: char = '\u{E122}';
pub const AUDIO_WAVEFORM: char = '\u{E55B}';
pub const PLUS: char = '\u{E13D}';
pub const TRASH_2: char = '\u{E18E}';
pub const X: char = '\u{E1B2}';
pub const CHEVRON_UP: char = '\u{E070}';
pub const CHEVRON_DOWN: char = '\u{E06D}';
pub const SLIDERS_VERTICAL: char = '\u{E162}';
pub const LAYOUT_LIST: char = '\u{E1D9}';
pub const POWER: char = '\u{E140}';
pub const CIRCLE: char = '\u{E076}';
pub const CIRCLE_DOT: char = '\u{E345}';
pub const COPY: char = '\u{E091}';
pub const SCISSORS: char = '\u{E152}';
pub const REPEAT: char = '\u{E146}';

/// Create an icon text element with the Lucide font.
pub fn icon(codepoint: char) -> iced::widget::Text<'static> {
    text(codepoint.to_string()).font(ICON_FONT)
}
