//! Every string literal in the UI must be drawable by the bundled Plex Sans
//! Condensed faces. A character they lack comes from whichever system face
//! has it at that weight, which differs per platform and can be a symbol
//! font (Windows once drew Marlett's window buttons for digits), so symbols
//! go through the icon font instead.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use proc_macro2::{TokenStream, TokenTree};
use syn::visit::Visit;

use iced::advanced::graphics::text::cosmic_text;

use super::{draws, loaded_faces, PLEX_SANS_CONDENSED_FAMILY};

/// Literals drawn in the default font, which falls back across the system's
/// faces, so they may hold characters Plex lacks. Each is a path under
/// `src` and the text the literal holds.
const DRAWN_IN_THE_DEFAULT_FONT: &[(&str, &str)] = &[
    ("widgets/device_controls.rs", "\u{25BE}"),
    ("widgets/device_controls.rs", "\u{25B8}"),
    ("widgets/gain_device.rs", "\u{25CF} Clip"),
    ("app/views_arrangement.rs", "\u{25CF}"),
    ("app/views_clip_launcher.rs", "\u{25A0}"),
];

/// The bundled Plex Sans Condensed faces, one per weight.
fn plex_condensed_faces() -> Vec<Arc<cosmic_text::Font>> {
    let faces = loaded_faces(|face| {
        face.families
            .iter()
            .any(|(name, _)| name == PLEX_SANS_CONDENSED_FAMILY)
    });
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

fn is_test_only(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| {
        attr.path().is_ident("test")
            || (attr.path().is_ident("cfg")
                && attr
                    .parse_args::<syn::Ident>()
                    .is_ok_and(|flag| flag == "test"))
    })
}

/// The text of every string literal outside test code and attributes,
/// including those inside macro calls such as `format!` and `row!`.
#[derive(Default)]
struct StringLiterals(Vec<String>);

impl StringLiterals {
    fn tokens(&mut self, stream: TokenStream) {
        for tree in stream {
            match tree {
                TokenTree::Group(group) => self.tokens(group.stream()),
                TokenTree::Literal(literal) => {
                    if let syn::Lit::Str(text) = syn::Lit::new(literal) {
                        self.0.push(text.value());
                    }
                }
                TokenTree::Ident(_) | TokenTree::Punct(_) => {}
            }
        }
    }
}

impl<'ast> Visit<'ast> for StringLiterals {
    fn visit_attribute(&mut self, _: &'ast syn::Attribute) {}

    fn visit_item_mod(&mut self, item: &'ast syn::ItemMod) {
        if !is_test_only(&item.attrs) {
            syn::visit::visit_item_mod(self, item);
        }
    }

    fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
        if !is_test_only(&item.attrs) {
            syn::visit::visit_item_fn(self, item);
        }
    }

    fn visit_item_impl(&mut self, item: &'ast syn::ItemImpl) {
        if !is_test_only(&item.attrs) {
            syn::visit::visit_item_impl(self, item);
        }
    }

    fn visit_lit_str(&mut self, literal: &'ast syn::LitStr) {
        self.0.push(literal.value());
    }

    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        self.tokens(mac.tokens.clone());
    }
}

#[test]
fn every_ui_string_has_glyphs_in_the_bundled_plex_faces() {
    let faces = plex_condensed_faces();
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
        let file = syn::parse_file(&source).expect("source parses");
        let mut literals = StringLiterals::default();
        literals.visit_file(&file);
        for text in literals.0 {
            let missing: String = text
                .chars()
                .filter(|c| !c.is_ascii() && !faces.iter().all(|face| draws(face, *c)))
                .collect();
            if !missing.is_empty()
                && !DRAWN_IN_THE_DEFAULT_FONT.contains(&(relative.as_str(), text.as_str()))
            {
                uncovered.push(format!("{relative}: {text:?} lacks {missing:?}"));
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
