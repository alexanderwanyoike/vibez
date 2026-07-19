use iced::widget::{container, horizontal_space, row};
use iced::{Element, Length, Theme};

use crate::message::Message;
use crate::theme as th;

pub(super) fn section_playhead_fraction(
    position_samples: u64,
    length_beats: f64,
    bpm: f64,
    sample_rate: u32,
) -> f32 {
    let length_samples = if bpm > 0.0 {
        (length_beats * f64::from(sample_rate.max(1)) * 60.0 / bpm)
            .round()
            .max(1.0)
    } else {
        1.0
    };
    (position_samples as f64 / length_samples).clamp(0.0, 1.0) as f32
}

pub(super) fn section_playhead_line(height: Length) -> Element<'static, Message> {
    container(horizontal_space())
        .width(Length::Fixed(2.0))
        .height(height)
        .style(|_theme: &Theme| container::Style {
            background: Some(th::playhead().into()),
            shadow: iced::Shadow {
                color: th::with_alpha(th::playhead(), 0.45),
                offset: iced::Vector::ZERO,
                blur_radius: 8.0,
            },
            ..Default::default()
        })
        .into()
}

pub(super) fn pad_playhead(fraction: f32) -> Element<'static, Message> {
    let left = (fraction.clamp(0.0, 1.0) * 1_000.0).round() as u16;
    let right = 1_000_u16.saturating_sub(left);
    container(row![
        horizontal_space().width(Length::FillPortion(left.max(1))),
        section_playhead_line(Length::Fill),
        horizontal_space().width(Length::FillPortion(right.max(1))),
    ])
    .width(Length::Fill)
    .height(Length::Fill)
    .padding(5)
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn section_playhead_fraction_uses_engine_samples_and_section_length() {
        assert_eq!(section_playhead_fraction(0, 4.0, 120.0, 48_000), 0.0);
        assert!((section_playhead_fraction(48_000, 4.0, 120.0, 48_000) - 0.5).abs() < 0.001);
        assert_eq!(section_playhead_fraction(120_000, 4.0, 120.0, 48_000), 1.0);
    }
}
