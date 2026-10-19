//! Bundled type roles used where the system fallback cannot express the
//! intended interface hierarchy consistently across platforms.

use iced::{font::Weight, Font};

pub const PLEX_SANS_CONDENSED_MEDIUM_BYTES: &[u8] =
    include_bytes!("../../../assets/fonts/ibm-plex/IBMPlexSansCondensed-Medium.ttf");
pub const PLEX_SANS_CONDENSED_SEMIBOLD_BYTES: &[u8] =
    include_bytes!("../../../assets/fonts/ibm-plex/IBMPlexSansCondensed-SemiBold.ttf");
pub const PLEX_MONO_MEDIUM_BYTES: &[u8] =
    include_bytes!("../../../assets/fonts/ibm-plex/IBMPlexMono-Medium.ttf");
pub const PLEX_MONO_SEMIBOLD_BYTES: &[u8] =
    include_bytes!("../../../assets/fonts/ibm-plex/IBMPlexMono-SemiBold.ttf");

pub const PERFORM_DISPLAY: Font = Font {
    weight: Weight::Medium,
    ..Font::with_name("IBM Plex Sans Condensed")
};
pub const PERFORM_LABEL: Font = Font {
    weight: Weight::Semibold,
    ..Font::with_name("IBM Plex Sans Condensed")
};
pub const PERFORM_TECH: Font = Font {
    weight: Weight::Medium,
    ..Font::with_name("IBM Plex Mono")
};
pub const PERFORM_TECH_STRONG: Font = Font {
    weight: Weight::Semibold,
    ..Font::with_name("IBM Plex Mono")
};

/// `text` shortened with ".." to fit `width` at font `size`, by an average
/// condensed glyph width; empty when not even a few letters fit.
pub(crate) fn fit(text: &str, width: f32, size: f32) -> String {
    let limit = (width / (size * 0.61)).max(0.0) as usize;
    if text.chars().count() <= limit {
        text.into()
    } else if limit < 3 {
        String::new()
    } else {
        format!("{}..", text.chars().take(limit - 2).collect::<String>())
    }
}
