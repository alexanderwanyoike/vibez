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
        allowance: &mut RetirementAllowance,
    ) {
        let clip_id = clip.id;
        insertion.clip = Some(ClipInsertionDelta::Audio(clip));
        self.begin_clip_insertion(
            track_id,
            ClipInsertionTarget::Clip(clip_id),
            insertion,
            allowance,
        );
    }

    pub(super) fn command_add_note_clip(
        &mut self,
        track_id: TrackId,
        insertion: PreparedNoteClipInsertion,
        allowance: &mut RetirementAllowance,
    ) {
        let (clip_id, control) = insertion.into_control();
        self.begin_clip_insertion(
            track_id,
            ClipInsertionTarget::Clip(clip_id),
            control,
            allowance,
        );
    }

    pub(super) fn command_add_note(
        &mut self,
        track_id: TrackId,
        clip_id: ClipId,
        insertion: PreparedNoteInsertion,
        allowance: &mut RetirementAllowance,
    ) {
        self.begin_clip_insertion(
            track_id,
            ClipInsertionTarget::Clip(clip_id),
            insertion.into_control(),
            allowance,
        );
    }

    pub(super) fn command_set_automation_lane(
        &mut self,
        track_id: TrackId,
        insertion: PreparedAutomationLaneInsertion,
        allowance: &mut RetirementAllowance,
    ) {
        let (lane_id, control) = insertion.into_control();
        self.begin_clip_insertion(
            track_id,
            ClipInsertionTarget::AutomationLane(lane_id),
            control,
            allowance,
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

    fn complete_missing_insertion(
        &mut self,
        mut insertion: Box<PreparedClipInsertion>,
        allowance: &mut RetirementAllowance,
    ) {
        let (_, target) = insertion.target.expect("one original insertion target");
        let outcome = match target {
            ClipInsertionTarget::Clip(_) => ClipInsertionOutcome::MissingTrack,
            ClipInsertionTarget::AutomationLane(_) => {
                let Some(ClipInsertionDelta::AutomationLane(lane)) = insertion.clip.take() else {
                    unreachable!("missing channel returns its original lane before the control");
                };
                self.retire_event(EngineEvent::RetiredAutomationLane(lane), allowance);
                ClipInsertionOutcome::MissingChannel
            }
        };
        self.complete_clip_insertion(insertion, outcome, allowance);
    }

    fn begin_clip_insertion(
        &mut self,
        track_id: TrackId,
        target: ClipInsertionTarget,
        mut insertion: Box<PreparedClipInsertion>,
        allowance: &mut RetirementAllowance,
    ) {
        insertion.target = Some((track_id, target));
        let Some(track) = self.insertion_channel_mut(track_id, target) else {
            self.complete_missing_insertion(insertion, allowance);
            return;
        };
        match insertion.try_apply(&mut track.playback_source) {
            ClipInsertionAttempt::Applied(retired_lane) => {
                if let Some(lane) = retired_lane {
                    self.retire_event(EngineEvent::RetiredAutomationLane(lane), allowance);
                }
                self.complete_clip_insertion(insertion, ClipInsertionOutcome::Applied, allowance)
            }
            ClipInsertionAttempt::MissingClip => self.complete_clip_insertion(
                insertion,
                ClipInsertionOutcome::MissingClip,
                allowance,
            ),
            ClipInsertionAttempt::NeedsCapacity(required) => {
                let request = insertion.request(required);
                self.pending_clip_insertion = Some(insertion);
                self.retire_event(
                    EngineEvent::ClipInsertionStorageRequested(request),
                    allowance,
                );
            }
        }
    }

    pub(super) fn continue_clip_insertion(&mut self) {
        if self.pending_retirements.available() == 0 {
            return;
        }
        let Some(insertion) = self.pending_clip_insertion.as_mut() else {
            return;
        };
        let reply = insertion.receive();
        if matches!(reply, ClipInsertionReply::Waiting) {
            return;
        }
        let (track_id, target) = insertion.target.expect("one pending command target");
        let budget = if matches!(reply, ClipInsertionReply::Ready)
            && matches!(target, ClipInsertionTarget::AutomationLane(lane_id)
                if self.channel_mut(track_id).is_none_or(|channel| channel.playback_source.automation.iter().any(|lane| lane.id == lane_id)))
        {
            2
        } else {
            1
        };
        let Some(mut allowance) = self.pending_retirements.reserve(budget) else {
            return;
        };
        // A reply remains inside the original typed control until every return
        // for this phase has room. Waiting never retains callback credits.
        let mut insertion = self
            .pending_clip_insertion
            .take()
            .expect("one pending insertion");
        match reply {
            ClipInsertionReply::Waiting => unreachable!("waiting retained its original control"),
            ClipInsertionReply::Abandoned => {
                self.complete_clip_insertion(
                    insertion,
                    ClipInsertionOutcome::Abandoned,
                    &mut allowance,
                );
            }
            ClipInsertionReply::Ready => {
                if let Some(track) = self.insertion_channel_mut(track_id, target) {
                    // FIFO is paused and runtime Section/Clip rendering restores
                    // Arrange storage before command drain. No writer can grow it.
                    match insertion.try_apply(&mut track.playback_source) {
                        ClipInsertionAttempt::Applied(retired_lane) => {
                            if let Some(lane) = retired_lane {
                                self.retire_event(
                                    EngineEvent::RetiredAutomationLane(lane),
                                    &mut allowance,
                                );
                            }
                            self.complete_clip_insertion(
                                insertion,
                                ClipInsertionOutcome::Applied,
                                &mut allowance,
                            )
                        }
                        ClipInsertionAttempt::MissingClip => self.complete_clip_insertion(
                            insertion,
                            ClipInsertionOutcome::MissingClip,
                            &mut allowance,
                        ),
                        ClipInsertionAttempt::NeedsCapacity(_) => unreachable!(
                            "FIFO and restored Arrange storage keep the prepared reply sufficient"
                        ),
                    }
                } else {
                    self.complete_missing_insertion(insertion, &mut allowance);
                }
            }
        }
    }

    fn complete_clip_insertion(
        &mut self,
        owner: Box<PreparedClipInsertion>,
        outcome: ClipInsertionOutcome,
        allowance: &mut RetirementAllowance,
    ) {
        let recalculate_length =
            owner.affects_length() && outcome != ClipInsertionOutcome::Abandoned;
        let (track_id, target) = owner.target.expect("one insertion outcome target");
        self.retire_event(
            EngineEvent::ClipInsertionCompleted(CompletedClipInsertion {
                track_id,
                target,
                outcome,
                owner,
            }),
            allowance,
        );
        if recalculate_length {
            self.recalculate_audio_length();
        }
    }
}
