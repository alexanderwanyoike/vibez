//! Bundled type roles used where the system fallback cannot express the
//! intended interface hierarchy consistently across platforms.

use iced::{font::Weight, Font};

mod fit;
pub use fit::fit_text;

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
/// Clip names in timeline title strips: semibold and condensed, so more of
/// a name fits a short clip.
pub const CLIP_TITLE: Font = PERFORM_LABEL;
pub const PERFORM_TECH: Font = Font {
    weight: Weight::Medium,
    ..Font::with_name("IBM Plex Mono")
};
pub const PERFORM_TECH_STRONG: Font = Font {
    weight: Weight::Semibold,
    ..Font::with_name("IBM Plex Mono")
};

/// `text`, or as much of it as fits `width` before an ellipsis; empty when
/// not even one letter fits.
pub(crate) fn fit(text: &str, width: f32, size: f32, font: Font) -> String {
    fit_text(text, width, size, font, 1).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::device_controls::text_width;

    #[test]
    fn a_fitted_name_stays_inside_its_measured_width() {
        let width = 60.0;

        let fitted = fit("WWWWWWWWWWWWWWWW", width, 13.0, Font::DEFAULT);

        assert!(!fitted.is_empty());
        assert!(text_width(&fitted, 13.0, Font::DEFAULT) <= width);
    }
}
