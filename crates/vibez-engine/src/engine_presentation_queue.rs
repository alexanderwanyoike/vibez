//! Bounded source event ownership and delivery survive UI backpressure.

use super::*;
use crate::events::{PerformanceCaptureStartedEvent, SectionTransitionedEvent};

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
        EngineEvent::SectionTransitioned(SectionTransitionedEvent { retired, .. }) => {
            EngineEvent::SectionQueueCancelled {
                retired: retired.take(),
            }
        }
        EngineEvent::ClipTransitioned {
            retired: Some(retired),
            ..
        } => EngineEvent::ClipRequestRetired(retired),
        EngineEvent::SectionCaptureSource { offsets, .. }
        | EngineEvent::PerformanceCaptureStarted(PerformanceCaptureStartedEvent {
            offsets, ..
        }) => EngineEvent::CaptureTimingRetired(offsets),
        EngineEvent::PlaybackStarted
        | EngineEvent::ClipTransitioned { .. }
        | EngineEvent::ClipSourceRefreshed { .. }
        | EngineEvent::ClipCaptureSource { .. }
        | EngineEvent::SectionCaptureStopped { .. }
        | EngineEvent::PerformanceCaptureStopped { .. }
        | EngineEvent::InstrumentNoteInput(_)
        | EngineEvent::NoteRepeated(_)
        | EngineEvent::AutomationGestureChanged(_) => EngineEvent::PresentationCancelled,
        event => event,
    }
}

fn source_feed(event: &EngineEvent) -> bool {
    matches!(
        event,
        EngineEvent::SourceNoteInput(_) | EngineEvent::SourceNoteRepeated(_)
    )
}

fn capture_feed(event: &EngineEvent) -> bool {
    matches!(
        event,
        EngineEvent::PerformanceCaptureStarted(_)
            | EngineEvent::PerformanceCaptureStopped { .. }
            | EngineEvent::InstrumentNoteInput(_)
            | EngineEvent::NoteRepeated(_)
            | EngineEvent::AutomationGestureChanged(_)
            | EngineEvent::TrackMuteChanged(_)
            | EngineEvent::SectionCaptureSource { .. }
            | EngineEvent::SectionCaptureStopped { .. }
            | EngineEvent::ClipCaptureSource { .. }
    )
}

fn ready(pending: &ScheduledPresentation, source_now: u64, heard_now: u64) -> bool {
    pending.due
        <= if source_feed(&pending.event) {
            source_now
        } else {
            heard_now
        }
}

fn flush_ready(
    events: &mut rtrb::Producer<EngineEvent>,
    scheduled: &mut Vec<ScheduledPresentation>,
    eligible: impl Fn(&ScheduledPresentation) -> bool,
) {
    let mut slots = events.slots();
    // Stable compaction visits each retained owner once, even when immediate
    // recording feeds sit beyond future heard events in physical-time order.
    scheduled.retain_mut(|pending| {
        if slots == 0 || !eligible(pending) {
            return true;
        }
        let event = std::mem::replace(&mut pending.event, EngineEvent::PresentationCancelled);
        let result = events.push(event);
        debug_assert!(result.is_ok());
        slots -= 1;
        false
    });
}

fn queue_event(
    event: EngineEvent,
    events: &mut rtrb::Producer<EngineEvent>,
    scheduled: &mut Vec<ScheduledPresentation>,
    now: u64,
    due: u64,
    heard_now: u64,
) -> Option<EngineEvent> {
    let cutoff = if source_feed(&event) { now } else { heard_now };
    let event = if due <= cutoff
        && !scheduled
            .iter()
            .any(|pending| ready(pending, now, heard_now))
    {
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
    source: EngineEvent,
    heard: EngineEvent,
    events: &mut rtrb::Producer<EngineEvent>,
    scheduled: &mut Vec<ScheduledPresentation>,
    now: u64,
    due: u64,
    heard_now: u64,
) -> bool {
    debug_assert_eq!(source.time_domain(), crate::events::TimeDomain::Source);
    debug_assert_eq!(heard.time_domain(), crate::events::TimeDomain::Heard);
    flush_ready(events, scheduled, |pending| ready(pending, now, heard_now));
    if scheduled.capacity() - scheduled.len() < 2 {
        return false;
    }
    queue_event(source, events, scheduled, now, now, heard_now).is_none()
        && queue_event(heard, events, scheduled, now, due, heard_now).is_none()
}

impl AudioEngine {
    pub(super) fn present_event(&mut self, event: EngineEvent) {
        let now = self.output_position + self.rendered_callback_frames as u64;
        self.present_event_at(event, now);
    }

    pub(super) fn present_event_after(&mut self, event: EngineEvent, delay: u32) {
        let now = self.output_position + self.rendered_callback_frames as u64;
        self.present_event_at(event, now.saturating_add(delay as u64));
    }

    pub(super) fn present_event_at(&mut self, event: EngineEvent, due: u64) {
        debug_assert!(
            due <= self.output_position + self.rendered_callback_frames as u64
                || event.time_domain() == crate::events::TimeDomain::Heard,
            "only heard-time events wait for output latency: {event:?}"
        );
        if self.presentation_state.is_faulted() {
            match event {
                EngineEvent::PlaybackStopped => {
                    self.presentation_state.terminals.request_playback_stop();
                    self.flush_presentation();
                    return;
                }
                EngineEvent::PerformanceCaptureStopped {
                    effective_at_samples,
                } => {
                    self.presentation_state
                        .terminals
                        .request_capture_stop(effective_at_samples);
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
            self.output_position,
        ) {
            self.fail_presentation();
            // Ownership producers reserve before handoff. Retain the discovering
            // event separately while the full UI ring holds the scheduled owners.
            let event = cancelled_event(event);
            if let Some(event) = self
                .presentation_state
                .terminals
                .retain_overflow_owner(event)
            {
                // Cancelled feedback owns nothing, so it must not consume the
                // retirement space reserved for owners awaiting the UI.
                if !matches!(event, EngineEvent::PresentationCancelled) {
                    self.retire_event(event);
                }
            }
        }
    }

    pub(super) fn flush_presentation(&mut self) {
        if self.presentation_state.terminals.capture_stop().is_some() {
            flush_ready(
                &mut self.event_tx,
                &mut self.scheduled_presentation,
                |pending| pending.due <= self.output_position && capture_feed(&pending.event),
            );
            if self
                .scheduled_presentation
                .iter()
                .any(|pending| pending.due <= self.output_position && capture_feed(&pending.event))
            {
                return;
            }
        }
        if !self
            .presentation_state
            .terminals
            .publish(&mut self.event_tx)
        {
            return;
        }
        let source_now = self.output_position + self.rendered_callback_frames as u64;
        flush_ready(
            &mut self.event_tx,
            &mut self.scheduled_presentation,
            |pending| ready(pending, source_now, self.output_position),
        );
    }

    pub(super) fn fail_presentation(&mut self) {
        if !self.presentation_state.enter_fault() {
            return;
        }
        self.capture_lifecycle.end();
        self.transport.stop();
        for track in &mut self.tracks {
            track.active_clip = None;
            track.flush_notes();
        }
        self.clip_resync_track = Some(0);
        self.cancel_presentation();
        self.presentation_state
            .terminals
            .request_capture_stop(self.compensation_callback_heard_start);
        self.presentation_state.terminals.request_playback_stop();
        self.presentation_state.terminals.request_failure(
            TrackId::MASTER,
            None,
            "Audio presentation event history is full",
        );
        self.flush_presentation();
    }

    pub(super) fn presentation_room(&mut self, events: usize) -> bool {
        if self.presentation_state.is_faulted() {
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
        self.cancel_capture_timing();
        for pending in &mut self.scheduled_presentation {
            let event = std::mem::replace(&mut pending.event, EngineEvent::PresentationCancelled);
            pending.event = if pending.due <= self.output_position && capture_feed(&event) {
                event
            } else {
                cancelled_event(event)
            };
            pending.due = self.output_position;
        }
        // Tombstones hold no owner or state and need no UI slot. Reclaiming
        // them leaves the reserved overflow holder for actual ownership.
        self.scheduled_presentation
            .retain(|pending| !matches!(pending.event, EngineEvent::PresentationCancelled));
    }

    pub(super) fn cancel_capture_presentation(&mut self) {
        self.cancel_capture_timing();
        // Preserve physical deadlines so Capture cancellation cannot advance
        // unrelated public state or invalidate the queue's sorted prefix.
        for pending in &mut self.scheduled_presentation {
            let event = std::mem::replace(&mut pending.event, EngineEvent::PresentationCancelled);
            pending.event = match event {
                event if pending.due <= self.output_position && capture_feed(&event) => event,
                EngineEvent::SectionCaptureSource { offsets, .. }
                | EngineEvent::PerformanceCaptureStarted(PerformanceCaptureStartedEvent {
                    offsets,
                    ..
                }) => EngineEvent::CaptureTimingRetired(offsets),
                EngineEvent::ClipCaptureSource { .. }
                | EngineEvent::SectionCaptureStopped { .. }
                | EngineEvent::PerformanceCaptureStopped { .. }
                | EngineEvent::AutomationGestureChanged(_) => EngineEvent::PresentationCancelled,
                event => event,
            };
        }
        self.scheduled_presentation
            .retain(|pending| !matches!(pending.event, EngineEvent::PresentationCancelled));
    }
}
