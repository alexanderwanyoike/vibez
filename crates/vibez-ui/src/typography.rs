//! Bundled type roles used where the system fallback cannot express the
//! intended interface hierarchy consistently across platforms.

use iced::{
    font::{Stretch, Weight},
    Font,
};

mod fit;
pub use fit::{fit_text, fit_text_middle};

pub const PLEX_SANS_CONDENSED_MEDIUM_BYTES: &[u8] =
    include_bytes!("../../../assets/fonts/ibm-plex/IBMPlexSansCondensed-Medium.ttf");
pub const PLEX_SANS_CONDENSED_SEMIBOLD_BYTES: &[u8] =
    include_bytes!("../../../assets/fonts/ibm-plex/IBMPlexSansCondensed-SemiBold.ttf");
pub const PLEX_MONO_MEDIUM_BYTES: &[u8] =
    include_bytes!("../../../assets/fonts/ibm-plex/IBMPlexMono-Medium.ttf");
pub const PLEX_MONO_SEMIBOLD_BYTES: &[u8] =
    include_bytes!("../../../assets/fonts/ibm-plex/IBMPlexMono-SemiBold.ttf");

/// Every font the app bundles, in the order it loads them.
pub const BUNDLED_FONTS: [&[u8]; 5] = [
    crate::icons::ICON_FONT_BYTES,
    PLEX_SANS_CONDENSED_MEDIUM_BYTES,
    PLEX_SANS_CONDENSED_SEMIBOLD_BYTES,
    PLEX_MONO_MEDIUM_BYTES,
    PLEX_MONO_SEMIBOLD_BYTES,
];

/// Loads [`BUNDLED_FONTS`] into the renderer's process-wide font system,
/// once, so tests shape and measure text in the faces the app draws with,
/// whatever order they run in.
#[cfg(test)]
pub(crate) fn load_bundled_fonts() {
    use std::sync::Once;

    static LOADED: Once = Once::new();
    LOADED.call_once(|| {
        let mut system = iced::advanced::graphics::text::font_system()
            .write()
            .expect("font system lock");
        for bytes in BUNDLED_FONTS {
            system.load_font(std::borrow::Cow::Borrowed(bytes));
        }
    });
}

/// The Plex Sans Condensed faces declare a condensed width, and cosmic-text
/// only offers faces whose width equals the one asked for, so a role that
/// asks for the normal width never reaches them and draws in whatever the
/// platform falls back to.
const PLEX_SANS_CONDENSED: Font = Font {
    stretch: Stretch::Condensed,
    ..Font::with_name("IBM Plex Sans Condensed")
};
pub const PERFORM_DISPLAY: Font = Font {
    weight: Weight::Medium,
    ..PLEX_SANS_CONDENSED
};
pub const PERFORM_LABEL: Font = Font {
    weight: Weight::Semibold,
    ..PLEX_SANS_CONDENSED
};
/// Clip names in timeline title strips: semibold and condensed, so more of
/// a name fits a short clip.
pub const CLIP_TITLE: Font = PERFORM_LABEL;
/// Track names and the corner titles in the Arrange header column.
pub const TRACK_TITLE: Font = PERFORM_LABEL;
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

    /// Family and weight of every face the renderer's font system shapes
    /// `content` with. The system fonts stay in, so a role that misses its
    /// bundled face shows whichever fallback this platform picks.
    fn drawn_faces(content: &str, font: Font) -> Vec<(String, u16)> {
        use iced::advanced::graphics::text::{cosmic_text, font_system, to_attributes};

        load_bundled_fonts();
        let mut system = font_system().write().expect("font system lock");
        let raw = system.raw();
        let mut buffer = cosmic_text::Buffer::new(raw, cosmic_text::Metrics::new(10.5, 13.0));
        buffer.set_text(
            raw,
            content,
            to_attributes(font),
            cosmic_text::Shaping::Advanced,
        );
        buffer.shape_until_scroll(raw, false);
        let mut faces: Vec<(String, u16)> = buffer
            .layout_runs()
            .flat_map(|run| run.glyphs.iter().map(|glyph| glyph.font_id))
            .map(|id| {
                let face = raw.db().face(id).expect("shaped face is loaded");
                let family = face
                    .families
                    .first()
                    .map_or_else(|| face.post_script_name.clone(), |(name, _)| name.clone());
                (family, face.weight.0)
            })
            .collect();
        faces.dedup();
        faces
    }

    #[test]
    fn each_role_draws_digits_with_its_bundled_face() {
        let roles = [
            (
                "PERFORM_DISPLAY",
                PERFORM_DISPLAY,
                "IBM Plex Sans Condensed",
                500,
            ),
            (
                "PERFORM_LABEL",
                PERFORM_LABEL,
                "IBM Plex Sans Condensed",
                600,
            ),
            ("PERFORM_TECH", PERFORM_TECH, "IBM Plex Mono", 500),
            (
                "PERFORM_TECH_STRONG",
                PERFORM_TECH_STRONG,
                "IBM Plex Mono",
                600,
            ),
        ];

        let drawn: Vec<_> = roles
            .iter()
            .map(|&(role, font, _, _)| (role, drawn_faces("0:00", font)))
            .collect();
        let bundled: Vec<_> = roles
            .iter()
            .map(|&(role, _, family, weight)| (role, vec![(family.to_string(), weight)]))
            .collect();

        assert_eq!(drawn, bundled);
    }
}
