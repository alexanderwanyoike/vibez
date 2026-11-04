//! Fitting a clip's name into its title strip.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use iced::alignment::Vertical;
use iced::widget::canvas;
use iced::{Color, Font, Point, Rectangle};

use crate::icons;
use crate::widgets::device_controls::text_width;

use super::CLIP_TITLE_HEIGHT;

const TITLE_SIZE: f32 = 11.0;
const TITLE_FONT: Font = crate::typography::CLIP_TITLE;
/// Space between the title text and either end of the strip.
const TITLE_PADDING: f32 = 6.0;
const LOOP_ICON_SIZE: f32 = 10.0;
/// Width of the loop icon plus its gap before the name.
const LOOP_ICON_ROOM: f32 = 15.0;
/// A looped clip's strip narrower than this shows no loop icon.
const LOOP_ICON_MIN_WIDTH: f32 = 60.0;
/// Fewer letters than this before an ellipsis say nothing, so the strip
/// stays empty instead.
const MIN_KEPT_CHARS: usize = 3;
const ELLIPSIS: char = '\u{2026}';

fn name_width(text: &str) -> f32 {
    text_width(text, TITLE_SIZE, TITLE_FONT)
}

/// Measured widths of a name, whole and shortened to each length.
struct NameWidths {
    whole: f32,
    /// `shortened[n]` is the width of its first `n` chars plus an ellipsis.
    shortened: Vec<f32>,
}

thread_local! {
    /// Shaping text is far too slow to repeat for every clip on every
    /// frame, and a song repeats a few names many times.
    static NAME_WIDTHS: RefCell<HashMap<String, Rc<NameWidths>>> = RefCell::new(HashMap::new());
}

fn name_widths(name: &str) -> Rc<NameWidths> {
    NAME_WIDTHS.with(|cache| {
        let mut cache = cache.borrow_mut();
        if let Some(widths) = cache.get(name) {
            return Rc::clone(widths);
        }
        if cache.len() >= 1024 {
            cache.clear();
        }
        let widths = Rc::new(NameWidths {
            whole: name_width(name),
            shortened: name
                .char_indices()
                .map(|(end, _)| name_width(&format!("{}{ELLIPSIS}", &name[..end])))
                .collect(),
        });
        cache.insert(name.to_owned(), Rc::clone(&widths));
        widths
    })
}

/// What of `name` fits a title strip `strip_width` wide.
fn fit_clip_title(name: &str, strip_width: f32, loop_icon_visible: bool) -> Option<String> {
    let room = strip_width
        - 2.0 * TITLE_PADDING
        - if loop_icon_visible {
            LOOP_ICON_ROOM
        } else {
            0.0
        };
    let widths = name_widths(name);
    if widths.whole <= room {
        return Some(name.to_owned());
    }
    let kept = widths.shortened.iter().rposition(|width| *width <= room)?;
    (kept >= MIN_KEPT_CHARS).then(|| {
        let mut title: String = name.chars().take(kept).collect();
        title.push(ELLIPSIS);
        title
    })
}

/// Where a clip title is clipped to, and where its text starts.
#[derive(Debug, PartialEq)]
struct ClipTitleLayout {
    region: Rectangle,
    /// Relative to `region`, because `Frame::with_clip` translates drawing
    /// to the region's origin.
    text: iced::Point,
}

fn clip_title_layout(
    clip_x: f32,
    clip_y: f32,
    title_width: f32,
    viewport_width: f32,
) -> Option<ClipTitleLayout> {
    let left = clip_x.max(0.0);
    let right = (clip_x + title_width).min(viewport_width);
    (right > left).then(|| ClipTitleLayout {
        region: Rectangle {
            x: left,
            y: clip_y,
            width: right - left,
            height: CLIP_TITLE_HEIGHT,
        },
        text: Point::new(TITLE_PADDING, CLIP_TITLE_HEIGHT / 2.0),
    })
}

pub(super) fn draw_clip_title(
    frame: &mut canvas::Frame,
    name: &str,
    body: Rectangle,
    looped: bool,
    color: Color,
    viewport_width: f32,
) {
    let Some(layout) = clip_title_layout(body.x, body.y, body.width, viewport_width) else {
        return;
    };
    let loop_icon = looped && layout.region.width > LOOP_ICON_MIN_WIDTH;
    let title = fit_clip_title(name, layout.region.width, loop_icon);
    if !loop_icon && title.is_none() {
        return;
    }
    frame.with_clip(layout.region, |title_frame| {
        let mut position = layout.text;
        if loop_icon {
            title_frame.fill_text(canvas::Text {
                content: icons::REPEAT.to_string(),
                position,
                color,
                size: LOOP_ICON_SIZE.into(),
                font: icons::ICON_FONT,
                vertical_alignment: Vertical::Center,
                ..Default::default()
            });
            position.x += LOOP_ICON_ROOM;
        }
        if let Some(title) = title {
            title_frame.fill_text(canvas::Text {
                content: title,
                position,
                color,
                size: TITLE_SIZE.into(),
                font: TITLE_FONT,
                vertical_alignment: Vertical::Center,
                ..Default::default()
            });
        }
    });
}

#[cfg(test)]
mod tests {
    use super::{clip_title_layout, fit_clip_title, name_width, LOOP_ICON_ROOM, TITLE_PADDING};

    const NAME: &str = "Capture · Pattern 1";

    #[test]
    fn a_title_wide_enough_for_the_measured_name_shows_all_of_it() {
        let width = name_width(NAME) + 2.0 * TITLE_PADDING;

        assert_eq!(fit_clip_title(NAME, width, false), Some(NAME.into()));
    }

    #[test]
    fn the_loop_icon_takes_its_room_once() {
        let width = name_width(NAME) + 2.0 * TITLE_PADDING + LOOP_ICON_ROOM;

        assert_eq!(fit_clip_title(NAME, width, true), Some(NAME.into()));
    }

    #[test]
    fn a_name_too_long_for_its_strip_keeps_as_much_as_fits_before_an_ellipsis() {
        let room = name_width(NAME) / 2.0;

        let title = fit_clip_title(NAME, room + 2.0 * TITLE_PADDING, false).unwrap();

        let kept = title.strip_suffix('…').expect("ends in an ellipsis");
        assert!(name_width(&title) <= room, "{title}");
        let one_more: String = NAME.chars().take(kept.chars().count() + 1).collect();
        assert!(name_width(&format!("{one_more}…")) > room, "{title}");
    }

    #[test]
    fn no_title_when_only_a_couple_of_letters_would_fit() {
        let width = name_width("Ca…") + 2.0 * TITLE_PADDING;

        assert_eq!(fit_clip_title(NAME, width, false), None);
    }

    #[test]
    fn clip_title_bounds_never_escape_the_arrangement_viewport() {
        let region = |clip_x| clip_title_layout(clip_x, 4.0, 400.0, 800.0).map(|l| l.region);
        assert_eq!(region(-200.0).map(|r| (r.x, r.width)), Some((0.0, 200.0)));
        assert_eq!(region(600.0).map(|r| (r.x, r.width)), Some((600.0, 200.0)));
        assert_eq!(region(900.0), None);
    }

    #[test]
    fn clip_title_text_starts_at_the_left_of_its_region_at_any_scroll() {
        let unscrolled = clip_title_layout(0.0, 4.0, 400.0, 800.0).unwrap().text;
        for clip_x in [-200.0, 100.0, 600.0] {
            let layout = clip_title_layout(clip_x, 4.0, 400.0, 800.0).unwrap();
            assert_eq!(layout.text, unscrolled, "clip at x {clip_x}");
        }
    }
}
