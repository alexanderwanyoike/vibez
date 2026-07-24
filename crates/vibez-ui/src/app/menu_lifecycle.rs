//! Explicit lifecycle for application and Arrange context-menu overlays.
//!
//! Background messages never pass through this module. Only a menu item,
//! backdrop press, or Escape is allowed to dismiss one of these overlays.

use crate::message::MenuOverlay;
use crate::state::AppState;

pub(super) fn dismiss(state: &mut AppState, overlay: MenuOverlay) -> bool {
    let was_open = match overlay {
        MenuOverlay::ArrangementContext => state.view.context_menu.is_some(),
        MenuOverlay::File => state.project.file_menu_open,
        MenuOverlay::Edit => state.view.edit_menu_open,
    };

    match overlay {
        MenuOverlay::ArrangementContext => state.view.context_menu = None,
        MenuOverlay::File => state.project.file_menu_open = false,
        MenuOverlay::Edit => state.view.edit_menu_open = false,
    }

    was_open
}

/// Return the topmost menu Escape should dismiss, matching shell overlay order.
pub(super) fn visible(state: &AppState) -> Option<MenuOverlay> {
    if state.project.file_menu_open {
        Some(MenuOverlay::File)
    } else if state.view.edit_menu_open {
        Some(MenuOverlay::Edit)
    } else if state.view.context_menu.is_some() {
        Some(MenuOverlay::ArrangementContext)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domains::view::ViewMsg;
    use crate::message::Message;
    use crate::state::{ContextMenu, ContextMenuTarget};

    fn test_app() -> super::super::App {
        let (plugin_effect_tx, plugin_effect_rx) = std::sync::mpsc::channel();
        let (plugin_instrument_tx, plugin_instrument_rx) = std::sync::mpsc::channel();

        super::super::App {
            state: AppState::default(),
            edge_shortcuts: Default::default(),
            cmd_tx: Default::default(),
            event_rx: None,
            spectrum_rx: None,
            spectrum_tap: None,
            _stream: None,
            plugin_effect_rx,
            plugin_effect_tx,
            plugin_instrument_rx,
            plugin_instrument_tx,
            plugin_window_manager: None,
            plugin_gui_raw_ptrs: Default::default(),
            plugin_state_ptrs: Default::default(),
            dropbox_settings: Default::default(),
            dropbox_cache: Default::default(),
            dropbox_client: None,
            remote_materialization_request: Default::default(),
            remote_import_request: Default::default(),
            remote_audition_cache_lease: None,
            pending_remote_audition: None,
            browser_import_request: Default::default(),
            remote_catalog_request: Default::default(),
            section_residency_request: Default::default(),
            remote_catalog_pending: Vec::new(),
            midi_input: None,
            midi_input_ports: Vec::new(),
        }
    }

    #[test]
    fn backdrop_or_escape_dismisses_an_open_menu_exactly_once() {
        let mut state = AppState::default();
        state.view.context_menu = Some(ContextMenu {
            x: 24.0,
            y: 48.0,
            target: ContextMenuTarget::ArrangementEmpty,
        });

        assert!(dismiss(&mut state, MenuOverlay::ArrangementContext));
        assert!(!dismiss(&mut state, MenuOverlay::ArrangementContext));
    }

    #[test]
    fn escape_targets_the_topmost_visible_application_menu() {
        let mut state = AppState::default();
        state.view.context_menu = Some(ContextMenu {
            x: 24.0,
            y: 48.0,
            target: ContextMenuTarget::ArrangementEmpty,
        });
        state.view.edit_menu_open = true;
        state.project.file_menu_open = true;

        assert_eq!(visible(&state), Some(MenuOverlay::File));
        dismiss(&mut state, MenuOverlay::File);
        assert_eq!(visible(&state), Some(MenuOverlay::Edit));
        dismiss(&mut state, MenuOverlay::Edit);
        assert_eq!(visible(&state), Some(MenuOverlay::ArrangementContext));
    }

    #[test]
    fn unrelated_application_messages_leave_context_and_file_menus_open() {
        let mut app = test_app();
        app.state.view.context_menu = Some(ContextMenu {
            x: 24.0,
            y: 48.0,
            target: ContextMenuTarget::ArrangementEmpty,
        });
        app.state.project.file_menu_open = true;

        let _ = app.update(Message::View(ViewMsg::CursorMoved(50.0, 75.0)));
        let _ = app.update(Message::EngineMetering {
            peak_l: 0.25,
            peak_r: 0.5,
        });

        assert!(app.state.view.context_menu.is_some());
        assert!(app.state.project.file_menu_open);
    }

    #[test]
    fn selecting_an_item_dispatches_its_action_then_dismisses_its_menu() {
        let mut app = test_app();
        app.state.project.file_menu_open = true;

        let _ = app.update(Message::menu_item(MenuOverlay::File, Message::OpenSettings));

        assert!(app.state.settings_open);
        assert!(!app.state.project.file_menu_open);
    }

    #[test]
    fn escape_dismisses_the_menu_before_unrelated_cancellation_paths() {
        let mut app = test_app();
        app.state.project.file_menu_open = true;
        app.state.browser.pending_drag = Some(crate::state::PendingMediaDrag {
            source: vibez_core::track::MediaSourceRef::LocalFile {
                path: "/music/kick.wav".into(),
            },
            label: "kick.wav".into(),
            origin_x: 10.0,
            origin_y: 10.0,
        });

        let _ = app.update(Message::EscapePressed);

        assert!(!app.state.project.file_menu_open);
        assert!(app.state.browser.pending_drag.is_some());
    }
}
