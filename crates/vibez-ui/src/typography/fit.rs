//! Shortening text with an ellipsis to fit a width, measured in the font it
//! is drawn in.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use iced::Font;

use crate::widgets::device_controls::text_width;

const ELLIPSIS: char = '\u{2026}';

/// Measured widths of a text, whole and shortened to each length.
struct Widths {
    whole: f32,
    /// `shortened[n]` is the width of its first `n` chars, without trailing
    /// space, plus an ellipsis.
    shortened: Vec<f32>,
}

type WidthsKey = (String, u32, Font);

thread_local! {
    /// Shaping text is far too slow to repeat for every clip on every
    /// frame, and a song repeats a few names many times.
    static WIDTHS: RefCell<HashMap<WidthsKey, Rc<Widths>>> = RefCell::new(HashMap::new());

    /// Browser rows fit their names again on every frame.
    static MIDDLE_FITS: RefCell<HashMap<(String, u32, Font, u32), String>> =
        RefCell::new(HashMap::new());
}

fn widths(text: &str, size: f32, font: Font) -> Rc<Widths> {
    WIDTHS.with(|cache| {
        let mut cache = cache.borrow_mut();
        let key = (text.to_owned(), size.to_bits(), font);
        if let Some(widths) = cache.get(&key) {
            return Rc::clone(widths);
        }
        if cache.len() >= 1024 {
            cache.clear();
        }
        let measure = |content: &str| text_width(content, size, font);
        let widths = Rc::new(Widths {
            whole: measure(text),
            shortened: text
                .char_indices()
                .map(|(end, _)| measure(&shortened(text, end)))
                .collect(),
        });
        cache.insert(key, Rc::clone(&widths));
        widths
    })
}

/// What of `text`, set at `size` in `font`, fits `width`: all of it, or as
/// many leading chars as fit before an ellipsis. `None` when fewer than
/// `min_kept` chars would fit.
pub fn fit_text(text: &str, width: f32, size: f32, font: Font, min_kept: usize) -> Option<String> {
    let widths = widths(text, size, font);
    if widths.whole <= width {
        return Some(text.to_owned());
    }
    let kept = widths
        .shortened
        .iter()
        .rposition(|cut_width| *cut_width <= width)?;
    (kept >= min_kept).then(|| {
        let end = text
            .char_indices()
            .nth(kept)
            .map_or(text.len(), |(end, _)| end);
        shortened(text, end)
    })
}

/// What of `text`, set at `size` in `font`, fits `width`: all of it, or its
/// start and its end around an ellipsis, so names that share a long prefix
/// stay told apart.
pub fn fit_text_middle(text: &str, width: f32, size: f32, font: Font) -> String {
    let key = (text.to_owned(), size.to_bits(), font, width.to_bits());
    if let Some(fitted) = MIDDLE_FITS.with(|cache| cache.borrow().get(&key).cloned()) {
        return fitted;
    }
    let fitted = if text_width(text, size, font) <= width {
        text.to_owned()
    } else {
        let chars: Vec<char> = text.chars().collect();
        let fits = |kept: usize| text_width(&middle_shortened(&chars, kept), size, font) <= width;
        let (mut low, mut high) = (0, chars.len().saturating_sub(1));
        while low < high {
            let kept = (low + high).div_ceil(2);
            if fits(kept) {
                low = kept;
            } else {
                high = kept - 1;
            }
        }
        middle_shortened(&chars, low)
    };
    MIDDLE_FITS.with(|cache| {
        let mut cache = cache.borrow_mut();
        if cache.len() >= 1024 {
            cache.clear();
        }
        cache.insert(key, fitted.clone());
    });
    fitted
}

/// `kept` chars of `chars`, half from its start and half from its end,
/// around an ellipsis.
fn middle_shortened(chars: &[char], kept: usize) -> String {
    let tail = kept / 2;
    let head: String = chars[..kept - tail].iter().collect();
    let end: String = chars[chars.len() - tail..].iter().collect();
    format!("{}{ELLIPSIS}{}", head.trim_end(), end.trim_start())
}

fn shortened(text: &str, end: usize) -> String {
    format!("{}{ELLIPSIS}", text[..end].trim_end())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIZE: f32 = 11.0;

    #[test]
    fn a_cut_at_a_space_drops_the_space_before_the_ellipsis() {
        let width = text_width("Open \u{2026}", SIZE, Font::DEFAULT);

        let fitted = fit_text("Open Hat Long Tail", width, SIZE, Font::DEFAULT, 0);

        assert_eq!(fitted.as_deref(), Some("Open\u{2026}"));
    }

    #[test]
    fn a_long_name_keeps_its_start_and_its_end_around_the_ellipsis() {
        let name = "UNDRGRND Sounds - Deep Tech";
        let width = text_width("UNDRGRND So\u{2026}Deep Tech", SIZE, Font::DEFAULT);

        let fitted = fit_text_middle(name, width, SIZE, Font::DEFAULT);

        assert!(fitted.starts_with("UNDRG"), "{fitted}");
        assert!(fitted.ends_with("Tech"), "{fitted}");
        assert!(fitted.contains('\u{2026}'), "{fitted}");
        assert!(
            text_width(&fitted, SIZE, Font::DEFAULT) <= width,
            "{fitted}"
        );
    }

    #[test]
    fn a_name_that_fits_stays_whole() {
        let name = "Deep Tech";
        let width = text_width(name, SIZE, Font::DEFAULT);

        assert_eq!(fit_text_middle(name, width, SIZE, Font::DEFAULT), name);
    }
}
