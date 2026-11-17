//! Every string literal in the UI must be drawable by the bundled Plex Sans
//! Condensed faces. Those roles ask for a condensed width, and cosmic-text
//! only falls back to faces of the width asked for, which few systems have,
//! so a character the bundled faces lack draws as an empty box. Symbols go
//! through the icon font instead.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use iced::advanced::graphics::text::{cosmic_text, font_system};

use super::load_bundled_fonts;

/// Literals drawn in the default font, which falls back across the system's
/// faces, so they may hold characters Plex lacks. Each is a path under
/// `src` and the literal as written.
const DRAWN_IN_THE_DEFAULT_FONT: &[(&str, &str)] = &[
    ("widgets/device_controls.rs", "\\u{25BE}"),
    ("widgets/device_controls.rs", "\\u{25B8}"),
    ("widgets/gain_device.rs", "\\u{25CF} Clip"),
    ("app/views_arrangement.rs", "\\u{25CF}"),
    ("app/views_clip_launcher.rs", "■"),
];

/// The bundled Plex Sans Condensed faces, one per weight.
fn plex_condensed_faces() -> Vec<Arc<cosmic_text::Font>> {
    load_bundled_fonts();
    let mut system = font_system().write().expect("font system lock");
    let raw = system.raw();
    let ids: Vec<_> = raw
        .db()
        .faces()
        .filter(|face| {
            face.families
                .iter()
                .any(|(name, _)| name == "IBM Plex Sans Condensed")
        })
        .map(|face| face.id)
        .collect();
    let faces: Vec<_> = ids
        .into_iter()
        .map(|id| raw.get_font(id).expect("bundled face parses"))
        .collect();
    assert_eq!(
        faces.len(),
        2,
        "both Plex Sans Condensed weights are loaded"
    );
    faces
}

fn production_sources(dir: &Path, found: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("source folder reads") {
        let path = entry.expect("source entry reads").path();
        if path.is_dir() {
            production_sources(&path, found);
        } else if path.extension().is_some_and(|ext| ext == "rs")
            && !path.to_string_lossy().ends_with("_tests.rs")
        {
            found.push(path);
        }
    }
}

/// A string literal as written between its quotes, the text it holds and
/// the line it starts on.
struct Literal {
    written: String,
    text: String,
    line: usize,
}

/// The string literals of `source` before its trailing `mod tests`, outside
/// comments.
fn string_literals(source: &str) -> Vec<Literal> {
    let source = source.replace("\r\n", "\n");
    let source = source
        .find("\n#[cfg(test)]\nmod tests {")
        .map_or(source.as_str(), |end| &source[..end]);
    let chars: Vec<char> = source.chars().collect();
    let mut literals = Vec::new();
    let mut line = 1;
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '\n' => {
                line += 1;
                i += 1;
            }
            '/' if chars.get(i + 1) == Some(&'/') => {
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
            }
            '\'' => {
                // A char literal, or a lifetime, which has no closing quote.
                if chars.get(i + 1) == Some(&'\\') {
                    i += 2;
                    while i < chars.len() && chars[i] != '\'' {
                        i += 1;
                    }
                    i += 1;
                } else if chars.get(i + 2) == Some(&'\'') {
                    i += 3;
                } else {
                    i += 1;
                }
            }
            'r' if matches!(chars.get(i + 1), Some('"' | '#'))
                && (i == 0 || !chars[i - 1].is_alphanumeric()) =>
            {
                let hashes = chars[i + 1..].iter().take_while(|c| **c == '#').count();
                let open = i + 1 + hashes;
                if chars.get(open) != Some(&'"') {
                    i += 1;
                    continue;
                }
                let start_line = line;
                let mut end = open + 1;
                while end < chars.len()
                    && !(chars[end] == '"'
                        && chars[end + 1..].iter().take_while(|c| **c == '#').count() >= hashes)
                {
                    if chars[end] == '\n' {
                        line += 1;
                    }
                    end += 1;
                }
                let written: String = chars[open + 1..end.min(chars.len())].iter().collect();
                literals.push(Literal {
                    text: written.clone(),
                    written,
                    line: start_line,
                });
                i = end + 1 + hashes;
            }
            '"' => {
                let start_line = line;
                let mut written = String::new();
                let mut text = String::new();
                i += 1;
                while i < chars.len() && chars[i] != '"' {
                    let c = chars[i];
                    written.push(c);
                    if c == '\n' {
                        line += 1;
                    }
                    if c != '\\' {
                        text.push(c);
                        i += 1;
                        continue;
                    }
                    let escaped = chars.get(i + 1).copied().unwrap_or_default();
                    written.push(escaped);
                    i += 2;
                    match escaped {
                        'u' => {
                            let mut hex = String::new();
                            while i < chars.len() && chars[i] != '}' {
                                written.push(chars[i]);
                                if chars[i] != '{' {
                                    hex.push(chars[i]);
                                }
                                i += 1;
                            }
                            written.push('}');
                            i += 1;
                            text.extend(
                                u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32),
                            );
                        }
                        '\n' => {
                            line += 1;
                            while i < chars.len() && chars[i].is_whitespace() {
                                if chars[i] == '\n' {
                                    line += 1;
                                }
                                i += 1;
                            }
                        }
                        'n' | 't' | 'r' | '0' => text.push(' '),
                        other => text.push(other),
                    }
                }
                i += 1;
                literals.push(Literal {
                    written,
                    text,
                    line: start_line,
                });
            }
            _ => i += 1,
        }
    }
    literals
}

#[test]
fn every_ui_string_has_glyphs_in_the_bundled_plex_faces() {
    let faces = plex_condensed_faces();
    let covered = |c: char| {
        faces
            .iter()
            .all(|face| face.rustybuzz().glyph_index(c).is_some())
    };
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut sources = Vec::new();
    production_sources(&src, &mut sources);
    sources.sort();

    let mut uncovered = Vec::new();
    for path in sources {
        let relative = path
            .strip_prefix(&src)
            .expect("found under src")
            .to_string_lossy()
            .replace('\\', "/");
        let source = std::fs::read_to_string(&path).expect("source reads");
        for literal in string_literals(&source) {
            let missing: String = literal
                .text
                .chars()
                .filter(|c| !c.is_ascii() && !covered(*c))
                .collect();
            if !missing.is_empty()
                && !DRAWN_IN_THE_DEFAULT_FONT
                    .contains(&(relative.as_str(), literal.written.as_str()))
            {
                uncovered.push(format!(
                    "{relative}:{}: \"{}\" lacks {missing:?}",
                    literal.line, literal.written
                ));
            }
        }
    }

    assert!(
        uncovered.is_empty(),
        "draw these symbols with the icon font, or list a literal drawn in the \
         default font in DRAWN_IN_THE_DEFAULT_FONT:\n{}",
        uncovered.join("\n")
    );
}
