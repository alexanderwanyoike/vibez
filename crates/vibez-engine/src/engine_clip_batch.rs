//! Clip packets reuse their transferred storage while acknowledgements wait for UI capacity.

use super::clip_launcher::ClipEventContext;
use super::presentation_queue::PresentationAdmission;
use super::*;
use crate::playback_source::{ActiveClipPlayback, PreparedClipPlayback, QueuedClipPlayback};
use vibez_core::perform::MusicalBoundary;

#[derive(Clone, Copy, PartialEq, Eq)]
enum BatchPhase {
    Staging,
    Waiting,
    Publishing { source: u64, physical: u64 },
}

// Unboxing a main-created source would free its allocation in the callback.
#[allow(clippy::vec_box)]
pub(super) struct PendingClipBatch {
    pub(super) clips: Vec<Box<PreparedClipPlayback>>,
    quantization: MusicalBoundary,
    boundary: u64,
    starting: bool,
    phase: BatchPhase,
}

impl AudioEngine {
    pub(super) fn clip_batch_blocks_commands(&self) -> bool {
        self.pending_clip_batch
            .as_ref()
            .is_some_and(|batch| batch.phase != BatchPhase::Waiting)
    }

    pub(super) fn clip_batch_waiting_boundary(&self) -> bool {
        self.pending_clip_batch
            .as_ref()
            .is_some_and(|batch| batch.phase == BatchPhase::Waiting)
    }

    fn clip_batch_event_room(&self, future: bool) -> usize {
        let scheduled = self.scheduled_presentation.capacity() - self.scheduled_presentation.len();
        let inline = self.presentation_state.terminals.occupied_slots();
        // Inline notifications flush before packet publication, so their
        // reserved ring credits cannot also admit the next retained owner.
        scheduled
            + if future {
                0
            } else {
                self.event_tx.slots().saturating_sub(inline)
            }
    }

    pub(super) fn clip_batch_holds_transition(&self, track: TrackId) -> bool {
        !self.presentation_state.is_faulted()
            && self.pending_clip_batch.as_ref().is_some_and(|batch| {
                matches!(batch.phase, BatchPhase::Publishing { .. })
                    && batch
                        .clips
                        .iter()
                        .any(|prepared| prepared.track_id == track)
            })
    }

    #[allow(clippy::vec_box)]
    pub(super) fn queue_clips(
        &mut self,
        clips: Vec<Box<PreparedClipPlayback>>,
        quantization: MusicalBoundary,
        allowance: &mut RetirementAllowance,
    ) {
        if let Some(batch) = self.pending_clip_batch.take() {
            // Only a completed staging phase permits newer command ingress.
            // Existing unrelated requests retain their own original deadlines.
            debug_assert!(batch.phase == BatchPhase::Waiting);
            for track in &mut self.tracks {
                if let Some(queued) = &mut track.queued_clip {
                    queued.pending_batch = false;
                }
            }
            self.clip_event(EngineEvent::ClipBatchRetired(batch.clips), allowance);
        }
        let starting = !self.clip_performance || !self.transport.is_playing();
        self.begin_clip_performance(allowance);
        let now = self.performance_position;
        let boundary = if starting {
            now
        } else {
            quantization
                .beats()
                .map_or(now, |beats| self.next_achievable_boundary(now, beats))
        };
        self.pending_clip_batch = Some(PendingClipBatch {
            clips,
            quantization,
            boundary,
            starting,
            phase: BatchPhase::Staging,
        });
        self.apply_clip_boundaries(now);
    }

    pub(super) fn continue_clip_batch(&mut self, now: u64) {
        let current_event_is_future = self.rendered_callback_frames != 0;
        let Some(mut batch) = self.pending_clip_batch.take() else {
            return;
        };
        if self.presentation_state.is_faulted() {
            for track in &mut self.tracks {
                if track
                    .queued_clip
                    .as_ref()
                    .is_some_and(|queued| queued.pending_batch)
                {
                    debug_assert!(batch.clips.len() < batch.clips.capacity());
                    batch
                        .clips
                        .push(track.queued_clip.take().expect("batch queue").prepared);
                }
            }
            while !batch.clips.is_empty() {
                if self.clip_batch_event_room(current_event_is_future) == 0 {
                    self.pending_clip_batch = Some(batch);
                    return;
                }
                let Some(admission) = PresentationAdmission::<1>::new_clip_batch(
                    &mut self.event_tx,
                    &mut self.scheduled_presentation,
                    &mut self.presentation_state.terminals,
                    current_event_is_future,
                ) else {
                    self.pending_clip_batch = Some(batch);
                    return;
                };
                let event =
                    EngineEvent::ClipRequestRetired(batch.clips.pop().expect("packet owner"));
                let physical = self.output_position + self.rendered_callback_frames as u64;
                admission.publish_clip(
                    [Some(event)],
                    ClipEventContext {
                        routing: &self.routing,
                        physical,
                        through_request: &mut self.clip_through_request,
                        drops: &mut self.clip_event_drops,
                        resync_track: &mut self.clip_resync_track,
                    },
                    physical,
                    self.output_position,
                );
            }
            if self.clip_batch_event_room(current_event_is_future) == 0 {
                self.pending_clip_batch = Some(batch);
                return;
            }
            let Some(admission) = PresentationAdmission::<1>::new_clip_batch(
                &mut self.event_tx,
                &mut self.scheduled_presentation,
                &mut self.presentation_state.terminals,
                current_event_is_future,
            ) else {
                self.pending_clip_batch = Some(batch);
                return;
            };
            let physical = self.output_position + self.rendered_callback_frames as u64;
            admission.publish_clip(
                [Some(EngineEvent::ClipBatchRetired(batch.clips))],
                ClipEventContext {
                    routing: &self.routing,
                    physical,
                    through_request: &mut self.clip_through_request,
                    drops: &mut self.clip_event_drops,
                    resync_track: &mut self.clip_resync_track,
                },
                physical,
                self.output_position,
            );
            return;
        }
        if batch.phase == BatchPhase::Staging {
            while !batch.clips.is_empty() {
                // One displaced owner and one queue acknowledgement are the
                // worst case. Ownership stays in the packet until both fit.
                if self.clip_batch_event_room(current_event_is_future) < 2 {
                    self.pending_clip_batch = Some(batch);
                    return;
                }
                let Some(admission) = PresentationAdmission::<2>::new_clip_batch(
                    &mut self.event_tx,
                    &mut self.scheduled_presentation,
                    &mut self.presentation_state.terminals,
                    current_event_is_future,
                ) else {
                    self.pending_clip_batch = Some(batch);
                    return;
                };
                let prepared = batch.clips.pop().expect("nonempty packet");
                let Some(track) = self
                    .tracks
                    .iter_mut()
                    .find(|track| track.id == prepared.track_id)
                else {
                    let physical = self.output_position + self.rendered_callback_frames as u64;
                    admission.publish_clip(
                        [Some(EngineEvent::ClipRequestRetired(prepared)), None],
                        ClipEventContext {
                            routing: &self.routing,
                            physical,
                            through_request: &mut self.clip_through_request,
                            drops: &mut self.clip_event_drops,
                            resync_track: &mut self.clip_resync_track,
                        },
                        physical,
                        self.output_position,
                    );
                    continue;
                };
                let event = EngineEvent::ClipQueued {
                    track_id: prepared.track_id,
                    clip_id: prepared.clip_id,
                    request_id: prepared.request_id,
                };
                let old = track.queued_clip.replace(QueuedClipPlayback {
                    pending_batch: true,
                    prepared,
                    effective_at: batch.boundary,
                });
                let retired = old.map(|old| EngineEvent::ClipRequestRetired(old.prepared));
                let physical = self.output_position + self.rendered_callback_frames as u64;
                admission.publish_clip(
                    [retired, Some(event)],
                    ClipEventContext {
                        routing: &self.routing,
                        physical,
                        through_request: &mut self.clip_through_request,
                        drops: &mut self.clip_event_drops,
                        resync_track: &mut self.clip_resync_track,
                    },
                    physical,
                    self.output_position,
                );
            }
            if batch.boundary < now {
                batch.boundary = if batch.starting {
                    now
                } else {
                    batch
                        .quantization
                        .beats()
                        .map_or(now, |beats| self.next_achievable_boundary(now, beats))
                };
            }
            for track in &mut self.tracks {
                if let Some(queued) = track
                    .queued_clip
                    .as_mut()
                    .filter(|queued| queued.pending_batch)
                {
                    queued.effective_at = batch.boundary;
                }
            }
            batch.phase = BatchPhase::Waiting;
        }
        if batch.phase == BatchPhase::Waiting {
            if batch.boundary > now {
                self.pending_clip_batch = Some(batch);
                return;
            }
            // At most one surviving queued source exists per original packet
            // entry. Its emptied capacity can retain every replaced source,
            // allowing one atomic swap even for rows larger than the event ring.
            for track in &mut self.tracks {
                if !track
                    .queued_clip
                    .as_ref()
                    .is_some_and(|queued| queued.pending_batch)
                {
                    continue;
                }
                debug_assert!(batch.clips.len() < batch.clips.capacity());
                let mut prepared = track.queued_clip.take().expect("batch slot").prepared;
                track.flush_notes();
                std::mem::swap(&mut track.launcher_source, &mut prepared.source);
                track.active_clip = prepared.clip_id.map(|clip_id| ActiveClipPlayback {
                    clip_id,
                    request_id: prepared.request_id,
                    position: 0,
                    length: prepared.length_samples.max(1),
                    looping: prepared.looping,
                });
                batch.clips.push(prepared);
            }
            batch.clips.reverse();
            batch.phase = BatchPhase::Publishing {
                source: now,
                physical: self.output_position + self.rendered_callback_frames as u64,
            };
        }
        if let BatchPhase::Publishing { source, physical } = batch.phase {
            while let Some(prepared) = batch.clips.last() {
                let due = physical.saturating_add(self.live_path_latency(prepared.track_id) as u64);
                let current = self.output_position;
                if self.clip_batch_event_room(due > current) == 0 {
                    self.pending_clip_batch = Some(batch);
                    return;
                }
                let Some(admission) = PresentationAdmission::<1>::new_clip_batch(
                    &mut self.event_tx,
                    &mut self.scheduled_presentation,
                    &mut self.presentation_state.terminals,
                    due > current,
                ) else {
                    self.pending_clip_batch = Some(batch);
                    return;
                };
                let prepared = batch.clips.pop().expect("publication owner");
                let event = EngineEvent::ClipTransitioned {
                    track_id: prepared.track_id,
                    clip_id: prepared.clip_id,
                    request_id: prepared.request_id,
                    effective_at_samples: source,
                    retired: Some(prepared),
                };
                let source_now = self.output_position + self.rendered_callback_frames as u64;
                admission.publish_clip(
                    [Some(event)],
                    ClipEventContext {
                        routing: &self.routing,
                        physical,
                        through_request: &mut self.clip_through_request,
                        drops: &mut self.clip_event_drops,
                        resync_track: &mut self.clip_resync_track,
                    },
                    source_now,
                    self.output_position,
                );
            }
            if self.clip_batch_event_room(current_event_is_future) == 0 {
                self.pending_clip_batch = Some(batch);
                return;
            }
            let Some(admission) = PresentationAdmission::<1>::new_clip_batch(
                &mut self.event_tx,
                &mut self.scheduled_presentation,
                &mut self.presentation_state.terminals,
                current_event_is_future,
            ) else {
                self.pending_clip_batch = Some(batch);
                return;
            };
            let physical = self.output_position + self.rendered_callback_frames as u64;
            admission.publish_clip(
                [Some(EngineEvent::ClipBatchRetired(batch.clips))],
                ClipEventContext {
                    routing: &self.routing,
                    physical,
                    through_request: &mut self.clip_through_request,
                    drops: &mut self.clip_event_drops,
                    resync_track: &mut self.clip_resync_track,
                },
                physical,
                self.output_position,
            );
        }
    }
}

#[cfg(test)]
#[path = "engine_clip_batch_tests.rs"]
mod tests;
