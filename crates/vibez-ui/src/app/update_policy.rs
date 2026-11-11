//! Router-level message policies kept separate from the exhaustive update
//! dispatch so policy growth cannot turn `update.rs` back into a megafile.

use vibez_core::id::TrackId;

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

/// The track whose instrument `message` may take away, so the undo step
/// taken before it can keep that instrument's live plugin state.
pub(super) fn departing_instrument(_message: &Message) -> Option<TrackId> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domains::devices::DevicesMsg;
    use vibez_core::midi::InstrumentKind;

    #[test]
    fn replacing_or_removing_an_instrument_names_the_departing_one() {
        let track_id = TrackId::new();
        for message in [
            Message::Devices(DevicesMsg::SetTrackInstrument(
                track_id,
                InstrumentKind::Sampler,
            )),
            Message::Devices(DevicesMsg::RemoveTrackInstrument(track_id)),
        ] {
            assert_eq!(departing_instrument(&message), Some(track_id));
        }
        let prepared = |target| Message::BrowserImportPrepared {
            target,
            generation: 0,
            payload: crate::message::PreparedBrowserImport {
                treatment: crate::state::AuditionImportInput {
                    mode: crate::state::AuditionMode::Raw,
                    source_bpm: None,
                },
                audio: std::sync::Arc::new(vibez_core::audio_buffer::DecodedAudio {
                    channels: vec![vec![0.0]],
                    sample_rate: 44_100,
                }),
                original_audio: None,
                name: "kick.wav".into(),
                source: vibez_core::track::MediaSourceRef::LocalFile {
                    path: "/kick.wav".into(),
                },
            },
        };
        assert_eq!(
            departing_instrument(&prepared(
                crate::message::BrowserImportTarget::ImpliedSampler(track_id)
            )),
            Some(track_id)
        );
        assert_eq!(
            departing_instrument(&prepared(crate::message::BrowserImportTarget::Sampler(
                track_id
            ))),
            None
        );
        assert_eq!(
            departing_instrument(&Message::Devices(DevicesMsg::SetInstrumentParam(
                track_id, 0, 0.5
            ))),
            None
        );
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
