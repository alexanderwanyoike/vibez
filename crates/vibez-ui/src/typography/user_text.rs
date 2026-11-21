//! How text showing a producer's names (Track, Clip and Section names) is
//! shaped.

use iced::widget::text::Shaping;

/// The shaping for any text that shows a Track, Clip or Section name,
/// alone or inside a longer line such as a status message, a dialog title
/// or a picker's options.
/// Names can be in any script, and only advanced shaping falls back to a
/// system face for characters the interface font lacks; iced's default
/// basic shaping draws them as empty boxes.
pub const USER_TEXT_SHAPING: Shaping = Shaping::Advanced;

#[cfg(test)]
mod tests {
    use iced::advanced::graphics::text::{font_system, Paragraph};
    use iced::advanced::text::Paragraph as _;
    use iced::alignment::{Horizontal, Vertical};
    use iced::widget::text::Wrapping;
    use iced::{Font, Size};

    use super::USER_TEXT_SHAPING;
    use crate::typography::{
        a_system_face_covers, is_bundled, load_bundled_fonts, CLIP_TITLE, PERFORM_DISPLAY,
        PERFORM_TECH, TRACK_TITLE,
    };

    /// Glyph id and whether the face is bundled with the app, for every
    /// glyph of `name` laid out the way a name is drawn. Glyph 0 is the empty
    /// box a face draws for a character it lacks.
    fn shaped(name: &str, font: Font) -> Vec<(u16, bool)> {
        let paragraph = Paragraph::with_text(iced::advanced::Text {
            content: name,
            bounds: Size::INFINITY,
            size: 13.0.into(),
            line_height: Default::default(),
            font,
            horizontal_alignment: Horizontal::Left,
            vertical_alignment: Vertical::Top,
            shaping: USER_TEXT_SHAPING,
            wrapping: Wrapping::None,
        });
        let mut system = font_system().write().expect("font system lock");
        let db = system.raw().db();
        paragraph
            .buffer()
            .layout_runs()
            .flat_map(|run| run.glyphs.iter())
            .map(|glyph| {
                let face = db.face(glyph.font_id).expect("shaped face is loaded");
                (glyph.glyph_id, is_bundled(face))
            })
            .collect()
    }

    #[test]
    fn japanese_and_chinese_names_draw_in_a_system_face() {
        load_bundled_fonts();
        let mut checked = 0;
        for name in ["ドラム", "贝斯线"] {
            if !a_system_face_covers(name) {
                eprintln!("no system face here draws {name}, so there is nothing to fall back to");
                continue;
            }
            checked += 1;
            for (role, font) in [
                ("TRACK_TITLE", TRACK_TITLE),
                ("CLIP_TITLE", CLIP_TITLE),
                ("PERFORM_DISPLAY", PERFORM_DISPLAY),
                ("PERFORM_TECH", PERFORM_TECH),
                ("DEFAULT", Font::DEFAULT),
            ] {
                let glyphs = shaped(name, font);

                assert!(
                    glyphs
                        .iter()
                        .all(|&(glyph, bundled)| glyph != 0 && !bundled),
                    "{role} drew {name} as {glyphs:?}"
                );
            }
        }
        // Windows and macOS ship CJK faces, so finding none there means the
        // font system missed them, not that there is nothing to check.
        if cfg!(any(windows, target_os = "macos")) {
            assert!(checked > 0, "no system face was found for any name");
        }
    }
}
