//! Bundled type roles used where the system fallback cannot express the
//! intended interface hierarchy consistently across platforms.

use iced::advanced::graphics::text::{cosmic_text::fontdb, font_system};
use iced::{font::Weight, Font};

#[cfg(test)]
mod coverage_tests;
mod fit;
pub use fit::{fit_text, fit_text_middle};

const PLEX_SANS_CONDENSED_MEDIUM_BYTES: &[u8] =
    include_bytes!("../../../assets/fonts/ibm-plex/IBMPlexSansCondensed-Medium.ttf");
const PLEX_SANS_CONDENSED_SEMIBOLD_BYTES: &[u8] =
    include_bytes!("../../../assets/fonts/ibm-plex/IBMPlexSansCondensed-SemiBold.ttf");
const PLEX_MONO_MEDIUM_BYTES: &[u8] =
    include_bytes!("../../../assets/fonts/ibm-plex/IBMPlexMono-Medium.ttf");
const PLEX_MONO_SEMIBOLD_BYTES: &[u8] =
    include_bytes!("../../../assets/fonts/ibm-plex/IBMPlexMono-SemiBold.ttf");

/// Every font the app bundles, in the order it loads them.
const BUNDLED_FONTS: [&[u8]; 5] = [
    crate::icons::ICON_FONT_BYTES,
    PLEX_SANS_CONDENSED_MEDIUM_BYTES,
    PLEX_SANS_CONDENSED_SEMIBOLD_BYTES,
    PLEX_MONO_MEDIUM_BYTES,
    PLEX_MONO_SEMIBOLD_BYTES,
];

/// Loads [`BUNDLED_FONTS`] into the renderer's process-wide font system,
/// once, each face registered at the normal width. Call it before a window
/// opens, and in any test that shapes or measures text, so that test sees
/// the faces the app draws with whatever order the tests run in.
///
/// Plex Sans Condensed declares a condensed width, and cosmic-text only
/// offers a face, or falls back for a character a face lacks, among faces
/// of the width a text asks for. A role asking for the condensed width would
/// draw every script Plex lacks as empty boxes, and one asking for the
/// normal width would never reach the condensed faces, so the faces are
/// registered at the width the roles ask for.
pub fn load_bundled_fonts() {
    use std::sync::{Arc, Once};

    static LOADED: Once = Once::new();
    LOADED.call_once(|| {
        let mut system = font_system().write().expect("font system lock");
        let db = system.raw().db_mut();
        for bytes in BUNDLED_FONTS {
            for id in db.load_font_source(fontdb::Source::Binary(Arc::new(bytes))) {
                register_at_normal_width(db, id);
            }
        }
    });
}

fn register_at_normal_width(db: &mut fontdb::Database, id: fontdb::ID) {
    let Some(face) = db
        .face(id)
        .filter(|face| face.stretch != fontdb::Stretch::Normal)
        .cloned()
    else {
        return;
    };
    db.remove_face(id);
    db.push_face_info(fontdb::FaceInfo {
        stretch: fontdb::Stretch::Normal,
        ..face
    });
}

const PLEX_SANS_CONDENSED_FAMILY: &str = "IBM Plex Sans Condensed";
const PLEX_MONO_FAMILY: &str = "IBM Plex Mono";

/// A family is only used at a weight it has a face for, so every role's
/// weight needs a bundled face.
const PLEX_SANS_CONDENSED: Font = Font::with_name(PLEX_SANS_CONDENSED_FAMILY);
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
    ..Font::with_name(PLEX_MONO_FAMILY)
};
pub const PERFORM_TECH_STRONG: Font = Font {
    weight: Weight::Semibold,
    ..Font::with_name(PLEX_MONO_FAMILY)
};

/// `text`, or as much of it as fits `width` before an ellipsis; empty when
/// not even one letter fits.
pub(crate) fn fit(text: &str, width: f32, size: f32, font: Font) -> String {
    fit_text(text, width, size, font, 1).unwrap_or_default()
}

/// The loaded faces `keep` accepts, parsed as cosmic-text shapes with them.
#[cfg(test)]
fn loaded_faces(
    keep: impl Fn(&fontdb::FaceInfo) -> bool,
) -> Vec<std::sync::Arc<iced::advanced::graphics::text::cosmic_text::Font>> {
    load_bundled_fonts();
    let mut system = font_system().write().expect("font system lock");
    let raw = system.raw();
    let ids: Vec<_> = raw
        .db()
        .faces()
        .filter(|face| keep(face))
        .map(|face| face.id)
        .collect();
    ids.into_iter().filter_map(|id| raw.get_font(id)).collect()
}

/// Whether `face` has a glyph for `c`.
#[cfg(test)]
fn draws(face: &iced::advanced::graphics::text::cosmic_text::Font, c: char) -> bool {
    face.rustybuzz().glyph_index(c).is_some()
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

    /// Family, weight and glyph of every glyph the renderer's font system
    /// shapes `content` with. The system fonts stay in, so a role that misses
    /// its bundled face shows whichever fallback this platform picks. Glyph 0
    /// is the empty box a face draws for a character it lacks.
    ///
    /// It shapes as `text_width` measures, with fallback. Text drawn with
    /// iced's default basic shaping uses only the first face that matches,
    /// so a character that face lacks stays an empty box.
    fn shaped_glyphs(content: &str, font: Font) -> Vec<(String, u16, u16)> {
        use iced::advanced::graphics::text::{cosmic_text, to_attributes};

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
        buffer
            .layout_runs()
            .flat_map(|run| run.glyphs.iter())
            .map(|glyph| {
                let face = raw.db().face(glyph.font_id).expect("shaped face is loaded");
                let family = face
                    .families
                    .first()
                    .map_or_else(|| face.post_script_name.clone(), |(name, _)| name.clone());
                (family, face.weight.0, glyph.glyph_id)
            })
            .collect()
    }

    /// Family and weight of every face `content` is shaped with.
    fn drawn_faces(content: &str, font: Font) -> Vec<(String, u16)> {
        let mut faces: Vec<(String, u16)> = shaped_glyphs(content, font)
            .into_iter()
            .map(|(family, weight, _)| (family, weight))
            .collect();
        faces.dedup();
        faces
    }

    /// Whether a face the app does not bundle has a glyph for every char of
    /// `content`. The app's faces load from memory, the system's from files.
    fn a_system_face_covers(content: &str) -> bool {
        loaded_faces(|face| !matches!(face.source, fontdb::Source::Binary(_)))
            .iter()
            .any(|face| content.chars().all(|c| draws(face, c)))
    }

    #[test]
    fn each_role_draws_digits_with_its_bundled_face() {
        let roles = [
            (
                "PERFORM_DISPLAY",
                PERFORM_DISPLAY,
                PLEX_SANS_CONDENSED_FAMILY,
                500,
            ),
            (
                "PERFORM_LABEL",
                PERFORM_LABEL,
                PLEX_SANS_CONDENSED_FAMILY,
                600,
            ),
            ("PERFORM_TECH", PERFORM_TECH, PLEX_MONO_FAMILY, 500),
            (
                "PERFORM_TECH_STRONG",
                PERFORM_TECH_STRONG,
                PLEX_MONO_FAMILY,
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

    #[test]
    fn a_japanese_name_shaped_with_fallback_in_a_plex_role_draws_in_a_system_face() {
        let name = "ドラム";
        if !a_system_face_covers(name) {
            eprintln!("no system face here draws {name}, so there is nothing to fall back to");
            return;
        }

        for (role, font) in [
            ("PERFORM_DISPLAY", PERFORM_DISPLAY),
            ("PERFORM_LABEL", PERFORM_LABEL),
        ] {
            let glyphs = shaped_glyphs(name, font);

            assert!(
                glyphs
                    .iter()
                    .all(|(family, _, glyph)| *glyph != 0 && family != PLEX_SANS_CONDENSED_FAMILY),
                "{role} drew {name} as {glyphs:?}"
            );
        }
    }
}
