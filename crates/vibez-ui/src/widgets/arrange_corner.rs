//! Labelled cells in the header column beside the Arrange ruler and minimap.

use iced::widget::{container, horizontal_space, row, text};
use iced::{Element, Length, Theme};

use crate::message::Message;
use crate::theme as th;
use crate::widgets::track_header::TRACK_HEADER_TOTAL_WIDTH;

const BEATS_PER_BAR: f64 = 4.0;

/// A header-column cell with a title on the left and a detail on the right.
pub fn arrange_corner<'a>(title: String, detail: String, height: f32) -> Element<'a, Message> {
    container(
        row![
            text(title).size(11).color(th::text()),
            horizontal_space(),
            text(detail).size(11).color(th::text_dim()),
        ]
        .align_y(iced::Alignment::Center),
    )
    .padding([0, 12])
    .width(Length::Fixed(TRACK_HEADER_TOTAL_WIDTH))
    .height(Length::Fixed(height))
    .align_y(iced::alignment::Vertical::Center)
    .style(|_theme: &Theme| container::Style {
        background: Some(th::bg_surface().into()),
        ..Default::default()
    })
    .into()
}

/// Song length beside the minimap, in bars and minutes.
pub fn song_length_label(seconds: f64, bpm: f64) -> String {
    if seconds <= 0.0 || bpm <= 0.0 {
        return "Empty".into();
    }
    let bars = (seconds * bpm / 60.0 / BEATS_PER_BAR).ceil() as u64;
    let whole_seconds = seconds.round() as u64;
    let unit = if bars == 1 { "bar" } else { "bars" };
    format!(
        "{bars} {unit}, {}:{:02}",
        whole_seconds / 60,
        whole_seconds % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_song_length_reads_in_whole_bars_and_minutes() {
        assert_eq!(song_length_label(140.0, 120.0), "70 bars, 2:20");
        assert_eq!(song_length_label(61.0, 120.0), "31 bars, 1:01");
        assert_eq!(song_length_label(2.0, 120.0), "1 bar, 0:02");
    }

    #[test]
    fn an_empty_song_has_no_length() {
        assert_eq!(song_length_label(0.0, 120.0), "Empty");
    }
}
