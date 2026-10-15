//! Clip and note value appends commit live content through one bounded FIFO storage exchange.

use super::*;
use vibez_core::id::ClipId;
use crate::clip_insertion::{
    ClipInsertionAttempt, ClipInsertionDelta, ClipInsertionOutcome, ClipInsertionReply, CompletedClipInsertion,
    PreparedClipInsertion, PreparedNoteClipInsertion, PreparedNoteInsertion,
};

impl AudioEngine {
    pub(super) fn command_add_clip(
        &mut self,
        track_id: TrackId,
        clip: EngineClip,
        mut insertion: Box<PreparedClipInsertion>,
    ) {
        let clip_id = clip.id;
        insertion.clip = Some(ClipInsertionDelta::Audio(clip));
        self.begin_clip_insertion(track_id, clip_id, insertion);
    }

    pub(super) fn command_add_note_clip(
        &mut self,
        track_id: TrackId,
        insertion: PreparedNoteClipInsertion,
    ) {
        let (clip_id, control) = insertion.into_control();
        self.begin_clip_insertion(track_id, clip_id, control);
    }

    pub(super) fn command_add_note(
        &mut self,
        track_id: TrackId,
        clip_id: ClipId,
        insertion: PreparedNoteInsertion,
    ) {
        self.begin_clip_insertion(track_id, clip_id, insertion.into_control());
    }

    fn begin_clip_insertion(
        &mut self,
        track_id: TrackId,
        clip_id: ClipId,
        mut insertion: Box<PreparedClipInsertion>,
    ) {
        insertion.target = Some((track_id, clip_id));
        let Some(track) = self.tracks.iter_mut().find(|track| track.id == track_id) else {
            self.complete_clip_insertion(insertion, ClipInsertionOutcome::MissingTrack);
            return;
        };
        match insertion.try_apply(&mut track.playback_source) {
            ClipInsertionAttempt::Applied => self.complete_clip_insertion(insertion, ClipInsertionOutcome::Applied),
            ClipInsertionAttempt::MissingClip => self.complete_clip_insertion(insertion, ClipInsertionOutcome::MissingClip),
            ClipInsertionAttempt::NeedsCapacity(required) => {
                let request = insertion.request(required);
                self.pending_clip_insertion = Some(insertion);
                self.retire_event(EngineEvent::ClipInsertionStorageRequested(request));
            }
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
                    match insertion.try_apply(&mut track.playback_source) {
                        ClipInsertionAttempt::Applied => self.complete_clip_insertion(insertion, ClipInsertionOutcome::Applied),
                        ClipInsertionAttempt::MissingClip => self.complete_clip_insertion(insertion, ClipInsertionOutcome::MissingClip),
                        ClipInsertionAttempt::NeedsCapacity(_) => unreachable!("FIFO and restored Arrange storage keep the prepared reply sufficient"),
                    }
                } else {
                    self.complete_clip_insertion(insertion, ClipInsertionOutcome::MissingTrack);
                }
            }
        }
    }

    fn complete_clip_insertion(
        &mut self,
        owner: Box<PreparedClipInsertion>,
        outcome: ClipInsertionOutcome,
    ) {
        let recalculate_length = owner.affects_length() && outcome != ClipInsertionOutcome::Abandoned;
        let (track_id, clip_id) = owner.target.expect("one insertion outcome target");
        self.retire_event(EngineEvent::ClipInsertionCompleted(
            CompletedClipInsertion {
                track_id,
                clip_id,
                outcome,
                owner,
            },
        ));
        if recalculate_length { self.recalculate_audio_length(); }
    }
}
