//! Global keyboard shortcuts.

//! Split out of app.rs; inherent methods on [`super::App`].

use crate::domains::arrangement::ArrangementMsg;
use crate::domains::browser::BrowserMsg;
use crate::domains::piano_roll::PianoRollMsg;
use crate::domains::project::ProjectMsg;
use crate::domains::transport::TransportMsg;
use crate::domains::view::ViewMsg;

use crate::message::Message;

pub(crate) fn truncate_end(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        text.to_string()
    } else {
        let head: String = text.chars().take(max_chars.saturating_sub(2)).collect();
        format!("{head}..")
    }
}

pub(crate) fn global_key_handler(
    key: iced::keyboard::Key,
    modifiers: iced::keyboard::Modifiers,
) -> Option<Message> {
    use iced::keyboard::key::Named;

    // Space: toggle playback (no modifiers required)
    if matches!(key, iced::keyboard::Key::Named(Named::Space)) {
        return Some(Message::Transport(TransportMsg::TogglePlayback));
    }

    // Escape: the router stops Audition first, then falls back to cancel editing.
    if matches!(key, iced::keyboard::Key::Named(Named::Escape)) {
        return Some(Message::EscapePressed);
    }

    // Enter: the focused Browser selection always means Arrangement Import.
    // Text inputs capture Enter before this global ignored-event subscription.
    if !modifiers.control()
        && !modifiers.alt()
        && !modifiers.shift()
        && !modifiers.logo()
        && matches!(key, iced::keyboard::Key::Named(Named::Enter))
    {
        return Some(Message::ImportSelectedBrowserSampleToArrangement);
    }

    // Delete/Backspace: context-resolved in update() (selected notes
    // first, then selected clips) and ignored while renaming.
    if !modifiers.control()
        && matches!(
            key,
            iced::keyboard::Key::Named(Named::Delete)
                | iced::keyboard::Key::Named(Named::Backspace)
        )
    {
        return Some(Message::DeleteKeyPressed);
    }

    // B: toggle piano roll draw mode (no modifiers)
    if !modifiers.control()
        && !modifiers.shift()
        && matches!(key, iced::keyboard::Key::Character(ref c) if c.as_str() == "b")
    {
        return Some(Message::PianoRoll(PianoRollMsg::ToggleEditMode));
    }

    // Alt+Left / Alt+Right: resize the Browser without permanent header chrome.
    if modifiers.alt() && !modifiers.control() && !modifiers.shift() && !modifiers.logo() {
        match key {
            iced::keyboard::Key::Named(Named::ArrowLeft) => {
                return Some(Message::Browser(BrowserMsg::NudgeDockWidth(-40.0)));
            }
            iced::keyboard::Key::Named(Named::ArrowRight) => {
                return Some(Message::Browser(BrowserMsg::NudgeDockWidth(40.0)));
            }
            _ => {}
        }
    }

    if modifiers.command() {
        if let iced::keyboard::Key::Character(ref c) = key {
            let grid_message = match c.as_str() {
                "1" => Some(ViewMsg::NarrowGrid),
                "2" => Some(ViewMsg::WidenGrid),
                "3" => Some(ViewMsg::ToggleTripletGrid),
                "4" => Some(ViewMsg::ToggleSnapToGrid),
                "5" => Some(ViewMsg::ToggleAdaptiveGrid),
                _ => None,
            };
            if let Some(message) = grid_message {
                return Some(Message::View(message));
            }
        }
    }

    if !modifiers.control() {
        return None;
    }
    match key {
        iced::keyboard::Key::Named(Named::ArrowUp) => {
            Some(Message::Arrangement(ArrangementMsg::MoveSelectedTrackUp))
        }
        iced::keyboard::Key::Named(Named::ArrowDown) => {
            Some(Message::Arrangement(ArrangementMsg::MoveSelectedTrackDown))
        }
        iced::keyboard::Key::Character(ref c) => match c.as_str() {
            "c" | "C" => Some(Message::Arrangement(ArrangementMsg::CopySelectedClips)),
            "x" | "X" => Some(Message::Arrangement(ArrangementMsg::CutSelectedClips)),
            "v" | "V" => Some(Message::Arrangement(ArrangementMsg::PasteClipsAtPlayhead)),
            "t" | "T" => {
                if modifiers.shift() {
                    Some(Message::Arrangement(ArrangementMsg::AddInstrumentTrack))
                } else {
                    Some(Message::Arrangement(ArrangementMsg::AddTrack))
                }
            }
            "m" => Some(Message::create_clip_from_selection()),
            "e" => Some(Message::split_selected_at_playhead()),
            "j" => Some(Message::join_selected_clips()),
            "l" | "L" => {
                if modifiers.shift() {
                    Some(Message::Arrangement(ArrangementMsg::ToggleSelectedClipLoop))
                } else {
                    Some(Message::Transport(TransportMsg::ToggleArrangementLoop))
                }
            }
            "0" => Some(Message::View(ViewMsg::ZoomToFit)),
            "z" | "Z" => {
                if modifiers.shift() {
                    Some(Message::Project(ProjectMsg::Redo))
                } else {
                    Some(Message::Project(ProjectMsg::Undo))
                }
            }
            "y" | "Y" => Some(Message::Project(ProjectMsg::Redo)),
            _ => None,
        },
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_number_shortcuts_control_the_shared_grid() {
        use iced::keyboard::{Key, Modifiers};

        #[cfg(target_os = "macos")]
        let command = Modifiers::LOGO;
        #[cfg(not(target_os = "macos"))]
        let command = Modifiers::CTRL;

        let expected = [
            ViewMsg::NarrowGrid,
            ViewMsg::WidenGrid,
            ViewMsg::ToggleTripletGrid,
            ViewMsg::ToggleSnapToGrid,
            ViewMsg::ToggleAdaptiveGrid,
        ];
        for (number, expected) in ["1", "2", "3", "4", "5"].into_iter().zip(expected) {
            let message = global_key_handler(Key::Character(number.into()), command);
            assert!(matches!(
                (message, expected),
                (
                    Some(Message::View(ViewMsg::NarrowGrid)),
                    ViewMsg::NarrowGrid
                ) | (Some(Message::View(ViewMsg::WidenGrid)), ViewMsg::WidenGrid)
                    | (
                        Some(Message::View(ViewMsg::ToggleTripletGrid)),
                        ViewMsg::ToggleTripletGrid
                    )
                    | (
                        Some(Message::View(ViewMsg::ToggleSnapToGrid)),
                        ViewMsg::ToggleSnapToGrid
                    )
                    | (
                        Some(Message::View(ViewMsg::ToggleAdaptiveGrid)),
                        ViewMsg::ToggleAdaptiveGrid
                    )
            ));
        }
    }

    #[test]
    fn alt_arrows_resize_the_browser_without_header_buttons() {
        use iced::keyboard::{key::Named, Key, Modifiers};

        let narrower = global_key_handler(Key::Named(Named::ArrowLeft), Modifiers::ALT);
        let wider = global_key_handler(Key::Named(Named::ArrowRight), Modifiers::ALT);

        assert!(matches!(
            narrower,
            Some(Message::Browser(BrowserMsg::NudgeDockWidth(delta))) if delta == -40.0
        ));
        assert!(matches!(
            wider,
            Some(Message::Browser(BrowserMsg::NudgeDockWidth(delta))) if delta == 40.0
        ));
    }

    #[test]
    fn enter_is_always_arrangement_import() {
        use iced::keyboard::{key::Named, Key, Modifiers};

        assert!(matches!(
            global_key_handler(Key::Named(Named::Enter), Modifiers::empty()),
            Some(Message::ImportSelectedBrowserSampleToArrangement)
        ));
        assert!(global_key_handler(Key::Named(Named::Enter), Modifiers::SHIFT).is_none());
    }

    #[test]
    fn space_is_transport_and_escape_routes_through_audition_priority() {
        use iced::keyboard::{key::Named, Key, Modifiers};

        assert!(matches!(
            global_key_handler(Key::Named(Named::Space), Modifiers::empty()),
            Some(Message::Transport(TransportMsg::TogglePlayback))
        ));
        assert!(matches!(
            global_key_handler(Key::Named(Named::Escape), Modifiers::empty()),
            Some(Message::EscapePressed)
        ));
    }
}
