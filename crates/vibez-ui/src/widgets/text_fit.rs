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
    /// `shortened[n]` is the width of its first `n` chars plus an ellipsis.
    shortened: Vec<f32>,
}

type WidthsKey = (String, u32, Font);

thread_local! {
    /// Shaping text is far too slow to repeat for every clip on every
    /// frame, and a song repeats a few names many times.
    static WIDTHS: RefCell<HashMap<WidthsKey, Rc<Widths>>> = RefCell::new(HashMap::new());
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
                .map(|(end, _)| measure(&format!("{}{ELLIPSIS}", &text[..end])))
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
        .rposition(|shortened| *shortened <= width)?;
    (kept >= min_kept).then(|| {
        let mut fitted: String = text.chars().take(kept).collect();
        fitted.push(ELLIPSIS);
        fitted
    })
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
}
