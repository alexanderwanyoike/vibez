//! Labelled cells in the header column beside the Arrange ruler and minimap.

use iced::widget::{container, horizontal_space, row, text};
use iced::{Element, Length, Theme};
use vibez_core::time::BEATS_PER_BAR;

use crate::message::Message;
use crate::theme as th;
use crate::widgets::track_header::TRACK_HEADER_TOTAL_WIDTH;

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
    format!(
        "{}, {}:{:02}",
        counted(bars as usize, "bar"),
        whole_seconds / 60,
        whole_seconds % 60
    )
}

/// The track and clip counts beside the ruler.
pub fn arrangement_size_labels(track_count: usize, clip_count: usize) -> (String, String) {
    (counted(track_count, "track"), counted(clip_count, "clip"))
}

fn counted(count: usize, unit: &str) -> String {
    format!("{count} {unit}{}", if count == 1 { "" } else { "s" })
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
    fn track_and_clip_counts_read_singular_for_one() {
        assert_eq!(
            arrangement_size_labels(1, 1),
            ("1 track".into(), "1 clip".into())
        );
        assert_eq!(
            arrangement_size_labels(8, 0),
            ("8 tracks".into(), "0 clips".into())
        );
    }

    #[test]
    fn an_empty_song_has_no_length() {
        assert_eq!(song_length_label(0.0, 120.0), "Empty");
    }
}
