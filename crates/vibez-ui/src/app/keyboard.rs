//! Global keyboard shortcuts.

//! Split out of app.rs; inherent methods on [`super::App`].

use crate::domains::arrangement::ArrangementMsg;
use crate::domains::browser::BrowserMsg;
use crate::domains::perform::{PerformMode, PerformMsg};
use crate::domains::piano_roll::PianoRollMsg;
use crate::domains::project::ProjectMsg;
use crate::domains::transport::TransportMsg;
use crate::domains::view::ViewMsg;

use crate::message::Message;

#[derive(Default)]
pub(crate) struct EdgeShortcutState {
    pressed_keys: std::collections::HashSet<String>,
    released_at: std::collections::HashMap<String, std::time::Instant>,
}

impl EdgeShortcutState {
    fn should_dispatch(
        &mut self,
        key_id: &str,
        message: &Message,
        occurred_at: std::time::Instant,
    ) -> bool {
        let edge_triggered = matches!(
            message,
            Message::Arrangement(ArrangementMsg::AddTrack | ArrangementMsg::AddInstrumentTrack)
        );
        if !edge_triggered {
            return true;
        }

        let release_gap = self
            .released_at
            .get(key_id)
            .map(|released_at| occurred_at.saturating_duration_since(*released_at));
        let synthetic_repeat =
            release_gap.is_some_and(|gap| gap <= std::time::Duration::from_millis(30));
        let first_press = self.pressed_keys.insert(key_id.to_owned());
        first_press && !synthetic_repeat
    }

    fn release(&mut self, key_id: &str, occurred_at: std::time::Instant) {
        self.pressed_keys.remove(key_id);
        self.released_at.insert(key_id.to_owned(), occurred_at);
    }
}

pub(crate) fn keyboard_input_message(
    event: iced::keyboard::Event,
    status: iced::event::Status,
) -> Option<Message> {
    let should_forward = match &event {
        iced::keyboard::Event::KeyPressed { .. } => status == iced::event::Status::Ignored,
        // A release must clear a press that began before focus moved into a
        // text field. Unpaired releases are harmless in the adapter.
        iced::keyboard::Event::KeyReleased { .. } => true,
        iced::keyboard::Event::ModifiersChanged(modifiers) => {
            status == iced::event::Status::Ignored || !modifiers.shift()
        }
    };
    should_forward.then(|| Message::KeyboardInput {
        event,
        occurred_at: std::time::Instant::now(),
    })
}

fn computer_key_from_physical(
    physical: iced::keyboard::key::Physical,
) -> Option<crate::domains::perform::ComputerKey> {
    use crate::domains::perform::ComputerKey;
    use iced::keyboard::key::Code;

    let iced::keyboard::key::Physical::Code(code) = physical else {
        return None;
    };
    Some(match code {
        Code::Digit0 => ComputerKey::Digit0,
        Code::Digit1 => ComputerKey::Digit1,
        Code::Digit2 => ComputerKey::Digit2,
        Code::Digit3 => ComputerKey::Digit3,
        Code::Digit4 => ComputerKey::Digit4,
        Code::Digit5 => ComputerKey::Digit5,
        Code::Digit6 => ComputerKey::Digit6,
        Code::Digit7 => ComputerKey::Digit7,
        Code::Digit8 => ComputerKey::Digit8,
        Code::Digit9 => ComputerKey::Digit9,
        Code::KeyA => ComputerKey::A,
        Code::KeyB => ComputerKey::B,
        Code::KeyC => ComputerKey::C,
        Code::KeyD => ComputerKey::D,
        Code::KeyE => ComputerKey::E,
        Code::KeyF => ComputerKey::F,
        Code::KeyG => ComputerKey::G,
        Code::KeyH => ComputerKey::H,
        Code::KeyI => ComputerKey::I,
        Code::KeyJ => ComputerKey::J,
        Code::KeyK => ComputerKey::K,
        Code::KeyL => ComputerKey::L,
        Code::KeyM => ComputerKey::M,
        Code::KeyN => ComputerKey::N,
        Code::KeyO => ComputerKey::O,
        Code::KeyP => ComputerKey::P,
        Code::KeyQ => ComputerKey::Q,
        Code::KeyR => ComputerKey::R,
        Code::KeyS => ComputerKey::S,
        Code::KeyT => ComputerKey::T,
        Code::KeyU => ComputerKey::U,
        Code::KeyV => ComputerKey::V,
        Code::KeyW => ComputerKey::W,
        Code::KeyX => ComputerKey::X,
        Code::KeyY => ComputerKey::Y,
        Code::KeyZ => ComputerKey::Z,
        _ => return None,
    })
}

fn runtime_key_id(key: &iced::keyboard::Key) -> String {
    format!("{key:?}")
}

fn is_note_repeat_key(physical: iced::keyboard::key::Physical) -> bool {
    matches!(
        physical,
        iced::keyboard::key::Physical::Code(iced::keyboard::key::Code::KeyN)
    )
}

fn is_note_repeat_release(key: &iced::keyboard::Key) -> bool {
    matches!(key, iced::keyboard::Key::Character(value) if value.eq_ignore_ascii_case("n"))
}

impl super::App {
    pub(super) fn handle_keyboard_input(
        &mut self,
        event: iced::keyboard::Event,
        occurred_at: std::time::Instant,
    ) -> iced::Task<Message> {
        use iced::keyboard::key::Named;

        let (perform_msg, fallback) = match event {
            iced::keyboard::Event::KeyPressed {
                key,
                physical_key,
                modifiers,
                ..
            } => {
                let edge_key_id = runtime_key_id(&key);
                if self.state.perform.key_rebind_target.is_some()
                    && matches!(key, iced::keyboard::Key::Named(Named::Escape))
                {
                    (Some(PerformMsg::CancelKeyRebind), None)
                } else if self.state.perform.key_rebind_target.is_some()
                    && is_note_repeat_key(physical_key)
                {
                    self.state.status_text = "N is reserved for Note Repeat".into();
                    return iced::Task::none();
                } else if is_note_repeat_key(physical_key)
                    && modifiers.is_empty()
                    && self.state.view.workspace == crate::state::Workspace::Perform
                    && self.state.perform.mode == PerformMode::Instrument
                {
                    (Some(PerformMsg::SetNoteRepeatMomentary(true)), None)
                } else if let Some(computer_key) = computer_key_from_physical(physical_key) {
                    if modifiers.is_empty()
                        || (self.state.perform.instrument_target_overlay
                            && self.state.perform.mode == PerformMode::Instrument
                            && modifiers == iced::keyboard::Modifiers::SHIFT)
                        || self.state.perform.key_rebind_target.is_some()
                    {
                        (
                            Some(PerformMsg::ComputerKeyPressed {
                                key: computer_key,
                                key_id: runtime_key_id(&key),
                                occurred_at,
                            }),
                            Some((key, modifiers, edge_key_id)),
                        )
                    } else {
                        (None, Some((key, modifiers, edge_key_id)))
                    }
                } else if self.state.perform.key_rebind_target.is_some() {
                    self.state.status_text = "Perform keys must be letters or numbers".into();
                    return iced::Task::none();
                } else {
                    (None, Some((key, modifiers, edge_key_id)))
                }
            }
            iced::keyboard::Event::KeyReleased { key, .. } => {
                let key_id = runtime_key_id(&key);
                self.edge_shortcuts.release(&key_id, occurred_at);
                if is_note_repeat_release(&key) {
                    (Some(PerformMsg::SetNoteRepeatMomentary(false)), None)
                } else {
                    (
                        Some(PerformMsg::ComputerKeyReleased {
                            key_id,
                            occurred_at,
                        }),
                        None,
                    )
                }
            }
            iced::keyboard::Event::ModifiersChanged(modifiers) => (
                Some(PerformMsg::SetInstrumentTargetOverlay(modifiers.shift())),
                None,
            ),
        };

        if let Some(msg) = perform_msg {
            let ctx = crate::domains::perform::PerformCtx {
                workspace_visible: self.state.view.workspace == crate::state::Workspace::Perform,
                project_tracks: &self.state.project_tracks.tracks,
                selected_project_track: self.state.arrangement.selected_track,
            };
            let action = {
                let mut engine = crate::domains::EngineTx(&mut self.cmd_tx);
                self.state.perform.update(msg, &mut engine, ctx)
            };
            let keyboard_consumed = action.keyboard_consumed;
            let task = self.apply_perform_action(action);
            if keyboard_consumed {
                return task;
            }
        }

        fallback
            .and_then(|(key, modifiers, key_id)| {
                let message = global_key_handler(key, modifiers)?;
                self.edge_shortcuts
                    .should_dispatch(&key_id, &message, occurred_at)
                    .then_some(message)
            })
            .map_or_else(iced::Task::none, iced::Task::done)
    }
}

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

    if modifiers.is_empty() {
        let mode = match key {
            iced::keyboard::Key::Named(Named::F1) => Some(PerformMode::Sections),
            iced::keyboard::Key::Named(Named::F2) => Some(PerformMode::TrackMutes),
            iced::keyboard::Key::Named(Named::F3) => Some(PerformMode::Instrument),
            _ => None,
        };
        if let Some(mode) = mode {
            return Some(Message::Perform(PerformMsg::SelectMode(mode)));
        }
        if let iced::keyboard::Key::Character(ref character) = key {
            let bank = match character.as_str() {
                "[" => Some(PerformMsg::PreviousBank),
                "]" => Some(PerformMsg::NextBank),
                _ => None,
            };
            if let Some(bank) = bank {
                return Some(Message::Perform(bank));
            }
        }
    }

    // Escape: the router stops Audition first, then falls back to cancel editing.
    if matches!(key, iced::keyboard::Key::Named(Named::Escape)) {
        return Some(Message::EscapePressed);
    }

    // Plain Up/Down moves the Active Source Entry through Browser Results.
    // Text inputs consume these before the global ignored-event subscription.
    if modifiers.is_empty() {
        match key {
            iced::keyboard::Key::Named(Named::ArrowUp) => {
                return Some(Message::SelectAdjacentBrowserResult(-1));
            }
            iced::keyboard::Key::Named(Named::ArrowDown) => {
                return Some(Message::SelectAdjacentBrowserResult(1));
            }
            _ => {}
        }
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
            "m" | "M" => Some(Message::create_clip_from_selection()),
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
    fn create_clip_shortcut_accepts_shifted_m() {
        use iced::keyboard::{Key, Modifiers};

        assert!(matches!(
            global_key_handler(
                Key::Character("M".into()),
                Modifiers::CTRL | Modifiers::SHIFT
            ),
            Some(Message::Arrangement(
                ArrangementMsg::CreateClipFromSelection
            ))
        ));
    }

    #[test]
    fn held_track_shortcut_dispatches_once_until_key_release() {
        let mut state = EdgeShortcutState::default();
        let add = Message::Arrangement(ArrangementMsg::AddInstrumentTrack);
        let started = std::time::Instant::now();

        assert!(state.should_dispatch("Character(\"T\")", &add, started));
        assert!(!state.should_dispatch("Character(\"T\")", &add, started));

        // X11 auto-repeat is delivered as a synthetic release/press pair.
        state.release(
            "Character(\"T\")",
            started + std::time::Duration::from_millis(500),
        );
        assert!(!state.should_dispatch(
            "Character(\"T\")",
            &add,
            started + std::time::Duration::from_millis(501),
        ));

        state.release(
            "Character(\"T\")",
            started + std::time::Duration::from_millis(900),
        );
        assert!(state.should_dispatch(
            "Character(\"T\")",
            &add,
            started + std::time::Duration::from_millis(1_000),
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
    fn plain_arrows_navigate_browser_results_without_stealing_track_reorder() {
        use iced::keyboard::{key::Named, Key, Modifiers};

        assert!(matches!(
            global_key_handler(Key::Named(Named::ArrowUp), Modifiers::empty()),
            Some(Message::SelectAdjacentBrowserResult(-1))
        ));
        assert!(matches!(
            global_key_handler(Key::Named(Named::ArrowDown), Modifiers::empty()),
            Some(Message::SelectAdjacentBrowserResult(1))
        ));
        assert!(matches!(
            global_key_handler(Key::Named(Named::ArrowUp), Modifiers::CTRL),
            Some(Message::Arrangement(ArrangementMsg::MoveSelectedTrackUp))
        ));
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

    #[test]
    fn function_keys_select_the_three_perform_modes() {
        use iced::keyboard::{key::Named, Key, Modifiers};

        let expected = [
            (Named::F1, PerformMode::Sections),
            (Named::F2, PerformMode::TrackMutes),
            (Named::F3, PerformMode::Instrument),
        ];
        for (key, expected_mode) in expected {
            assert!(matches!(
                global_key_handler(Key::Named(key), Modifiers::empty()),
                Some(Message::Perform(PerformMsg::SelectMode(mode))) if mode == expected_mode
            ));
        }

        assert!(global_key_handler(Key::Named(Named::F1), Modifiers::SHIFT).is_none());
    }

    #[test]
    fn brackets_navigate_the_active_perform_mode_bank() {
        use iced::keyboard::{Key, Modifiers};

        assert!(matches!(
            global_key_handler(Key::Character("[".into()), Modifiers::empty()),
            Some(Message::Perform(PerformMsg::PreviousBank))
        ));
        assert!(matches!(
            global_key_handler(Key::Character("]".into()), Modifiers::empty()),
            Some(Message::Perform(PerformMsg::NextBank))
        ));
        assert!(global_key_handler(Key::Character("]".into()), Modifiers::SHIFT).is_none());
    }

    #[test]
    fn physical_n_is_the_momentary_note_repeat_control() {
        use iced::keyboard::key::{Code, Physical};
        use iced::keyboard::Key;

        assert!(is_note_repeat_key(Physical::Code(Code::KeyN)));
        assert!(!is_note_repeat_key(Physical::Code(Code::KeyM)));
        assert!(is_note_repeat_release(&Key::Character("n".into())));
        assert!(is_note_repeat_release(&Key::Character("N".into())));
    }

    #[test]
    fn text_field_capture_suppresses_computer_pad_presses() {
        use iced::keyboard::key::{Code, Physical};
        use iced::keyboard::{Event, Key, Location, Modifiers};

        let event = Event::KeyPressed {
            key: Key::Character("q".into()),
            modified_key: Key::Character("q".into()),
            physical_key: Physical::Code(Code::KeyQ),
            location: Location::Standard,
            modifiers: Modifiers::empty(),
            text: Some("q".into()),
        };

        assert!(keyboard_input_message(event.clone(), iced::event::Status::Captured).is_none());
        assert!(matches!(
            keyboard_input_message(event, iced::event::Status::Ignored),
            Some(Message::KeyboardInput { .. })
        ));
    }

    #[test]
    fn uncaptured_shift_changes_reach_the_instrument_target_overlay() {
        use iced::keyboard::{Event, Modifiers};

        assert!(matches!(
            keyboard_input_message(
                Event::ModifiersChanged(Modifiers::SHIFT),
                iced::event::Status::Ignored,
            ),
            Some(Message::KeyboardInput {
                event: Event::ModifiersChanged(modifiers),
                ..
            }) if modifiers.shift()
        ));
        assert!(keyboard_input_message(
            Event::ModifiersChanged(Modifiers::SHIFT),
            iced::event::Status::Captured,
        )
        .is_none());
    }

    #[test]
    fn repeated_t_taps_dispatch_while_control_stays_held() {
        let mut state = EdgeShortcutState::default();
        let add = Message::Arrangement(ArrangementMsg::AddTrack);
        let started = std::time::Instant::now();

        for tap in 0..4 {
            let pressed_at = started + std::time::Duration::from_millis(tap * 300);
            assert!(state.should_dispatch("Character(\"t\")", &add, pressed_at));
            state.release(
                "Character(\"t\")",
                pressed_at + std::time::Duration::from_millis(100),
            );
        }
    }
}
