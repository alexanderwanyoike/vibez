//! Bounded source event ownership and delivery survive UI backpressure.

use super::*;

pub(super) struct ScheduledPresentation {
    pub(super) due: u64,
    pub(super) event: EngineEvent,
}

pub(super) const PRESENTATION_EVENT_CAPACITY: usize = 2048;
// Track-sized cleanup pauses before consuming the slots needed by the remaining
// fixed command body (recording, Section, Capture and transport acknowledgements).
pub(super) const COMMAND_PRESENTATION_RESERVE: usize = 16;

fn cancelled_event(event: EngineEvent) -> EngineEvent {
    match event {
        EngineEvent::SectionTransitioned { retired, .. } => {
            EngineEvent::SectionQueueCancelled { retired }
        }
        EngineEvent::ClipTransitioned {
            retired: Some(retired),
            ..
        } => EngineEvent::ClipRequestRetired(retired),
        EngineEvent::PlaybackStarted
        | EngineEvent::ClipTransitioned { .. }
        | EngineEvent::ClipSourceRefreshed { .. }
        | EngineEvent::ClipCaptureSource { .. }
        | EngineEvent::PerformanceCaptureStarted { .. }
        | EngineEvent::PerformanceCaptureStopped { .. }
        | EngineEvent::InstrumentNoteInput { .. }
        | EngineEvent::NoteRepeated { .. }
        | EngineEvent::AutomationGestureChanged { .. } => EngineEvent::PresentationCancelled,
        event => event,
    }
}

fn queue_event(
    event: EngineEvent,
    events: &mut rtrb::Producer<EngineEvent>,
    scheduled: &mut Vec<ScheduledPresentation>,
    now: u64,
    due: u64,
) -> Option<EngineEvent> {
    let event = if due <= now && scheduled.first().is_none_or(|pending| pending.due > now) {
        match events.push(event) {
            Ok(()) => return None,
            Err(rtrb::PushError::Full(event)) => event,
        }
    } else {
        event
    };
    if scheduled.len() == scheduled.capacity() {
        return Some(event);
    }
    let index = scheduled.partition_point(|pending| pending.due <= due);
    scheduled.insert(index, ScheduledPresentation { due, event });
    None
}

pub(super) fn emit_repeated(
    event: EngineEvent,
    events: &mut rtrb::Producer<EngineEvent>,
    scheduled: &mut Vec<ScheduledPresentation>,
    now: u64,
) -> bool {
    if scheduled.len() == scheduled.capacity() {
        return false;
    }
    queue_event(event, events, scheduled, now, now).is_none()
}

impl AudioEngine {
    pub(super) fn present_event(&mut self, event: EngineEvent) {
        let now = self.output_position + self.rendered_callback_frames as u64;
        self.present_event_at(event, now);
    }

    pub(super) fn present_event_at(&mut self, event: EngineEvent, due: u64) {
        if self.presentation_fault {
            match event {
                EngineEvent::PlaybackStopped => {
                    self.pending_playback_stop = true;
                    self.flush_presentation();
                    return;
                }
                EngineEvent::PerformanceCaptureStopped {
                    effective_at_samples,
                } => {
                    self.pending_capture_stop
                        .get_or_insert(effective_at_samples);
                    self.flush_presentation();
                    return;
                }
                _ => {}
            }
        }
        self.flush_presentation();
        let now = self.output_position + self.rendered_callback_frames as u64;
        if let Some(event) = queue_event(
            event,
            &mut self.event_tx,
            &mut self.scheduled_presentation,
            now,
            due,
        ) {
            self.fail_presentation();
            // Ownership producers reserve before handoff. Retain the discovering
            // event separately while the full UI ring holds the scheduled owners.
            let event = cancelled_event(event);
            if self.presentation_overflow_owner.is_none() {
                self.presentation_overflow_owner = Some(event);
            } else {
                self.retire_event(event);
            }
        }
    }

    pub(super) fn flush_presentation(&mut self) {
        if let Some(effective_at_samples) = self.pending_capture_stop {
            if self
                .event_tx
                .push(EngineEvent::PerformanceCaptureStopped {
                    effective_at_samples,
                })
                .is_ok()
            {
                self.pending_capture_stop = None;
            } else {
                return;
            }
        }
        if let Some((track_id, effect_id, reason)) = self.pending_compensation_failure {
            if self
                .event_tx
                .push(EngineEvent::CompensationInvalid {
                    track_id,
                    effect_id,
                    reason,
                })
                .is_ok()
            {
                self.pending_compensation_failure = None;
            } else {
                return;
            }
        }
        if self.pending_playback_stop {
            if self.event_tx.push(EngineEvent::PlaybackStopped).is_err() {
                return;
            }
            self.pending_playback_stop = false;
        }
        if let Some(event) = self.presentation_overflow_owner.take() {
            if let Err(rtrb::PushError::Full(event)) = self.event_tx.push(event) {
                self.presentation_overflow_owner = Some(event);
                return;
            }
        }
        let due = self.scheduled_presentation.partition_point(|pending| {
            pending.due <= self.output_position + self.rendered_callback_frames as u64
        });
        let delivered = due.min(self.event_tx.slots());
        // The producer owns these free slots, so one drain moves the prefix once
        // and never drops an undelivered owner on the rendering thread.
        for pending in self.scheduled_presentation.drain(..delivered) {
            let result = self.event_tx.push(pending.event);
            debug_assert!(result.is_ok());
        }
    }

    pub(super) fn fail_presentation(&mut self) {
        if self.presentation_fault {
            return;
        }
        self.presentation_fault = true;
        self.transport.stop();
        for track in &mut self.tracks {
            track.active_clip = None;
            track.flush_notes();
        }
        self.clip_resync_track = Some(0);
        self.cancel_presentation();
        self.pending_capture_stop
            .get_or_insert(self.compensation_callback_heard_start);
        self.pending_playback_stop = true;
        self.pending_compensation_failure.get_or_insert((
            TrackId::MASTER,
            None,
            "Audio presentation event history is full",
        ));
        self.flush_presentation();
    }

    pub(super) fn presentation_room(&mut self, events: usize) -> bool {
        if self.presentation_fault {
            return false;
        }
        if self.scheduled_presentation.capacity() - self.scheduled_presentation.len() < events {
            self.fail_presentation();
            false
        } else {
            true
        }
    }

    pub(super) fn cancel_presentation(&mut self) {
        for pending in &mut self.scheduled_presentation {
            let event = std::mem::replace(&mut pending.event, EngineEvent::PresentationCancelled);
            pending.event = cancelled_event(event);
            pending.due = self.output_position;
        }
        // Tombstones hold no owner or state and need no UI slot. Reclaiming
        // them leaves the reserved overflow holder for actual ownership.
        self.scheduled_presentation
            .retain(|pending| !matches!(pending.event, EngineEvent::PresentationCancelled));
    }

    pub(super) fn cancel_capture_presentation(&mut self) {
        self.scheduled_presentation.retain(|pending| {
            !matches!(
                pending.event,
                EngineEvent::PerformanceCaptureStarted { .. }
                    | EngineEvent::PerformanceCaptureStopped { .. }
                    | EngineEvent::ClipCaptureSource { .. }
            )
        });
    }
}
