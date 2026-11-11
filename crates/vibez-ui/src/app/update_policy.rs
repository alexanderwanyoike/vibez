//! Router-level message policies kept separate from the exhaustive update
//! dispatch so policy growth cannot turn `update.rs` back into a megafile.

use crate::domains::arrangement::ArrangementMsg;
use crate::message::{BrowserImportTarget, Message};

pub(super) fn apply_project_track_deletion_policy(message: Message, confirm: bool) -> Message {
    match message {
        Message::Arrangement(ArrangementMsg::RequestRemoveTrack(track_id)) if !confirm => {
            Message::Arrangement(ArrangementMsg::RemoveTrack(track_id))
        }
        message => message,
    }
}

/// Whether `message` may take an instrument away, so the undo step before it
/// records the live plugin state.
pub(super) fn may_remove_an_instrument(message: &Message) -> bool {
    match message {
        Message::Devices(msg) => msg.instrument_change_track().is_some(),
        Message::BrowserImportPrepared { target, .. } => {
            matches!(target, BrowserImportTarget::ImpliedSampler(_))
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domains::devices::DevicesMsg;
    use crate::message::PreparedBrowserImport;
    use crate::state::{AuditionImportInput, AuditionMode};
    use std::sync::Arc;
    use vibez_core::audio_buffer::DecodedAudio;
    use vibez_core::id::TrackId;
    use vibez_core::midi::InstrumentKind;
    use vibez_core::track::MediaSourceRef;

    fn prepared(target: BrowserImportTarget) -> Message {
        Message::BrowserImportPrepared {
            target,
            generation: 0,
            payload: PreparedBrowserImport {
                treatment: AuditionImportInput {
                    mode: AuditionMode::Raw,
                    source_bpm: None,
                },
                audio: Arc::new(DecodedAudio {
                    channels: Vec::new(),
                    sample_rate: 44_100,
                }),
                original_audio: None,
                name: String::new(),
                source: MediaSourceRef::LocalFile {
                    path: Default::default(),
                },
            },
        }
    }

    #[test]
    fn instrument_changes_and_panel_sampler_imports_may_remove_an_instrument() {
        let track_id = TrackId::new();
        let set = DevicesMsg::SetTrackInstrument(track_id, InstrumentKind::Sampler);
        assert!(may_remove_an_instrument(&Message::Devices(set)));
        let remove = DevicesMsg::RemoveTrackInstrument(track_id);
        assert!(may_remove_an_instrument(&Message::Devices(remove)));
        assert!(may_remove_an_instrument(&prepared(
            BrowserImportTarget::ImpliedSampler(track_id)
        )));
        assert!(!may_remove_an_instrument(&prepared(
            BrowserImportTarget::Sampler(track_id)
        )));
        let param = DevicesMsg::SetInstrumentParam(track_id, 0, 0.5);
        assert!(!may_remove_an_instrument(&Message::Devices(param)));
    }

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
