//! The soft tint that marks where a Browser drop will land.

use iced::widget::{container, Space};
use iced::{Element, Length, Padding, Theme};

use crate::theme as th;
use crate::widgets::flat_stack::flat_stack;

const OUTLINE_W: f32 = 1.0;

/// `content` under a faint accent wash and hairline while `active`, rounded
/// to `radius` and held `inset` inside the content's edges so it never
/// covers chrome such as a scrollbar. The tint layer stays when inactive, so
/// a hover never rebuilds the content's widget state, and it sits on top so
/// cards cannot paint over it.
pub fn drop_highlight<'a, Message: 'a>(
    content: Element<'a, Message>,
    active: bool,
    radius: f32,
    inset: impl Into<Padding>,
) -> Element<'a, Message> {
    let size = content.as_widget().size_hint();
    let tint = container(Space::new(Length::Fill, Length::Fill))
        .width(Length::Fill)
        .height(Length::Fill)
        .style(move |_theme: &Theme| {
            if !active {
                return container::Style::default();
            }
            container::Style {
                background: Some(th::drop_target_wash().into()),
                border: iced::Border {
                    color: th::drop_target_outline(),
                    width: OUTLINE_W,
                    radius: radius.into(),
                },
                ..Default::default()
            }
        });
    let tint = container(tint)
        .width(Length::Fill)
        .height(Length::Fill)
        .padding(inset);
    flat_stack([content, tint.into()])
        .width(size.width)
        .height(size.height)
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::Size;

    #[test]
    fn a_rounded_outline_is_its_sides_plus_one_circle_of_corners() {
        let perimeter = rounded_perimeter(Size::new(100.0, 40.0), 8.0);
        let expected = 2.0 * (100.0 - 16.0 + 40.0 - 16.0) + std::f32::consts::TAU * 8.0;
        assert!((perimeter - expected).abs() < 1e-3);
    }

    #[test]
    fn a_corner_never_rounds_past_half_the_short_side() {
        let pill = rounded_perimeter(Size::new(100.0, 10.0), 8.0);
        let expected = 2.0 * (100.0 - 10.0) + std::f32::consts::TAU * 5.0;
        assert!((pill - expected).abs() < 1e-3);
    }

    #[test]
    fn dashes_stretch_to_close_the_outline_without_a_stub() {
        let [dash, gap] = even_dash(103.0, 6.0, 4.0);
        let periods: f32 = 103.0 / (dash + gap);
        assert!((periods - periods.round()).abs() < 1e-4);
        assert_eq!(periods.round(), 10.0);
        assert!((dash / gap - 1.5).abs() < 1e-4);
    }

    #[test]
    fn an_outline_shorter_than_one_dash_still_gets_one() {
        let [dash, gap] = even_dash(4.0, 6.0, 4.0);
        assert!((dash + gap - 4.0).abs() < 1e-4);
    }

    #[test]
    fn the_label_shows_only_with_room_around_it() {
        let label = Size::new(120.0, 24.0);
        assert!(label_fits(Size::new(200.0, 60.0), label));
        assert!(!label_fits(Size::new(124.0, 60.0), label));
        assert!(!label_fits(Size::new(200.0, 26.0), label));
    }
}
