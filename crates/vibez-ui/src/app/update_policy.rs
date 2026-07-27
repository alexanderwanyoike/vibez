//! Router-level message policies kept separate from the exhaustive update
//! dispatch so policy growth cannot turn `update.rs` back into a megafile.

use crate::domains::arrangement::ArrangementMsg;
use crate::message::Message;

pub(super) fn apply_project_track_deletion_policy(message: Message, confirm: bool) -> Message {
    match message {
        Message::Arrangement(ArrangementMsg::RequestRemoveTrack(track_id)) if !confirm => {
            Message::Arrangement(ArrangementMsg::RemoveTrack(track_id))
        }
        message => message,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
}
