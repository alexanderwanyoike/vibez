//! Prepared content edits commit current values through one bounded FIFO storage exchange.

use super::*;
use crate::clip_insertion::{
    ClipInsertionAttempt, ClipInsertionDelta, ClipInsertionOutcome, ClipInsertionReply,
    ClipInsertionTarget, CompletedClipInsertion, PreparedAutomationLaneInsertion,
    PreparedClipInsertion, PreparedNoteClipInsertion, PreparedNoteInsertion,
};
use vibez_core::id::ClipId;

impl AudioEngine {
    pub(super) fn command_add_clip(
        &mut self,
        track_id: TrackId,
        clip: EngineClip,
        mut insertion: Box<PreparedClipInsertion>,
    ) {
        let clip_id = clip.id;
        insertion.clip = Some(ClipInsertionDelta::Audio(clip));
        self.begin_clip_insertion(track_id, ClipInsertionTarget::Clip(clip_id), insertion);
    }

    pub(super) fn command_add_note_clip(
        &mut self,
        track_id: TrackId,
        insertion: PreparedNoteClipInsertion,
    ) {
        let (clip_id, control) = insertion.into_control();
        self.begin_clip_insertion(track_id, ClipInsertionTarget::Clip(clip_id), control);
    }

    pub(super) fn command_add_note(
        &mut self,
        track_id: TrackId,
        clip_id: ClipId,
        insertion: PreparedNoteInsertion,
    ) {
        self.begin_clip_insertion(
            track_id,
            ClipInsertionTarget::Clip(clip_id),
            insertion.into_control(),
        );
    }

    pub(super) fn command_set_automation_lane(
        &mut self,
        track_id: TrackId,
        insertion: PreparedAutomationLaneInsertion,
    ) {
        let (lane_id, control) = insertion.into_control();
        self.begin_clip_insertion(
            track_id,
            ClipInsertionTarget::AutomationLane(lane_id),
            control,
        );
    }

    fn insertion_channel_mut(
        &mut self,
        track_id: TrackId,
        target: ClipInsertionTarget,
    ) -> Option<&mut EngineTrack> {
        match target {
            ClipInsertionTarget::Clip(_) => self
                .tracks
                .iter_mut()
                .find(|track| track.id == track_id)
                .map(|track| &mut **track),
            ClipInsertionTarget::AutomationLane(_) => self.channel_mut(track_id),
        }
    }

    fn complete_missing_insertion(&mut self, mut insertion: Box<PreparedClipInsertion>) {
        let (_, target) = insertion.target.expect("one original insertion target");
        let outcome = match target {
            ClipInsertionTarget::Clip(_) => ClipInsertionOutcome::MissingTrack,
            ClipInsertionTarget::AutomationLane(_) => {
                let Some(ClipInsertionDelta::AutomationLane(lane)) = insertion.clip.take() else {
                    unreachable!("missing channel returns its original lane before the control");
                };
                self.retire_event(EngineEvent::RetiredAutomationLane(lane));
                ClipInsertionOutcome::MissingChannel
            }
        };
        self.complete_clip_insertion(insertion, outcome);
    }

    fn begin_clip_insertion(
        &mut self,
        track_id: TrackId,
        target: ClipInsertionTarget,
        mut insertion: Box<PreparedClipInsertion>,
    ) {
        insertion.target = Some((track_id, target));
        let Some(track) = self.insertion_channel_mut(track_id, target) else {
            self.complete_missing_insertion(insertion);
            return;
        };
        match insertion.try_apply(&mut track.playback_source) {
            ClipInsertionAttempt::Applied(retired_lane) => {
                if let Some(lane) = retired_lane {
                    self.retire_event(EngineEvent::RetiredAutomationLane(lane));
                }
                self.complete_clip_insertion(insertion, ClipInsertionOutcome::Applied)
            }
            ClipInsertionAttempt::MissingClip => {
                self.complete_clip_insertion(insertion, ClipInsertionOutcome::MissingClip)
            }
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
                let (track_id, target) = insertion.target.expect("one pending command target");
                if let ClipInsertionTarget::AutomationLane(lane_id) = target {
                    let needs_two = self.channel_mut(track_id).is_none_or(|channel| {
                        channel
                            .playback_source
                            .automation
                            .iter()
                            .any(|lane| lane.id == lane_id)
                    });
                    // Public FIFO cannot replace or remove this target while waiting.
                    // Defensive owner returns still need both credits before mutation.
                    if needs_two
                        && self.pending_retirements.capacity() - self.pending_retirements.len() < 2
                    {
                        self.pending_clip_insertion = Some(insertion);
                        return;
                    }
                }
                if let Some(track) = self.insertion_channel_mut(track_id, target) {
                    // FIFO is paused and runtime Section/Clip rendering restores
                    // Arrange storage before command drain. No writer can grow it.
                    match insertion.try_apply(&mut track.playback_source) {
                        ClipInsertionAttempt::Applied(retired_lane) => {
                            if let Some(lane) = retired_lane {
                                self.retire_event(EngineEvent::RetiredAutomationLane(lane));
                            }
                            self.complete_clip_insertion(insertion, ClipInsertionOutcome::Applied)
                        }
                        ClipInsertionAttempt::MissingClip => self
                            .complete_clip_insertion(insertion, ClipInsertionOutcome::MissingClip),
                        ClipInsertionAttempt::NeedsCapacity(_) => unreachable!(
                            "FIFO and restored Arrange storage keep the prepared reply sufficient"
                        ),
                    }
                } else {
                    self.complete_missing_insertion(insertion);
                }
            }
        }
    }

    fn complete_clip_insertion(
        &mut self,
        owner: Box<PreparedClipInsertion>,
        outcome: ClipInsertionOutcome,
    ) {
        let recalculate_length =
            owner.affects_length() && outcome != ClipInsertionOutcome::Abandoned;
        let (track_id, target) = owner.target.expect("one insertion outcome target");
        self.retire_event(EngineEvent::ClipInsertionCompleted(
            CompletedClipInsertion {
                track_id,
                target,
                outcome,
                owner,
            },
        ));
        if recalculate_length {
            self.recalculate_audio_length();
        }
    }
}
