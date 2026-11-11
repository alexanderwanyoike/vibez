//! Shortening text with an ellipsis to fit a width, measured in the font it
//! is drawn in.

use std::cell::RefCell;
use std::collections::HashMap;
use std::hash::Hash;
use std::rc::Rc;

use iced::Font;

use crate::widgets::device_controls::text_width;

const ELLIPSIS: char = '\u{2026}';
/// Fewer kept letters than this read better as one run from the start
/// than as two fragments.
const MIN_SPLIT_KEPT: usize = 5;

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

/// Each cache starts over at this size, so names from closed projects and
/// left folders do not pile up.
const CACHE_LIMIT: usize = 1024;

fn remember<K: Eq + Hash, V>(cache: &mut HashMap<K, V>, key: K, value: V) {
    if cache.len() >= CACHE_LIMIT {
        cache.clear();
    }
    cache.insert(key, value);
}

fn widths(text: &str, size: f32, font: Font) -> Rc<Widths> {
    WIDTHS.with(|cache| {
        let mut cache = cache.borrow_mut();
        let key = (text.to_owned(), size.to_bits(), font);
        if let Some(widths) = cache.get(&key) {
            return Rc::clone(widths);
        }
        let measure = |content: &str| text_width(content, size, font);
        let widths = Rc::new(Widths {
            whole: measure(text),
            shortened: text
                .char_indices()
                .map(|(end, _)| measure(&shortened(text, end)))
                .collect(),
        });
        remember(&mut cache, key, Rc::clone(&widths));
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
    MIDDLE_FITS.with(|cache| remember(&mut cache.borrow_mut(), key, fitted.clone()));
    fitted
}

/// `kept` chars of `chars`, a third from its start and the rest from its
/// end, around an ellipsis. Names in one pack or folder tend to share their
/// start and differ at their end.
fn middle_shortened(chars: &[char], kept: usize) -> String {
    let head_len = if kept < MIN_SPLIT_KEPT {
        kept
    } else {
        kept / 3
    };
    let tail = kept - head_len;
    let head: String = chars[..head_len].iter().collect();
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

    #[test]
    fn a_narrow_cut_keeps_more_of_the_end_than_the_start() {
        let name = "UNDRGRND Sounds - Deep Dub";
        let width = text_width("UNDRGR\u{2026}p Dub", SIZE, Font::DEFAULT);

        let fitted = fit_text_middle(name, width, SIZE, Font::DEFAULT);

        let (start, end) = fitted.split_once('\u{2026}').expect("cut");
        assert!(end.chars().count() > start.chars().count(), "{fitted}");
    }

    #[test]
    fn a_cut_too_short_to_split_keeps_only_the_start() {
        let width = text_width("UNDR\u{2026}", SIZE, Font::DEFAULT);

        let fitted = fit_text_middle("UNDRGRND", width, SIZE, Font::DEFAULT);

        assert_eq!(fitted, "UNDR\u{2026}");
    }

    #[test]
    fn a_short_cut_of_a_long_name_still_keeps_its_end() {
        let width = text_width("UN\u{2026}Dub", SIZE, Font::DEFAULT);

        let fitted = fit_text_middle("UNDRGRND Sounds - Deep Dub", width, SIZE, Font::DEFAULT);

        assert!(fitted.ends_with("Dub"), "{fitted}");
    }
}
