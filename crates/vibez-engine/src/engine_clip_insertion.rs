//! AddClip commits live content through one bounded FIFO storage exchange.

use super::*;
use crate::clip_insertion::{
    ClipInsertionOutcome, ClipInsertionReply, CompletedClipInsertion, PreparedClipInsertion,
};

impl AudioEngine {
    pub(super) fn command_add_clip(
        &mut self,
        track_id: TrackId,
        clip: EngineClip,
        mut insertion: Box<PreparedClipInsertion>,
    ) {
        insertion.target = Some((track_id, clip.id));
        insertion.clip = Some(clip);
        let Some(track) = self.tracks.iter_mut().find(|track| track.id == track_id) else {
            self.complete_clip_insertion(insertion, ClipInsertionOutcome::MissingTrack);
            self.recalculate_audio_length();
            return;
        };
        if insertion.try_apply(&mut track.playback_source) {
            self.complete_clip_insertion(insertion, ClipInsertionOutcome::Applied);
            self.recalculate_audio_length();
        } else {
            let required = track.playback_source.clips.len() + 1;
            let request = insertion.request(required);
            self.pending_clip_insertion = Some(insertion);
            self.retire_event(EngineEvent::ClipInsertionStorageRequested(request));
        }
    }

    pub(super) fn continue_clip_insertion(&mut self) {
        if self.pending_retirements.len() == self.pending_retirements.capacity() {
            return;
        }
        let Some(mut insertion) = self.pending_clip_insertion.take() else {
            return;
        };
        match insertion.receive() {
            ClipInsertionReply::Waiting => self.pending_clip_insertion = Some(insertion),
            ClipInsertionReply::Abandoned => {
                self.complete_clip_insertion(insertion, ClipInsertionOutcome::Abandoned);
            }
            ClipInsertionReply::Ready => {
                let (track_id, _) = insertion.target.expect("one pending command target");
                if let Some(track) = self.tracks.iter_mut().find(|track| track.id == track_id) {
                    // FIFO is paused and runtime Section/Clip rendering restores
                    // Arrange storage before command drain. No writer can grow it.
                    assert!(insertion.try_apply(&mut track.playback_source));
                    self.complete_clip_insertion(insertion, ClipInsertionOutcome::Applied);
                } else {
                    self.complete_clip_insertion(insertion, ClipInsertionOutcome::MissingTrack);
                }
                self.recalculate_audio_length();
            }
        }
    }

    fn complete_clip_insertion(
        &mut self,
        owner: Box<PreparedClipInsertion>,
        outcome: ClipInsertionOutcome,
    ) {
        let (track_id, clip_id) = owner.target.expect("one insertion outcome target");
        self.retire_event(EngineEvent::ClipInsertionCompleted(CompletedClipInsertion {
            track_id,
            clip_id,
            outcome,
            owner,
        }));
    }
}
