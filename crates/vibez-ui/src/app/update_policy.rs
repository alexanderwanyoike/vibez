//! Router-level message policies kept separate from the exhaustive update
//! dispatch so policy growth cannot turn `update.rs` back into a megafile.

use crate::domains::arrangement::ArrangementMsg;
use crate::domains::project::ProjectMsg;
use crate::domains::transport::TransportMsg;
use crate::domains::view::ViewMsg;
use crate::message::Message;

pub(super) fn apply_project_track_deletion_policy(message: Message, confirm: bool) -> Message {
    match message {
        Message::Arrangement(ArrangementMsg::RequestRemoveTrack(track_id)) if !confirm => {
            Message::Arrangement(ArrangementMsg::RemoveTrack(track_id))
        }
        message => message,
    }
}

pub(super) fn context_menu_message_keeps_open(message: &Message) -> bool {
    if let Message::Browser(message) = message {
        return message.keeps_context_menu();
    }

    matches!(
        message,
        Message::Tick
            | Message::Transport(TransportMsg::EnginePosition(_))
            | Message::EngineMetering { .. }
            | Message::Transport(TransportMsg::EngineStopped)
            | Message::Arrangement(ArrangementMsg::EngineTrackMeter { .. })
            | Message::View(ViewMsg::ShowContextMenu { .. })
            | Message::View(ViewMsg::DismissContextMenu)
            | Message::Arrangement(ArrangementMsg::DeleteClipsInRegion { .. })
            | Message::Arrangement(ArrangementMsg::SetSelectionAsLoop)
            | Message::Arrangement(ArrangementMsg::DeleteSelectedClip)
            | Message::Arrangement(ArrangementMsg::DuplicateSelectedClip)
            | Message::Arrangement(ArrangementMsg::SplitSelectedAtPlayhead)
            | Message::Arrangement(ArrangementMsg::JoinSelectedClips)
            | Message::Arrangement(ArrangementMsg::SplitAudioClip { .. })
            | Message::Arrangement(ArrangementMsg::SplitNoteClip { .. })
            | Message::Arrangement(ArrangementMsg::SplitClipsAtRegion { .. })
            | Message::Arrangement(ArrangementMsg::CreateNoteClipFromSelection(_))
            | Message::View(ViewMsg::EditNameText(_))
            | Message::View(ViewMsg::CursorMoved(_, _))
            | Message::View(ViewMsg::WindowResized(_, _))
            | Message::View(ViewMsg::MouseReleased)
            | Message::KeyboardInput {
                event: iced::keyboard::Event::ModifiersChanged(_),
                ..
            }
            | Message::RemoteCatalogPageFetched { .. }
            | Message::RemoteCatalogSaved { .. }
            | Message::NewProject
            | Message::OpenProject
            | Message::SaveProject
            | Message::SaveProjectAs
            | Message::Project(ProjectMsg::ToggleFileMenu)
            | Message::Project(ProjectMsg::DismissFileMenu)
            | Message::ProjectOpenPathSelected(_)
            | Message::ProjectSavePathSelected(_)
            | Message::ProjectLoaded(_)
            | Message::ProjectSaved(_)
            | Message::OpenSettings
            | Message::CloseSettings
            | Message::SelectSettingsTab(_)
            | Message::SetBufferSize(_)
            | Message::ScanPlugins
            | Message::ScanPluginsComplete(_)
            | Message::PluginLoadError(_)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domains::browser::BrowserMsg;
    use vibez_core::id::TrackId;

    #[test]
    fn project_track_deletion_policy_defaults_to_the_direct_undoable_command() {
        let track_id = TrackId::new();
        let direct = apply_project_track_deletion_policy(
            Message::Arrangement(ArrangementMsg::RequestRemoveTrack(track_id)),
            false,
        );
        assert!(matches!(
            direct,
            Message::Arrangement(ArrangementMsg::RemoveTrack(id)) if id == track_id
        ));

        let confirmed = apply_project_track_deletion_policy(
            Message::Arrangement(ArrangementMsg::RequestRemoveTrack(track_id)),
            true,
        );
        assert!(matches!(
            confirmed,
            Message::Arrangement(ArrangementMsg::RequestRemoveTrack(id)) if id == track_id
        ));
    }

    #[test]
    fn passive_arrangement_drag_hover_does_not_dismiss_context_menu() {
        assert!(context_menu_message_keeps_open(&Message::Browser(
            BrowserMsg::DragHoverTrack {
                track_id: TrackId::new(),
                beat: 4.0,
                compatible: true,
            },
        )));
    }

    #[test]
    fn deliberate_browser_actions_dismiss_context_menu() {
        assert!(!context_menu_message_keeps_open(&Message::Browser(
            BrowserMsg::ToggleSampleBrowser,
        )));
    }

    #[test]
    fn remote_catalog_progress_does_not_dismiss_context_menu() {
        assert!(context_menu_message_keeps_open(
            &Message::RemoteCatalogSaved {
                generation: 1,
                next_checkpoint: None,
                result: Ok(()),
            },
        ));
    }

    #[test]
    fn modifier_state_sync_after_right_click_does_not_dismiss_context_menu() {
        assert!(context_menu_message_keeps_open(&Message::KeyboardInput {
            event: iced::keyboard::Event::ModifiersChanged(iced::keyboard::Modifiers::empty()),
            occurred_at: std::time::Instant::now(),
        }));
    }
}
