//! Colours of a timeline clip's parts, derived from its track colour.

use iced::Color;

use crate::theme::{self, ClipStyle};

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct ClipColors {
    pub title: Color,
    pub body: Color,
    /// Notes and waveform.
    pub ink: Color,
    pub text: Color,
    pub loop_tick: Color,
    pub selection: Option<Color>,
}

/// How far a selected clip's title moves toward white.
const SELECTED_TITLE_LIFT: f32 = 0.33;

/// Opacity left to a clip on a muted track. Lower than it looks: the
/// renderer blends in linear light, which makes translucency read stronger.
const MUTED_OPACITY: f32 = 0.25;

impl ClipColors {
    /// The same clip on a muted track: it stays in place and readable but
    /// steps back, while a selection outline keeps its full strength.
    pub fn muted(self) -> Self {
        let fade = |color: Color| theme::with_alpha(color, color.a * MUTED_OPACITY);
        Self {
            title: fade(self.title),
            body: fade(self.body),
            ink: fade(self.ink),
            text: fade(self.text),
            loop_tick: fade(self.loop_tick),
            selection: self.selection,
        }
    }
}

/// The colours of a clip on `track`'s lane in the active theme.
pub(super) fn clip_colors(track: Color, selected: bool) -> ClipColors {
    shade(
        track,
        selected,
        theme::clip_style(),
        theme::track_bg(),
        theme::text(),
    )
}

fn shade(track: Color, selected: bool, style: ClipStyle, lane: Color, text: Color) -> ClipColors {
    let track = theme::with_alpha(track, 1.0);
    let title = if selected {
        theme::blend(track, Color::WHITE, SELECTED_TITLE_LIFT)
    } else {
        track
    };
    let body = theme::blend(track, lane, style.body_toward_lane);
    ClipColors {
        title,
        body: theme::with_alpha(body, style.body_opacity),
        ink: style.ink,
        text: theme::with_alpha(style.ink, 1.0),
        loop_tick: theme::with_alpha(style.ink, style.ink.a * 0.2),
        selection: selected.then(|| theme::with_alpha(text, 0.9)),
    }
}

#[cfg(test)]
mod tests {
    use super::shade;
    use crate::theme::ClipStyle;
    use iced::Color;

    const RED: Color = Color::from_rgb(0.8, 0.4, 0.4);
    const DARK_LANE: Color = Color::from_rgb(0.1, 0.1, 0.1);
    const LIGHT_LANE: Color = Color::from_rgb(0.95, 0.95, 0.94);
    const TEXT: Color = Color::from_rgb(0.9, 0.9, 0.9);

    fn luma(c: Color) -> f32 {
        0.2126 * c.r + 0.7152 * c.g + 0.0722 * c.b
    }

    #[test]
    fn a_clip_has_an_opaque_title_in_its_track_colour_and_a_body_at_the_theme_opacity() {
        let style = ClipStyle {
            body_opacity: 0.7,
            ..ClipStyle::DARK
        };

        let colors = shade(RED, false, style, DARK_LANE, TEXT);

        assert_eq!(colors.title, RED);
        assert_eq!(colors.body.a, 0.7);
    }

    #[test]
    fn the_body_leans_toward_the_lane_behind_it() {
        let on_dark = shade(RED, false, ClipStyle::DARK, DARK_LANE, TEXT).body;
        let on_light = shade(RED, false, ClipStyle::LIGHT, LIGHT_LANE, TEXT).body;

        assert!(luma(on_dark) < luma(RED), "{on_dark:?}");
        assert!(luma(on_light) > luma(RED), "{on_light:?}");
        assert!(on_dark.r > on_dark.g, "keeps the track hue: {on_dark:?}");
    }

    #[test]
    fn notes_waveform_and_name_are_drawn_in_the_theme_ink() {
        let colors = shade(RED, false, ClipStyle::DARK, DARK_LANE, TEXT);

        assert_eq!(colors.ink, ClipStyle::DARK.ink);
        assert_eq!(
            colors.text,
            Color {
                a: 1.0,
                ..ClipStyle::DARK.ink
            }
        );
        assert!(colors.loop_tick.a > 0.0 && colors.loop_tick.a < colors.ink.a);
    }

    #[test]
    fn a_clip_on_a_muted_track_fades_everything_but_its_selection_outline() {
        let live = shade(RED, true, ClipStyle::DARK, DARK_LANE, TEXT);

        let muted = live.muted();

        for (part, before, after) in [
            ("title", live.title, muted.title),
            ("body", live.body, muted.body),
            ("ink", live.ink, muted.ink),
            ("text", live.text, muted.text),
            ("loop tick", live.loop_tick, muted.loop_tick),
        ] {
            assert!(after.a < before.a * 0.6, "{part}: {after:?}");
            assert_eq!((after.r, after.g, after.b), (before.r, before.g, before.b));
        }
        assert_eq!(muted.selection, live.selection);
    }

    #[test]
    fn selecting_a_clip_lightens_its_title_and_outlines_it() {
        let idle = shade(RED, false, ClipStyle::DARK, DARK_LANE, TEXT);
        let selected = shade(RED, true, ClipStyle::DARK, DARK_LANE, TEXT);

        assert!(luma(selected.title) > luma(idle.title));
        assert_eq!(selected.title.a, 1.0);
        assert_eq!(idle.selection, None);
        assert_eq!(
            selected.selection.map(|c| (c.r, c.g, c.b)),
            Some((0.9, 0.9, 0.9))
        );
    }
}
