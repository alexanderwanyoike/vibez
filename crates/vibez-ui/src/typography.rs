//! Bundled type roles used where the system fallback cannot express the
//! intended interface hierarchy consistently across platforms.

use iced::{font::Weight, Font};

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
    /// `content` with, after loading the fonts the app bundles in the order
    /// `app::run` loads them. The system fonts stay in, so a role that
    /// misses its bundled face shows whichever fallback this platform picks.
    fn drawn_faces(content: &str, font: Font) -> Vec<(String, u16)> {
        use iced::advanced::graphics::text::{cosmic_text, font_system, to_attributes};
        use std::borrow::Cow;
        use std::sync::Once;

        static LOAD_BUNDLED: Once = Once::new();
        let mut system = font_system().write().expect("font system lock");
        LOAD_BUNDLED.call_once(|| {
            for bytes in [
                crate::icons::ICON_FONT_BYTES,
                PLEX_SANS_CONDENSED_MEDIUM_BYTES,
                PLEX_SANS_CONDENSED_SEMIBOLD_BYTES,
                PLEX_MONO_MEDIUM_BYTES,
                PLEX_MONO_SEMIBOLD_BYTES,
            ] {
                system.load_font(Cow::Borrowed(bytes));
            }
        });
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
