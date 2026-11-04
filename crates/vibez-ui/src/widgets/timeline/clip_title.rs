//! Fitting a clip's name into its title strip.

use iced::widget::canvas;
use iced::{Color, Rectangle};

use super::CLIP_TITLE_HEIGHT;

fn fit_clip_title(name: &str, clip_width: f32, loop_icon_visible: bool) -> Option<String> {
    if clip_width <= 40.0 {
        return None;
    }
    const HORIZONTAL_PADDING: f32 = 8.0;
    const LOOP_ICON_WIDTH: f32 = 18.0;
    const APPROX_GLYPH_WIDTH: f32 = 6.5;
    let reserved = HORIZONTAL_PADDING
        + if loop_icon_visible {
            LOOP_ICON_WIDTH
        } else {
            0.0
        };
    let max_chars = ((clip_width - reserved).max(0.0) / APPROX_GLYPH_WIDTH).floor() as usize;
    if max_chars < 4 {
        return None;
    }
    if name.chars().count() <= max_chars {
        Some(name.to_string())
    } else {
        let prefix: String = name.chars().take(max_chars - 2).collect();
        Some(format!("{prefix}.."))
    }
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
        text: iced::Point::new(4.0, 3.0),
    })
}

pub(super) fn draw_clip_title(
    frame: &mut canvas::Frame,
    name: &str,
    body: Rectangle,
    loop_enabled: bool,
    color: Color,
    viewport_width: f32,
) {
    let title_width = (body.width - if loop_enabled { 18.0 } else { 0.0 }).max(0.0);
    let Some(layout) = clip_title_layout(body.x, body.y, title_width, viewport_width) else {
        return;
    };
    let Some(title) = fit_clip_title(name, layout.region.width, loop_enabled) else {
        return;
    };
    frame.with_clip(layout.region, |title_frame| {
        title_frame.fill_text(canvas::Text {
            content: title,
            position: layout.text,
            color,
            size: iced::Pixels(11.0),
            ..Default::default()
        });
    });
}

#[cfg(test)]
mod tests {
    use super::{clip_title_layout, fit_clip_title};

    #[test]
    fn clip_title_is_constrained_to_its_visible_width() {
        assert_eq!(
            fit_clip_title("Kick.wav", 100.0, false),
            Some("Kick.wav".into())
        );
        assert_eq!(
            fit_clip_title("OTH_128_Hub_Full.wav", 80.0, false),
            Some("OTH_128_H..".into())
        );
        assert_eq!(fit_clip_title("Kick.wav", 40.0, false), None);
    }

    #[test]
    fn loop_icon_reserves_title_space() {
        assert_eq!(
            fit_clip_title("OTH_128_Hub_Full.wav", 80.0, true),
            Some("OTH_12..".into())
        );
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
        for clip_x in [-200.0, 0.0, 100.0, 600.0] {
            let layout = clip_title_layout(clip_x, 4.0, 400.0, 800.0).unwrap();
            assert_eq!(
                layout.text,
                iced::Point::new(4.0, 3.0),
                "clip at x {clip_x}"
            );
        }
    }
}
