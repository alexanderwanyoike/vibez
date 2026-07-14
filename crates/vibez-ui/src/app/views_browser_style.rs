//! Shared styling and formatting helpers for the sample browser views.
//! Split from views_browser.rs.

use std::path::Path;

use iced::widget::{button, container, text};
use iced::{Element, Length, Theme};

use crate::message::Message;
use crate::state::SampleBrowserEntry;
use crate::theme as th;

pub(super) const REMOTE_CONNECTION_INDENT: f32 = 14.0;
pub(super) const BROWSER_TREE_INDENT_STEP: f32 = 8.0;
pub(super) const BROWSER_TREE_MAX_DEPTH: f32 = 5.0;

pub(super) fn remote_places_indent(depth: usize) -> f32 {
    REMOTE_CONNECTION_INDENT
        + ((depth as f32 + 1.0).min(BROWSER_TREE_MAX_DEPTH) * BROWSER_TREE_INDENT_STEP)
}

pub(super) fn browser_root_name(root: &Path) -> String {
    root.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| root.display().to_string())
}

pub(super) fn browser_folder_context(
    root: &Path,
    relative_path: &Path,
    detail: &str,
    file_size: Option<u64>,
) -> String {
    let size = file_size
        .map(format_browser_file_size)
        .map(|size| format!(" · {size}"))
        .unwrap_or_default();
    format!(
        "{detail}{size} · {}/{}",
        browser_root_name(root),
        relative_path.display()
    )
}

pub(super) fn browser_entry_metadata(entry: &SampleBrowserEntry) -> String {
    let channels = entry.channels.map(|channels| match channels {
        1 => "MONO".into(),
        2 => "STEREO".into(),
        channels => format!("{channels} CH"),
    });
    let sample_rate = entry.sample_rate.map(|sample_rate| {
        if sample_rate % 1_000 == 0 {
            format!("{} KHZ", sample_rate / 1_000)
        } else {
            format!("{:.1} KHZ", sample_rate as f64 / 1_000.0)
        }
    });
    std::iter::once(entry.format.clone())
        .chain(channels)
        .chain(sample_rate)
        .collect::<Vec<_>>()
        .join(" · ")
}

pub(super) fn format_browser_duration(seconds: f64) -> String {
    if seconds >= 60.0 {
        let total_seconds = seconds.round() as u64;
        format!("{}:{:02}", total_seconds / 60, total_seconds % 60)
    } else {
        format!("{seconds:.1}s")
    }
}

pub(super) fn format_browser_file_size(bytes: u64) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = KIB * 1024.0;
    if bytes < 1024 {
        format!("{bytes} B")
    } else if (bytes as f64) < MIB {
        format!("{:.1} KB", bytes as f64 / KIB)
    } else {
        format!("{:.1} MB", bytes as f64 / MIB)
    }
}

pub(super) fn browser_icon_button_style(_theme: &Theme, status: button::Status) -> button::Style {
    let background = match status {
        button::Status::Hovered | button::Status::Pressed => Some(th::bg_hover().into()),
        _ => None,
    };
    button::Style {
        background,
        text_color: th::text_dim(),
        border: iced::Border {
            color: if matches!(status, button::Status::Pressed) {
                th::accent()
            } else {
                th::border()
            },
            width: 1.0,
            radius: 0.0.into(),
        },
        ..Default::default()
    }
}

pub(super) fn browser_row_divider<'a>() -> Element<'a, Message> {
    container(text(""))
        .width(Length::Fill)
        .height(Length::Fixed(1.0))
        .style(|_theme: &Theme| container::Style {
            background: Some(th::divider().into()),
            ..Default::default()
        })
        .into()
}

pub(super) fn audition_gain_label(gain: f32) -> String {
    if gain <= 0.0001 {
        "−∞ dB".into()
    } else {
        format!("{:+.1} dB", 20.0 * gain.log10())
    }
}

pub(super) fn browser_place_button_style(active: bool, status: button::Status) -> button::Style {
    button::Style {
        background: Some(
            if active {
                th::accent_dim()
            } else if matches!(status, button::Status::Hovered | button::Status::Pressed) {
                th::bg_hover()
            } else {
                iced::Color::TRANSPARENT
            }
            .into(),
        ),
        text_color: if active { th::text() } else { th::text_dim() },
        border: iced::Border {
            color: if active {
                th::accent()
            } else {
                iced::Color::TRANSPARENT
            },
            width: if active { 1.0 } else { 0.0 },
            radius: 0.0.into(),
        },
        ..Default::default()
    }
}

pub(super) fn browser_utility_action_style(
    _theme: &Theme,
    status: button::Status,
) -> button::Style {
    button::Style {
        background: matches!(status, button::Status::Hovered | button::Status::Pressed)
            .then(|| th::bg_hover().into()),
        text_color: if matches!(status, button::Status::Pressed) {
            th::accent()
        } else {
            th::text_dim()
        },
        border: iced::Border {
            color: iced::Color::TRANSPARENT,
            width: 0.0,
            radius: 0.0.into(),
        },
        ..Default::default()
    }
}

pub(super) fn browser_compact_input_style(
    _theme: &Theme,
    _status: iced::widget::text_input::Status,
) -> iced::widget::text_input::Style {
    iced::widget::text_input::Style {
        background: th::bg_dark().into(),
        border: iced::Border {
            color: th::border(),
            width: 1.0,
            radius: 0.0.into(),
        },
        icon: th::text_dim(),
        placeholder: th::text_dim(),
        value: th::text(),
        selection: th::accent(),
    }
}

pub(super) fn browser_transport_button_style(
    _theme: &Theme,
    status: button::Status,
) -> button::Style {
    button::Style {
        background: Some(
            if matches!(status, button::Status::Hovered | button::Status::Pressed) {
                th::bg_hover()
            } else {
                th::bg_elevated()
            }
            .into(),
        ),
        text_color: th::text_dim(),
        border: iced::Border {
            color: if matches!(status, button::Status::Pressed) {
                th::accent()
            } else {
                th::border()
            },
            width: 1.0,
            radius: 0.0.into(),
        },
        ..Default::default()
    }
}

pub(super) fn browser_header_style(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(th::bg_surface().into()),
        border: iced::Border {
            color: th::divider(),
            width: 1.0,
            radius: 0.0.into(),
        },
        ..Default::default()
    }
}

pub(super) fn browser_places_style(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(th::bg_dark().into()),
        border: iced::Border {
            color: th::divider(),
            width: 1.0,
            radius: 0.0.into(),
        },
        ..Default::default()
    }
}

pub(super) fn browser_results_style(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(th::bg_surface().into()),
        border: iced::Border {
            color: th::divider(),
            width: 1.0,
            radius: 0.0.into(),
        },
        ..Default::default()
    }
}

pub(super) fn browser_table_header_style(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(th::bg_dark().into()),
        ..Default::default()
    }
}

pub(super) fn browser_result_cell_color(selected: bool) -> iced::Color {
    if selected {
        th::text()
    } else {
        th::text_dim()
    }
}

pub(super) fn browser_footer_style(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(th::bg_surface().into()),
        border: iced::Border {
            color: th::divider(),
            width: 1.0,
            radius: 0.0.into(),
        },
        ..Default::default()
    }
}

#[cfg(test)]
mod browser_table_tests {
    use super::*;
    use vibez_core::track::MediaSourceRef;

    #[test]
    fn selected_result_metadata_uses_the_selected_foreground() {
        assert_eq!(browser_result_cell_color(true), th::text());
        assert_eq!(browser_result_cell_color(false), th::text_dim());
    }

    #[test]
    fn decoded_metadata_is_compact_and_truthful() {
        let entry = SampleBrowserEntry {
            source: MediaSourceRef::LocalFile {
                path: "/samples/loop.aiff".into(),
            },
            name: "loop.aiff".into(),
            root_path: "/samples".into(),
            relative_path: "loop.aiff".into(),
            format: "AIFF".into(),
            duration_seconds: Some(119.6),
            channels: Some(2),
            sample_rate: Some(48_000),
            file_size: Some(42),
            modified: None,
            search_text: "loop aiff".into(),
        };
        assert_eq!(browser_entry_metadata(&entry), "AIFF · STEREO · 48 KHZ");
        assert_eq!(
            format_browser_duration(entry.duration_seconds.unwrap()),
            "2:00"
        );
    }

    #[test]
    fn remote_folders_begin_beyond_the_connection_and_nest_by_depth() {
        assert!(remote_places_indent(0) > REMOTE_CONNECTION_INDENT);
        assert_eq!(
            remote_places_indent(1) - remote_places_indent(0),
            BROWSER_TREE_INDENT_STEP
        );
        assert_eq!(
            remote_places_indent(8),
            REMOTE_CONNECTION_INDENT + BROWSER_TREE_MAX_DEPTH * BROWSER_TREE_INDENT_STEP
        );
    }
}
